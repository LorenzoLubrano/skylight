//! Cambio di nome sul PC vero: la cartella in `%APPDATA%` e il registro
//! dell'utente. La logica (cosa copiare, quando, cosa fare se fallisce) è in
//! `crate::migrate`; qui ci sono solo le chiamate a Windows.

use std::ffi::c_void;

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;
use windows::core::{HSTRING, PCWSTR};

use super::settings_store;
use crate::migrate::{self, APP_NAME, OLD_NAME, Registry};
use crate::store;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Dove Gestione attività ricorda le voci di avvio disattivate.
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// Porta dati, impostazioni e avvio con Windows dal nome vecchio. Va chiamata
/// prima di leggere dati e impostazioni; non fallisce mai: al peggio si parte da zero.
pub fn run() {
    if let Some(base) = store::base_dir() {
        let _ = migrate::copy_data(&base.join(OLD_NAME), &base.join(APP_NAME));
    }
    let _ = migrate::migrate_registry(&mut WinRegistry, &settings_store::exe_command());
}

struct WinRegistry;

fn settings_key(app: &str) -> HSTRING {
    HSTRING::from(format!(r"Software\{app}"))
}

/// Chiave aperta, chiusa da sola.
struct Key(HKEY);

impl Key {
    fn open(path: &HSTRING) -> Option<Self> {
        let mut key = HKEY::default();
        let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path, Some(0), KEY_READ, &mut key) };
        (status == ERROR_SUCCESS).then_some(Self(key))
    }

    fn create(path: &HSTRING) -> Option<Self> {
        let mut key = HKEY::default();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path,
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                None,
                &mut key,
                None,
            )
        };
        (status == ERROR_SUCCESS).then_some(Self(key))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

impl Registry for WinRegistry {
    fn has_settings(&self, app: &str) -> bool {
        Key::open(&settings_key(app)).is_some()
    }

    fn copy_settings(&mut self, from: &str, to: &str) -> bool {
        let (Some(source), Some(target)) = (
            Key::open(&settings_key(from)),
            Key::create(&settings_key(to)),
        ) else {
            return false;
        };
        unsafe { RegCopyTreeW(source.0, PCWSTR::null(), target.0) == ERROR_SUCCESS }
    }

    fn delete_settings(&mut self, app: &str) {
        let key = settings_key(app);
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &key);
            let _ = RegDeleteKeyW(HKEY_CURRENT_USER, &key);
        }
    }

    fn has_autostart(&self, app: &str) -> bool {
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(RUN_KEY),
                &HSTRING::from(app),
                RRF_RT_ANY,
                None,
                None,
                None,
            )
        };
        status == ERROR_SUCCESS
    }

    fn autostart_disabled(&self, app: &str) -> bool {
        // 12 byte: il primo è dispari se la voce è disattivata, poi la data.
        let mut data = [0u8; 64];
        let mut size = data.len() as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(APPROVED_KEY),
                &HSTRING::from(app),
                RRF_RT_REG_BINARY,
                None,
                Some(data.as_mut_ptr() as *mut c_void),
                Some(&mut size),
            )
        };
        status == ERROR_SUCCESS && size > 0 && data[0] & 1 == 1
    }

    fn set_autostart(&mut self, app: &str, command: &str) -> bool {
        settings_store::write_run_value(&HSTRING::from(app), command).is_ok()
    }

    fn delete_autostart(&mut self, app: &str) {
        let name = HSTRING::from(app);
        unsafe {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(RUN_KEY), &name);
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(APPROVED_KEY), &name);
        }
    }
}
