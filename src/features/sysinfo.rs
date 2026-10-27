//! Pagina Sistema: valori misurati, storico per i mini-grafici e formattazione.
//! Le misure vere le fa `win::sysmon`; qui c'è solo la logica, testabile ovunque.

use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerSource {
    /// Scarica della batteria: è il consumo di tutto il portatile.
    Battery,
    /// Contatore "Power Meter" di Windows (ACPI): raro, misura tutto il PC.
    Meter,
    /// CPU + GPU letti da LibreHardwareMonitor.
    Sensors,
    /// Contatori di energia del processore (RAPL): tutta la piattaforma ("PSYS")...
    Platform,
    /// ...oppure solo il processore con la grafica integrata ("PKG").
    Processor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempSource {
    /// LibreHardwareMonitor (preciso).
    Sensor,
    /// Zona termica ACPI: indicativa, non è la temperatura del die.
    ThermalZone,
    /// Driver video, come in Gestione attività.
    Driver,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Frazioni tra 0 e 1.
    pub cpu: Option<f32>,
    pub ram_used: u64,
    pub ram_total: u64,
    pub disk_busy: Option<f32>,
    /// Byte al secondo.
    pub disk_io: Option<f64>,
    pub gpu: Option<f32>,
    pub gpu_memory: Option<u64>,
    pub download: Option<f64>,
    pub upload: Option<f64>,
    pub power_w: Option<f32>,
    pub power_source: Option<PowerSource>,
    pub cpu_temp: Option<f32>,
    pub cpu_temp_source: Option<TempSource>,
    pub gpu_temp: Option<f32>,
    pub gpu_temp_source: Option<TempSource>,
}

impl Stats {
    pub fn ram(&self) -> Option<f32> {
        (self.ram_total > 0).then(|| self.ram_used as f32 / self.ram_total as f32)
    }
}

/// Utilizzo GPU come lo calcola Gestione attività: per ogni motore fisico
/// (3D, Copy, Video Decode... di ciascuna scheda) si sommano i processi, poi si
/// prende il massimo. Le istanze si chiamano tipo
/// `pid_42_luid_0x0_0xD1A6_phys_0_eng_0_engtype_3D`: il motore è il tratto
/// da `luid_` a `_engtype`, così due motori dello stesso tipo restano separati.
pub fn gpu_utilization(instances: &[(String, f64)]) -> Option<f32> {
    if instances.is_empty() {
        return None;
    }
    let mut totals: Vec<(&str, f64)> = Vec::new();
    for (name, value) in instances {
        let engine = name.split_once("luid_").map_or(name.as_str(), |(_, rest)| {
            rest.split("_engtype").next().unwrap_or(rest)
        });
        match totals.iter_mut().find(|(e, _)| *e == engine) {
            Some(entry) => entry.1 += value.max(0.0),
            None => totals.push((engine, value.max(0.0))),
        }
    }
    let max = totals.iter().map(|t| t.1).fold(0.0, f64::max);
    Some((max / 100.0).clamp(0.0, 1.0) as f32)
}

/// Un canale dei contatori di energia di Windows (es. "RAPL_Package0_PKG") con la
/// potenza media dall'ultima lettura.
#[derive(Clone, Debug, PartialEq)]
pub struct EnergyChannel {
    pub name: String,
    pub watts: f32,
}

/// Potenza tra due letture (energia in picowattora, tempo in unità di 100 ns):
/// pWh × 3600 / 10¹² = J, diviso i secondi (× 10⁷).
pub fn emi_watts(before: (u64, u64), after: (u64, u64)) -> Option<f32> {
    let energy = after.0.checked_sub(before.0)?;
    let time = after.1.checked_sub(before.1)?;
    (time > 0).then(|| (energy as f64 * 0.036 / time as f64) as f32)
}

/// Il canale più completo: "PSYS" (tutta la piattaforma) se c'è, altrimenti
/// "PKG" (il processore, grafica integrata compresa), altrimenti il più alto.
pub fn pick_energy(channels: &[EnergyChannel]) -> Option<(f32, PowerSource)> {
    let valid: Vec<&EnergyChannel> = channels
        .iter()
        .filter(|c| c.watts.is_finite() && (0.0..2000.0).contains(&c.watts))
        .collect();
    let sum = |suffix: &str| {
        let matching: Vec<f32> = valid
            .iter()
            .filter(|c| c.name.to_uppercase().ends_with(suffix))
            .map(|c| c.watts)
            .collect();
        (!matching.is_empty()).then(|| matching.iter().sum::<f32>())
    };
    if let Some(w) = sum("_PSYS").filter(|&w| w > 0.0) {
        return Some((w, PowerSource::Platform));
    }
    if let Some(w) = sum("_PKG").filter(|&w| w > 0.0) {
        return Some((w, PowerSource::Processor));
    }
    valid
        .iter()
        .map(|c| c.watts)
        .filter(|&w| w > 0.0)
        .fold(None, |best: Option<f32>, w| {
            Some(best.map_or(w, |b| b.max(w)))
        })
        .map(|w| (w, PowerSource::Processor))
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn wide_at(data: &[u8], at: usize, bytes: usize) -> Option<String> {
    let units: Vec<u16> = data
        .get(at..at + bytes)?
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&c| c != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// EMI_METADATA_V1: unità (4 byte), produttore e modello (2 × 32 byte),
/// revisione (2), lunghezza del nome in byte (2), nome.
pub fn parse_emi_v1(data: &[u8]) -> Option<String> {
    let name_size = u16_at(data, 4 + 64 + 2)? as usize;
    wide_at(data, 4 + 64 + 4, name_size)
}

/// EMI_METADATA_V2: produttore e modello (2 × 32 byte), revisione (2), numero di
/// canali (2), poi i canali uno dopo l'altro senza allineamento: unità (4),
/// lunghezza del nome in byte (2), nome.
pub fn parse_emi_v2(data: &[u8]) -> Option<Vec<String>> {
    let count = u16_at(data, 64 + 2)? as usize;
    let mut at = 64 + 4;
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        let name_size = u16_at(data, at + 4)? as usize;
        names.push(wide_at(data, at + 6, name_size)?);
        at += 6 + name_size;
    }
    Some(names)
}

/// Temperatura più alta tra le zone termiche (in decimi di kelvin), scartando
/// i valori impossibili che alcune schede madri riportano.
pub fn thermal_zone_celsius(tenths_of_kelvin: &[f64]) -> Option<f32> {
    tenths_of_kelvin
        .iter()
        .map(|t| (t / 10.0 - 273.15) as f32)
        .filter(|c| (1.0..=125.0).contains(c))
        .fold(None, |best: Option<f32>, c| {
            Some(best.map_or(c, |b| b.max(c)))
        })
}

/// Un sensore di LibreHardwareMonitor.
#[derive(Clone, Debug, PartialEq)]
pub struct Sensor {
    pub identifier: String,
    pub name: String,
    pub kind: String,
    pub value: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SensorPick {
    pub cpu_temp: Option<f32>,
    pub gpu_temp: Option<f32>,
    pub cpu_power: Option<f32>,
    pub gpu_power: Option<f32>,
}

/// Sceglie i sensori più rappresentativi: "CPU Package" / "Tctl" per la CPU,
/// "GPU Core" e "GPU Package" per la scheda video.
pub fn pick_sensors(sensors: &[Sensor]) -> SensorPick {
    let is_cpu =
        |s: &&Sensor| s.identifier.starts_with("/intelcpu") || s.identifier.starts_with("/amdcpu");
    let is_gpu = |s: &&Sensor| s.identifier.starts_with("/gpu");
    fn of<'a>(sensors: &'a [Sensor], kind: &'a str) -> impl Iterator<Item = &'a Sensor> + 'a {
        sensors
            .iter()
            .filter(move |s| s.kind == kind && s.value.is_finite() && s.value > 0.0)
    }
    let prefer = |list: Vec<&Sensor>, names: &[&str]| -> Option<f32> {
        names
            .iter()
            .find_map(|n| {
                list.iter()
                    .find(|s| s.name.eq_ignore_ascii_case(n))
                    .map(|s| s.value)
            })
            .or_else(|| list.iter().map(|s| s.value).reduce(f32::max))
    };
    SensorPick {
        cpu_temp: prefer(
            of(sensors, "Temperature").filter(is_cpu).collect(),
            &[
                "CPU Package",
                "Core (Tctl/Tdie)",
                "Core (Tctl)",
                "Core (Tdie)",
                "Core Max",
            ],
        ),
        gpu_temp: prefer(
            of(sensors, "Temperature").filter(is_gpu).collect(),
            &["GPU Core", "GPU"],
        ),
        cpu_power: prefer(
            of(sensors, "Power").filter(is_cpu).collect(),
            &["CPU Package", "Package"],
        ),
        gpu_power: prefer(
            of(sensors, "Power").filter(is_gpu).collect(),
            &["GPU Package", "GPU Power", "GPU Board Power"],
        ),
    }
}

/// Ultimi valori di una misura, per i mini-grafici.
#[derive(Clone, Debug)]
pub struct History {
    values: VecDeque<f32>,
    capacity: usize,
}

impl History {
    pub fn new(capacity: usize) -> Self {
        Self {
            values: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, value: f32) {
        if self.values.len() == self.capacity {
            self.values.pop_front();
        }
        self.values.push_back(value);
    }

    pub fn values(&self) -> impl Iterator<Item = f32> + '_ {
        self.values.iter().copied()
    }

    pub fn max(&self) -> f32 {
        self.values.iter().copied().fold(0.0, f32::max)
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }
}

fn one_decimal(v: f64) -> String {
    format!("{v:.1}").replace('.', ",")
}

/// "9,8 GB", "512 MB".
pub fn fmt_bytes(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        if b >= 100.0 * GB {
            format!("{:.0} GB", b / GB)
        } else {
            format!("{} GB", one_decimal(b / GB))
        }
    } else {
        format!("{:.0} MB", b / (1024.0 * 1024.0))
    }
}

/// "0 KB/s", "350 KB/s", "1,2 MB/s", "118 MB/s".
pub fn fmt_rate(bytes_per_s: f64) -> String {
    let kb = bytes_per_s.max(0.0) / 1024.0;
    if kb < 1000.0 {
        format!("{kb:.0} KB/s")
    } else if kb < 100.0 * 1024.0 {
        format!("{} MB/s", one_decimal(kb / 1024.0))
    } else {
        format!("{:.0} MB/s", kb / 1024.0)
    }
}

pub fn fmt_percent(fraction: f32) -> String {
    format!("{:.0}%", (fraction * 100.0).clamp(0.0, 100.0))
}

pub fn fmt_temp(celsius: f32) -> String {
    format!("{celsius:.0} °C")
}

pub fn fmt_watts(watts: f32) -> String {
    if watts < 100.0 {
        format!("{} W", one_decimal(watts as f64))
    } else {
        format!("{watts:.0} W")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heat {
    Normal,
    Warm,
    Hot,
}

pub fn heat(celsius: f32) -> Heat {
    if celsius >= 85.0 {
        Heat::Hot
    } else if celsius >= 70.0 {
        Heat::Warm
    } else {
        Heat::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_like_task_manager() {
        let i = |pid: u32, luid: &str, eng: u32, kind: &str, v: f64| {
            (
                format!("pid_{pid}_luid_{luid}_phys_0_eng_{eng}_engtype_{kind}"),
                v,
            )
        };
        let instances = vec![
            i(1, "0x0_0xA", 0, "3D", 30.0),
            i(2, "0x0_0xA", 0, "3D", 25.0),
            i(1, "0x0_0xA", 5, "VideoDecode", 40.0),
            i(3, "0x0_0xB", 0, "3D", 50.0),
        ];
        assert_eq!(gpu_utilization(&instances), Some(0.55));
        assert_eq!(gpu_utilization(&[]), None);
        assert_eq!(gpu_utilization(&[i(1, "x", 0, "3D", 180.0)]), Some(1.0));
        // Due motori Copy distinti, ognuno al 60%: la GPU è al 60%, non al 100%.
        let copies = vec![
            i(1, "0x0_0xA", 3, "Copy", 60.0),
            i(2, "0x0_0xA", 4, "Copy", 60.0),
        ];
        assert!((gpu_utilization(&copies).unwrap() - 0.6).abs() < 1e-6);
    }

    fn wide(s: &str) -> Vec<u8> {
        s.encode_utf16()
            .chain([0])
            .flat_map(|c| c.to_le_bytes())
            .collect()
    }

    #[test]
    fn energy_meter_metadata() {
        let mut v2 = vec![0u8; 64];
        v2.extend_from_slice(&1u16.to_le_bytes());
        v2.extend_from_slice(&2u16.to_le_bytes());
        for name in ["RAPL_Package0_PKG", "RAPL_Package0_PP0"] {
            let name = wide(name);
            v2.extend_from_slice(&0i32.to_le_bytes());
            v2.extend_from_slice(&(name.len() as u16).to_le_bytes());
            v2.extend_from_slice(&name);
        }
        assert_eq!(
            parse_emi_v2(&v2).unwrap(),
            vec!["RAPL_Package0_PKG", "RAPL_Package0_PP0"]
        );
        assert_eq!(parse_emi_v2(&v2[..80]), None, "dati troncati");

        let mut v1 = vec![0u8; 4 + 64 + 2];
        let name = wide("CPU");
        v1.extend_from_slice(&(name.len() as u16).to_le_bytes());
        v1.extend_from_slice(&name);
        assert_eq!(parse_emi_v1(&v1).as_deref(), Some("CPU"));
    }

    #[test]
    fn energy_meter_power() {
        // 10 W per 1 s = 10 J = 2 777 777 778 pWh
        let w = emi_watts((1_000, 50_000_000), (1_000 + 2_777_777_778, 60_000_000)).unwrap();
        assert!((w - 10.0).abs() < 0.01);
        assert_eq!(emi_watts((5, 10), (4, 20)), None, "contatore azzerato");
        assert_eq!(emi_watts((5, 10), (6, 10)), None);

        let ch = |name: &str, watts: f32| EnergyChannel {
            name: name.into(),
            watts,
        };
        let rapl = [
            ch("RAPL_Package0_PKG", 8.0),
            ch("RAPL_Package0_PP0", 5.0),
            ch("RAPL_Package0_DRAM", 1.0),
        ];
        assert_eq!(pick_energy(&rapl), Some((8.0, PowerSource::Processor)));
        let mut with_psys = rapl.to_vec();
        with_psys.push(ch("RAPL_Package0_PSYS", 14.0));
        assert_eq!(pick_energy(&with_psys), Some((14.0, PowerSource::Platform)));
        assert_eq!(
            pick_energy(&[ch("Other", 3.0), ch("Other2", 4.0)]),
            Some((4.0, PowerSource::Processor))
        );
        assert_eq!(
            pick_energy(&[ch("RAPL_Package0_PKG", 1e9)]),
            None,
            "valori assurdi"
        );
        assert_eq!(pick_energy(&[]), None);
    }

    #[test]
    fn thermal_zones() {
        assert_eq!(thermal_zone_celsius(&[]), None);
        let c = thermal_zone_celsius(&[3231.5, 3331.5, 0.0]).unwrap();
        assert!((c - 60.0).abs() < 0.01);
    }

    #[test]
    fn picks_package_sensors() {
        let s = |id: &str, name: &str, kind: &str, value: f32| Sensor {
            identifier: id.into(),
            name: name.into(),
            kind: kind.into(),
            value,
        };
        let sensors = vec![
            s(
                "/amdcpu/0/temperature/2",
                "Core (Tctl/Tdie)",
                "Temperature",
                61.0,
            ),
            s(
                "/amdcpu/0/temperature/3",
                "CCD1 (Tdie)",
                "Temperature",
                70.0,
            ),
            s("/amdcpu/0/power/0", "Package", "Power", 45.5),
            s(
                "/gpu-nvidia/0/temperature/0",
                "GPU Core",
                "Temperature",
                52.0,
            ),
            s(
                "/gpu-nvidia/0/temperature/2",
                "GPU Hot Spot",
                "Temperature",
                65.0,
            ),
            s("/gpu-nvidia/0/power/0", "GPU Package", "Power", 120.0),
            s("/lpc/nct6798d/temperature/0", "CPU", "Temperature", 40.0),
        ];
        let p = pick_sensors(&sensors);
        assert_eq!(p.cpu_temp, Some(61.0));
        assert_eq!(p.gpu_temp, Some(52.0));
        assert_eq!(p.cpu_power, Some(45.5));
        assert_eq!(p.gpu_power, Some(120.0));

        let only_cores = vec![
            s("/intelcpu/0/temperature/1", "Core #1", "Temperature", 55.0),
            s("/intelcpu/0/temperature/2", "Core #2", "Temperature", 58.0),
        ];
        assert_eq!(pick_sensors(&only_cores).cpu_temp, Some(58.0));
        assert_eq!(pick_sensors(&[]), SensorPick::default());
    }

    #[test]
    fn history_keeps_the_last_values() {
        let mut h = History::new(3);
        for v in [1.0, 5.0, 2.0, 3.0] {
            h.push(v);
        }
        assert_eq!(h.values().collect::<Vec<_>>(), vec![5.0, 2.0, 3.0]);
        assert_eq!(h.max(), 5.0);
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt_bytes(16 * 1024 * 1024 * 1024), "16,0 GB");
        assert_eq!(fmt_bytes(10_522_669_875), "9,8 GB");
        assert_eq!(fmt_bytes(512 * 1024 * 1024), "512 MB");
        assert_eq!(fmt_rate(0.0), "0 KB/s");
        assert_eq!(fmt_rate(350.0 * 1024.0), "350 KB/s");
        assert_eq!(fmt_rate(1.2 * 1024.0 * 1024.0), "1,2 MB/s");
        assert_eq!(fmt_rate(118.0 * 1024.0 * 1024.0), "118 MB/s");
        assert_eq!(fmt_percent(0.234), "23%");
        assert_eq!(fmt_temp(54.4), "54 °C");
        assert_eq!(fmt_watts(12.44), "12,4 W");
        assert_eq!(fmt_watts(165.2), "165 W");
        assert_eq!(heat(50.0), Heat::Normal);
        assert_eq!(heat(75.0), Heat::Warm);
        assert_eq!(heat(90.0), Heat::Hot);
        let stats = Stats {
            ram_used: 4,
            ram_total: 16,
            ..Default::default()
        };
        assert_eq!(stats.ram(), Some(0.25));
    }
}
