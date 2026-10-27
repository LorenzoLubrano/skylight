//! Misure per la pagina Sistema. Il campionatore lavora solo mentre la pagina è
//! aperta (una misura al secondo); chiusa la pagina, il thread dorme.
//!
//! Fonti, tutte senza driver né permessi di amministratore:
//! - CPU: `GetSystemTimes`; RAM: `GlobalMemoryStatusEx`; rete: `GetIfTable2`.
//! - Disco, GPU, zona termica, misuratore di energia: contatori di prestazioni (PDH),
//!   gli stessi di Gestione attività e Performance Monitor.
//! - Temperatura GPU: driver video via D3DKMT (come Gestione attività).
//! - Consumo dalla batteria: WMI `BatteryStatus`.
//! - Se è aperto LibreHardwareMonitor (o OpenHardwareMonitor): i suoi sensori
//!   precisi per temperatura e consumo di CPU e GPU.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use windows::Wdk::Graphics::Direct3D::{
    D3DKMT_ADAPTER_PERFDATA, D3DKMT_CLOSEADAPTER, D3DKMT_OPENADAPTERFROMLUID,
    D3DKMT_QUERYADAPTERINFO, D3DKMTCloseAdapter, D3DKMTOpenAdapterFromLuid, D3DKMTQueryAdapterInfo,
    KMTQAITYPE_ADAPTERPERFDATA,
};
use windows::Win32::Foundation::{FILETIME, NO_ERROR};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::Performance::*;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;
use windows::Win32::System::Wmi::IWbemServices;
use windows::core::{HSTRING, PCWSTR};

use super::energy::EnergyMeters;
use super::{Notifier, wmi};
use crate::features::sysinfo::{
    PowerSource, Sensor, Stats, TempSource, gpu_utilization, pick_sensors, thermal_zone_celsius,
};
use crate::util::Inbox;

pub enum Control {
    Start,
    Stop,
}

pub fn spawn(inbox: Arc<Inbox<Stats>>, notifier: Notifier) -> Sender<Control> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("sysmon".into())
        .spawn(move || run(rx, &inbox, notifier));
    tx
}

