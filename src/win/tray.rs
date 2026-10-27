//! Icona nell'area di notifica e relativo menu.

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

pub const ID_SETTINGS: u32 = 9;
pub const ID_LYRICS: u32 = 10;
pub const ID_AUTOSTART: u32 = 12;
pub const ID_EXIT: u32 = 13;

/// Icone PNG già pronte per ogni dimensione comune della tray (100%-400%).
const ICONS: &[(i32, &[u8])] = &[
    (16, include_bytes!("../../assets/tray-16.png")),
    (20, include_bytes!("../../assets/tray-20.png")),
    (24, include_bytes!("../../assets/tray-24.png")),
    (32, include_bytes!("../../assets/tray-32.png")),
    (40, include_bytes!("../../assets/tray-40.png")),
    (48, include_bytes!("../../assets/tray-48.png")),
    (64, include_bytes!("../../assets/tray-64.png")),
];

/// Stato delle voci con la spunta.
pub struct MenuState {
    pub lyrics: bool,
    pub autostart: bool,
}

pub struct Tray {
    hwnd: HWND,
    callback: u32,
    icon: HICON,
}

impl Tray {
    pub fn new(hwnd: HWND, callback: u32) -> Self {
        let tray = Self {
            hwnd,
            callback,
            icon: load_icon(),
        };
        tray.add();
        tray
    }

    /// Da richiamare anche quando Explorer si riavvia (messaggio "TaskbarCreated").
    pub fn add(&self) {
        let mut data = self.data();
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = self.callback;
        data.hIcon = self.icon;
        for (dst, src) in data.szTip.iter_mut().zip("Skylight".encode_utf16()) {
            *dst = src;
        }
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &data);
        }
    }

    fn data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            ..Default::default()
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data());
            let _ = DestroyIcon(self.icon);
        }
    }
}

fn load_icon() -> HICON {
    unsafe {
        let size = GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem());
        let (_, png) = ICONS
            .iter()
            .find(|(s, _)| *s >= size)
            .unwrap_or(&ICONS[ICONS.len() - 1]);
        // CreateIconFromResourceEx accetta direttamente dati PNG (Windows Vista+).
        CreateIconFromResourceEx(png, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR)
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default()
    }
}

/// Mostra il menu sotto il cursore e restituisce la voce scelta (0 se nessuna).
pub fn show_menu(hwnd: HWND, state: MenuState) -> u32 {
    let check = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return 0;
        };
        let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS as usize, w!("Impostazioni…"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            menu,
            MF_STRING | check(state.lyrics),
            ID_LYRICS as usize,
            w!("Testi sincronizzati"),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING | check(state.autostart),
            ID_AUTOSTART as usize,
            w!("Avvia con Windows"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_EXIT as usize, w!("Esci"));

        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        // Senza questo il menu non si chiude cliccando altrove (comportamento documentato).
        let previous = GetForegroundWindow();
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            cursor.x,
            cursor.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        // Il notch non deve tenersi il focus della tastiera dopo il menu.
        if !previous.is_invalid() && previous != hwnd && GetForegroundWindow() == hwnd {
            let _ = SetForegroundWindow(previous);
        }
        chosen.0 as u32
    }
}
