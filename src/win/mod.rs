//! Tutto ciò che parla con Windows: finestre, grafica, tray, impostazioni,
//! osservatori di sistema e trascinamento.

mod app;
mod dnd;
mod energy;
mod gfx;
mod keyboard;
mod migration;
mod monitors;
mod power;
mod render;
mod settings_store;
mod settings_window;
mod sys;
mod sysmon;
mod thumbs;
mod tray;
mod volume;
mod wmi;

use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::{HSTRING, w};

pub use app::run;
pub use sys::{Notifier, now_filetime};

/// Mostra un errore che impedisce l'avvio.
pub fn fatal(error: &windows::core::Error) {
    let text = HSTRING::from(format!("Skylight non riesce ad avviarsi:\n{error}"));
    unsafe {
        MessageBoxW(None, &text, w!("Skylight"), MB_ICONERROR | MB_OK);
    }
}