fn run(rx: Receiver<Control>, inbox: &Inbox<Stats>, notifier: Notifier) {
    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
        return;
    }
    let mut sampler: Option<Sampler> = None;
    loop {
        let message = match &sampler {
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(_) => rx.recv_timeout(Duration::from_secs(1)),
        };
        match message {
            Ok(Control::Start) => {
                // La prima misura arriva dopo un secondo: con un intervallo
                // quasi nullo CPU e velocità sarebbero senza senso.
                if sampler.is_none() {
                    sampler = Some(Sampler::new());
                }
            }
            Ok(Control::Stop) => sampler = None,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(s) = &mut sampler {
                    inbox.push(s.sample());
                    notifier.notify();
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

struct Sampler {
    pdh: Pdh,
    cpu_previous: Option<(u64, u64)>,
    /// Contatori di ogni scheda all'ultima misura (per LUID) e quando.
    net_previous: (HashMap<u64, (u64, u64)>, Instant),
    wmi: Option<IWbemServices>,
    sensors: Option<IWbemServices>,
    gpus: Vec<u32>,
    /// Contatori di energia del processore: ci sono anche col caricatore collegato.
    energy: EnergyMeters,
}

impl Sampler {
    fn new() -> Self {
        let sensors = ["ROOT\\LibreHardwareMonitor", "ROOT\\OpenHardwareMonitor"]
            .iter()
            .find_map(|ns| wmi::connect(ns).ok());
        let sampler = Self {
            pdh: Pdh::open(),
            cpu_previous: cpu_times(),
            net_previous: (network_counters(), Instant::now()),
            wmi: wmi::connect("ROOT\\WMI").ok(),
            sensors,
            gpus: open_gpus(),
            energy: EnergyMeters::open(),
        };
        sampler.pdh.collect();
        let mut sampler = sampler;
        // La prima lettura fa da punto di partenza per la potenza media.
        let _ = sampler.energy.read();
        sampler
    }

    fn sample(&mut self) -> Stats {
        let mut stats = Stats::default();

        let now = cpu_times();
        if let (Some((idle, total)), Some((idle0, total0))) = (now, self.cpu_previous) {
            let elapsed = total.saturating_sub(total0);
            let busy = elapsed.saturating_sub(idle.saturating_sub(idle0));
            if elapsed > 0 {
                stats.cpu = Some((busy as f64 / elapsed as f64).clamp(0.0, 1.0) as f32);
            }
        }
        self.cpu_previous = now;

        let mut memory = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if unsafe { GlobalMemoryStatusEx(&mut memory) }.is_ok() {
            stats.ram_total = memory.ullTotalPhys;
            stats.ram_used = memory.ullTotalPhys.saturating_sub(memory.ullAvailPhys);
        }

        // Solo le schede presenti anche nella misura precedente: una che si
        // riconnette riporterebbe di colpo tutti i byte dall'accensione.
        let counters = network_counters();
        let now = Instant::now();
        let (previous, at) = &self.net_previous;
        let seconds = (now - *at).as_secs_f64().max(0.001);
        let (mut down, mut up) = (0u64, 0u64);
        for (luid, (received, sent)) in &counters {
            if let Some((received0, sent0)) = previous.get(luid) {
                down += received.saturating_sub(*received0);
                up += sent.saturating_sub(*sent0);
            }
        }
        stats.download = Some(down as f64 / seconds);
        stats.upload = Some(up as f64 / seconds);
        self.net_previous = (counters, now);

        self.pdh.collect();
        stats.disk_busy = self
            .pdh
            .single(self.pdh.disk_idle)
            .map(|idle| (1.0 - idle / 100.0).clamp(0.0, 1.0) as f32);
        stats.disk_io = match (
            self.pdh.single(self.pdh.disk_read),
            self.pdh.single(self.pdh.disk_write),
        ) {
            (None, None) => None,
            (r, w) => Some(r.unwrap_or(0.0) + w.unwrap_or(0.0)),
        };
        stats.gpu = self
            .pdh
            .gpu_engine
            .and_then(|c| gpu_utilization(&self.pdh.array(c)));
        stats.gpu_memory = self
            .pdh
            .gpu_memory
            .map(|c| {
                self.pdh
                    .array(c)
                    .iter()
                    .map(|(_, v)| *v)
                    .fold(0.0, f64::max) as u64
            })
            .filter(|&bytes| bytes > 0);

        // Temperature e consumo: dai sensori precisi se ci sono, altrimenti dalle fonti di Windows.
        let picked = self
            .sensors
            .as_ref()
            .map(|s| pick_sensors(&read_sensors(s)))
            .unwrap_or_default();
        if let Some(t) = picked.cpu_temp {
            stats.cpu_temp = Some(t);
            stats.cpu_temp_source = Some(TempSource::Sensor);
        } else if let Some(t) = self
            .pdh
            .thermal
            .and_then(|c| thermal_zone_celsius(&self.pdh.values(c)))
        {
            stats.cpu_temp = Some(t);
            stats.cpu_temp_source = Some(TempSource::ThermalZone);
        }
        if let Some(t) = gpu_temperature(&self.gpus) {
            stats.gpu_temp = Some(t);
            stats.gpu_temp_source = Some(TempSource::Driver);
        } else if let Some(t) = picked.gpu_temp {
            stats.gpu_temp = Some(t);
            stats.gpu_temp_source = Some(TempSource::Sensor);
        }

        let battery = self.wmi.as_ref().and_then(battery_discharge_watts);
        let meter = self
            .pdh
            .power_meter
            .map(|c| self.pdh.array(c))
            .and_then(|v| meter_watts(&v));
        let sensors = match (picked.cpu_power, picked.gpu_power) {
            (None, None) => None,
            (c, g) => Some(c.unwrap_or(0.0) + g.unwrap_or(0.0)),
        };
        let energy = if self.energy.is_empty() {
            None
        } else {
            self.energy.read()
        };
        // Dalla misura più completa: tutto il portatile (batteria), tutto il PC,
        // la piattaforma, CPU + GPU dai sensori, il solo processore.
        let platform = energy
            .as_ref()
            .filter(|r| r.source == PowerSource::Platform);
        (stats.power_w, stats.power_source) = if let Some(w) = battery {
            (Some(w), Some(PowerSource::Battery))
        } else if let Some(w) = meter {
            (Some(w), Some(PowerSource::Meter))
        } else if let Some(r) = platform {
            (Some(r.watts), Some(r.source))
        } else if let Some(w) = sensors {
            (Some(w), Some(PowerSource::Sensors))
        } else if let Some(r) = &energy {
            (Some(r.watts), Some(r.source))
        } else {
            (None, None)
        };
        stats
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        for &adapter in &self.gpus {
            let _ = unsafe { D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: adapter }) };
        }
    }
}

// ---- CPU, rete --------------------------------------------------------------------

fn filetime(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

/// (tempo inattivo, tempo totale) dall'avvio; il tempo "kernel" include l'inattività.
fn cpu_times() -> Option<(u64, u64)> {
    let (mut idle, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    Some((filetime(idle), filetime(kernel) + filetime(user)))
}

/// Byte ricevuti e inviati da ogni scheda di rete fisica attiva, per LUID.
fn network_counters() -> HashMap<u64, (u64, u64)> {
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        let mut counters = HashMap::new();
        if GetIfTable2(&mut table) != NO_ERROR || table.is_null() {
            return counters;
        }
        let rows =
            std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        for row in rows {
            let flags = row.InterfaceAndOperStatusFlags._bitfield;
            let hardware = flags & 0b01 != 0;
            // Le interfacce "filtro" duplicano il traffico della scheda vera.
            let filter = flags & 0b10 != 0;
            if hardware
                && !filter
                && row.OperStatus == IfOperStatusUp
                && row.Type != IF_TYPE_SOFTWARE_LOOPBACK
            {
                counters.insert(row.InterfaceLuid.Value, (row.InOctets, row.OutOctets));
            }
        }
        FreeMibTable(table as *const _);
        counters
    }
}

// ---- Contatori di prestazioni (PDH) ---------------------------------------------------

struct Pdh {
    query: Option<PDH_HQUERY>,
    disk_idle: Option<PDH_HCOUNTER>,
    disk_read: Option<PDH_HCOUNTER>,
    disk_write: Option<PDH_HCOUNTER>,
    gpu_engine: Option<PDH_HCOUNTER>,
    gpu_memory: Option<PDH_HCOUNTER>,
    thermal: Option<PDH_HCOUNTER>,
    power_meter: Option<PDH_HCOUNTER>,
}

impl Pdh {
    fn open() -> Self {
        let mut query = PDH_HQUERY::default();
        let opened = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } == 0;
        let query = opened.then_some(query);
        // Nomi inglesi: funzionano con Windows in qualsiasi lingua.
        let add = |path: &str| -> Option<PDH_HCOUNTER> {
            let mut counter = PDH_HCOUNTER::default();
            let status =
                unsafe { PdhAddEnglishCounterW(query?, &HSTRING::from(path), 0, &mut counter) };
            (status == 0).then_some(counter)
        };
        Self {
            disk_idle: add("\\PhysicalDisk(_Total)\\% Idle Time"),
            disk_read: add("\\PhysicalDisk(_Total)\\Disk Read Bytes/sec"),
            disk_write: add("\\PhysicalDisk(_Total)\\Disk Write Bytes/sec"),
            gpu_engine: add("\\GPU Engine(*)\\Utilization Percentage"),
            gpu_memory: add("\\GPU Adapter Memory(*)\\Dedicated Usage"),
            thermal: add("\\Thermal Zone Information(*)\\High Precision Temperature"),
            power_meter: add("\\Power Meter(*)\\Power"),
            query,
        }
    }

    fn collect(&self) {
        if let Some(query) = self.query {
            unsafe { PdhCollectQueryData(query) };
        }
    }

    fn single(&self, counter: Option<PDH_HCOUNTER>) -> Option<f64> {
        let mut value = PDH_FMT_COUNTERVALUE::default();
        let status =
            unsafe { PdhGetFormattedCounterValue(counter?, PDH_FMT_DOUBLE, None, &mut value) };
        let valid =
            value.CStatus == PDH_CSTATUS_VALID_DATA || value.CStatus == PDH_CSTATUS_NEW_DATA;
        (status == 0 && valid).then_some(unsafe { value.Anonymous.doubleValue })
    }

    /// Tutte le istanze di un contatore con caratteri jolly, come (nome, valore).
    fn array(&self, counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
        unsafe {
            let (mut size, mut count) = (0u32, 0u32);
            let status =
                PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None);
            if status != PDH_MORE_DATA || size == 0 {
                return Vec::new();
            }
            // Buffer allineato a 8 byte: contiene le strutture e, in coda, i nomi.
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            let items = buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                Some(items),
            ) != 0
            {
                return Vec::new();
            }
            std::slice::from_raw_parts(items, count as usize)
                .iter()
                .filter(|item| {
                    item.FmtValue.CStatus == PDH_CSTATUS_VALID_DATA
                        || item.FmtValue.CStatus == PDH_CSTATUS_NEW_DATA
                })
                .map(|item| {
                    (
                        item.szName.to_string().unwrap_or_default(),
                        item.FmtValue.Anonymous.doubleValue,
                    )
                })
                .collect()
        }
    }

    fn values(&self, counter: PDH_HCOUNTER) -> Vec<f64> {
        self.array(counter).into_iter().map(|(_, v)| v).collect()
    }
}

