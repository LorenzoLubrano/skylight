//! Impostazioni persistenti nel registro dell'utente (HKCU\Software\Skylight):
//! niente file da gestire e nessun permesso di amministratore richiesto.

use std::ffi::c_void;

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::*;
use windows::core::{HSTRING, PCWSTR, Param, Result, w};

use crate::settings::{FULLSCREEN_NORMAL, Field, Settings};

const KEY: PCWSTR = w!("Software\\Skylight");
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Skylight");

pub fn load() -> Settings {
    let mut s = Settings::default();
    // Due passate: alcuni valori dipendono da altri (es. la fascia oraria dell'acqua).
    for _ in 0..2 {
        for field in Field::ALL.into_iter().filter(|&f| f != Field::Autostart) {
            if let Some(value) = get_u32(field.key()) {
                field.set(&mut s, value);
            }
        }
    }
    // Fino alla 0.2 c'era solo "Nascondi a schermo intero": chi l'aveva spento
    // voleva il notch sempre visibile, quindi "Resta com'è".
    if get_u32(Field::Fullscreen.key()).is_none() && get_u32("HideInFullscreen") == Some(0) {
        s.fullscreen_mode = FULLSCREEN_NORMAL;
    }
    s.autostart = autostart_enabled();
    s
}

pub fn save(s: &Settings, field: Field) {
    if field == Field::Autostart {
        let _ = set_autostart(s.autostart);
    } else {
        set_u32(field.key(), field.get(s));
    }
}

fn get_u32(name: &str) -> Option<u32> {
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEY,
            &HSTRING::from(name),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(data)
}

fn set_u32(name: &str, value: u32) {
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            KEY,
            &HSTRING::from(name),
            REG_DWORD.0,
            Some(&value as *const u32 as *const c_void),
            size_of::<u32>() as u32,
        );
    }
}

fn autostart_enabled() -> bool {
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        )
    };
    status == ERROR_SUCCESS
}

/// "Avvia con Windows": una voce in `HKCU\...\Run` che punta a questo eseguibile.
fn set_autostart(enabled: bool) -> Result<()> {
    if !enabled {
        return unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE).ok() };
    }
    write_run_value(RUN_VALUE, &exe_command())
}

/// Questo eseguibile tra virgolette, come va scritto nella voce di avvio.
pub fn exe_command() -> String {
    let mut path = vec![0u16; 32_768];
    let len = unsafe { GetModuleFileNameW(None, &mut path) } as usize;
    format!("\"{}\"", String::from_utf16_lossy(&path[..len]))
}

/// Scrive la voce di avvio `name` con la riga di comando data.
pub fn write_run_value<P: Param<PCWSTR>>(name: P, command: &str) -> Result<()> {
    let command: Vec<u16> = command.encode_utf16().chain([0]).collect();
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            name,
            REG_SZ.0,
            Some(command.as_ptr() as *const c_void),
            (command.len() * size_of::<u16>()) as u32,
        )
        .ok()
    }
}
