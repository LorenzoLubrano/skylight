//! Contatori di energia dei processori (Energy Meter Interface di Windows, ad
//! esempio i canali RAPL di Intel e AMD): danno il consumo del processore, o di
//! tutta la piattaforma, anche col caricatore collegato e senza driver esterni.
//!
//! Ogni dispositivo "energy meter" ha uno o più canali; ogni lettura dà l'energia
//! accumulata (picowattora) e l'istante (unità di 100 ns): la potenza è la
//! differenza tra due letture divisa per il tempo trascorso.

use std::mem::size_of;

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces,
    SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Power::{
    EMI_CHANNEL_MEASUREMENT_DATA, EMI_METADATA_SIZE, EMI_VERSION, EMI_VERSION_V1, EMI_VERSION_V2,
    GUID_DEVICE_ENERGY_METER, IOCTL_EMI_GET_MEASUREMENT, IOCTL_EMI_GET_METADATA,
    IOCTL_EMI_GET_METADATA_SIZE, IOCTL_EMI_GET_VERSION,
};
use windows::core::{HSTRING, PCWSTR};

use crate::features::sysinfo::{
    EnergyChannel, PowerSource, emi_watts, parse_emi_v1, parse_emi_v2, pick_energy,
};

/// Cosa misura un insieme di canali, e quanti watt.
pub struct Reading {
    pub watts: f32,
    pub source: PowerSource,
}

pub struct EnergyMeters {
    devices: Vec<Device>,
}

struct Device {
    handle: HANDLE,
    channels: Vec<String>,
    last: Option<Vec<EMI_CHANNEL_MEASUREMENT_DATA>>,
}

impl EnergyMeters {
    /// Apre tutti i contatori di energia presenti (nessuno su molti PC fissi).
    pub fn open() -> Self {
        let devices = device_paths()
            .into_iter()
            .filter_map(|path| Device::open(&path))
            .collect();
        Self { devices }
    }

    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    /// Potenza media dall'ultima lettura; `None` alla prima o se i canali non servono.
    pub fn read(&mut self) -> Option<Reading> {
        let mut channels = Vec::new();
        for device in &mut self.devices {
            let Some(now) = device.measure() else {
                continue;
            };
            if let Some(before) = device.last.replace(now.clone()) {
                for ((name, a), b) in device.channels.iter().zip(&before).zip(&now) {
                    if let Some(watts) = emi_watts(
                        (a.AbsoluteEnergy, a.AbsoluteTime),
                        (b.AbsoluteEnergy, b.AbsoluteTime),
                    ) {
                        channels.push(EnergyChannel {
                            name: name.clone(),
                            watts,
                        });
                    }
                }
            }
        }
        let (watts, source) = pick_energy(&channels)?;
        Some(Reading { watts, source })
    }
}

impl Drop for EnergyMeters {
    fn drop(&mut self) {
        for device in &self.devices {
            let _ = unsafe { CloseHandle(device.handle) };
        }
    }
}

impl Device {
    fn open(path: &HSTRING) -> Option<Self> {
        let handle = unsafe {
            CreateFileW(
                path,
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .ok()?;
        let device = Self {
            channels: channel_names(handle).unwrap_or_default(),
            handle,
            last: None,
        };
        if device.channels.is_empty() {
            let _ = unsafe { CloseHandle(handle) };
            return None;
        }
        Some(device)
    }

    fn measure(&self) -> Option<Vec<EMI_CHANNEL_MEASUREMENT_DATA>> {
        let mut data = vec![EMI_CHANNEL_MEASUREMENT_DATA::default(); self.channels.len()];
        let size = (data.len() * size_of::<EMI_CHANNEL_MEASUREMENT_DATA>()) as u32;
        let mut returned = 0u32;
        unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_EMI_GET_MEASUREMENT,
                None,
                0,
                Some(data.as_mut_ptr().cast()),
                size,
                Some(&mut returned),
                None,
            )
        }
        .ok()?;
        (returned >= size).then_some(data)
    }
}

/// Percorsi dei dispositivi con l'interfaccia "energy meter".
fn device_paths() -> Vec<HSTRING> {
    let mut paths = Vec::new();
    unsafe {
        let Ok(set) = SetupDiGetClassDevsW(
            Some(&GUID_DEVICE_ENERGY_METER),
            PCWSTR::null(),
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        ) else {
            return paths;
        };
        for index in 0.. {
            let mut interface = SP_DEVICE_INTERFACE_DATA {
                cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInterfaces(
                set,
                None,
                &GUID_DEVICE_ENERGY_METER,
                index,
                &mut interface,
            )
            .is_err()
            {
                break;
            }
            let mut needed = 0u32;
            let _ =
                SetupDiGetDeviceInterfaceDetailW(set, &interface, None, 0, Some(&mut needed), None);
            if (needed as usize) < size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() {
                continue;
            }
            // Buffer allineato: la struttura ha una lunghezza variabile.
            let mut buffer = vec![0u32; (needed as usize).div_ceil(4)];
            let detail = buffer.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &interface, Some(detail), needed, None, None)
                .is_ok()
            {
                let start = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
                let max = (needed as usize - 4) / 2;
                let wide = std::slice::from_raw_parts(start, max);
                let len = wide.iter().position(|&c| c == 0).unwrap_or(max);
                paths.push(HSTRING::from_wide(&wide[..len]));
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    paths
}

fn ioctl<T: Default>(handle: HANDLE, code: u32) -> Option<T> {
    let mut value = T::default();
    let mut returned = 0u32;
    unsafe {
        DeviceIoControl(
            handle,
            code,
            None,
            0,
            Some(&mut value as *mut T as *mut _),
            size_of::<T>() as u32,
            Some(&mut returned),
            None,
        )
    }
    .ok()?;
    (returned as usize >= size_of::<T>()).then_some(value)
}

/// Nomi dei canali del dispositivo, es. "RAPL_Package0_PKG".
fn channel_names(handle: HANDLE) -> Option<Vec<String>> {
    let version: EMI_VERSION = ioctl(handle, IOCTL_EMI_GET_VERSION)?;
    let size: EMI_METADATA_SIZE = ioctl(handle, IOCTL_EMI_GET_METADATA_SIZE)?;
    let mut metadata = vec![0u8; size.MetadataSize as usize];
    let mut returned = 0u32;
    unsafe {
        DeviceIoControl(
            handle,
            IOCTL_EMI_GET_METADATA,
            None,
            0,
            Some(metadata.as_mut_ptr().cast()),
            metadata.len() as u32,
            Some(&mut returned),
            None,
        )
    }
    .ok()?;
    let metadata = &metadata[..(returned as usize).min(metadata.len())];
    match version.EmiVersion as u32 {
        EMI_VERSION_V1 => parse_emi_v1(metadata).map(|name| vec![name]),
        EMI_VERSION_V2 => parse_emi_v2(metadata),
        _ => None,
    }
}