impl Drop for Pdh {
    fn drop(&mut self) {
        if let Some(query) = self.query {
            unsafe { PdhCloseQuery(query) };
        }
    }
}

/// Il misuratore di Windows riporta milliwatt; "_Total" se c'è, altrimenti la somma.
fn meter_watts(instances: &[(String, f64)]) -> Option<f32> {
    let total = match instances.iter().find(|(name, _)| name == "_Total") {
        Some((_, value)) => *value,
        None => instances.iter().map(|(_, v)| v).sum(),
    };
    (total > 0.0).then(|| (total / 1000.0) as f32)
}

// ---- Batteria e sensori (WMI) ---------------------------------------------------------

/// Consumo di tutto il portatile mentre va a batteria (la scarica, in milliwatt).
fn battery_discharge_watts(services: &IWbemServices) -> Option<f32> {
    let batteries = wmi::query(
        services,
        "SELECT DischargeRate, Discharging FROM BatteryStatus",
    );
    let watts: f64 = batteries
        .iter()
        .filter(|b| wmi::boolean(b, "Discharging").unwrap_or(false))
        .filter_map(|b| wmi::number(b, "DischargeRate"))
        .filter(|&mw| mw > 0.0)
        .map(|mw| mw / 1000.0)
        .sum();
    (watts > 0.0).then_some(watts as f32)
}

fn read_sensors(services: &IWbemServices) -> Vec<Sensor> {
    wmi::query(
        services,
        "SELECT Identifier, Name, SensorType, Value FROM Sensor WHERE SensorType='Temperature' OR SensorType='Power'",
    )
    .iter()
    .filter_map(|s| {
        Some(Sensor {
            identifier: wmi::string(s, "Identifier")?,
            name: wmi::string(s, "Name")?,
            kind: wmi::string(s, "SensorType")?,
            value: wmi::number(s, "Value")? as f32,
        })
    })
    .collect()
}

// ---- Temperatura GPU dal driver video ------------------------------------------------

/// Apre le schede video fisiche (esclude il rasterizzatore software).
fn open_gpus() -> Vec<u32> {
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };
    let mut adapters = Vec::new();
    for index in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
            break;
        };
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let mut open = D3DKMT_OPENADAPTERFROMLUID {
            AdapterLuid: desc.AdapterLuid,
            hAdapter: 0,
        };
        if unsafe { D3DKMTOpenAdapterFromLuid(&mut open) }.is_ok() {
            adapters.push(open.hAdapter);
        }
    }
    adapters
}

/// La stessa lettura di Gestione attività; disponibile se il driver la supporta
/// (di solito le schede video dedicate).
fn gpu_temperature(adapters: &[u32]) -> Option<f32> {
    adapters
        .iter()
        .filter_map(|&adapter| {
            let mut perf = D3DKMT_ADAPTER_PERFDATA {
                PhysicalAdapterIndex: 0,
                ..Default::default()
            };
            let mut query = D3DKMT_QUERYADAPTERINFO {
                hAdapter: adapter,
                Type: KMTQAITYPE_ADAPTERPERFDATA,
                pPrivateDriverData: &mut perf as *mut D3DKMT_ADAPTER_PERFDATA as *mut _,
                PrivateDriverDataSize: size_of::<D3DKMT_ADAPTER_PERFDATA>() as u32,
            };
            let ok = unsafe { D3DKMTQueryAdapterInfo(&mut query) }.is_ok();
            // Decimi di grado Celsius.
            (ok && perf.Temperature > 0).then(|| perf.Temperature as f32 / 10.0)
        })
        .filter(|t| (1.0..=125.0).contains(t))
        .reduce(f32::max)
}
