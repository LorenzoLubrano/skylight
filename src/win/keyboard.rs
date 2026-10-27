//! Bloc Maiusc e lingua della tastiera.
//!
//! Con il "raw input" Windows avvisa la finestra a ogni tasto premuto, anche in
//! altre app. Qui si guarda solo *quale* tasto è (Bloc Maiusc, Shift, Alt, Ctrl,
//! Win, Spazio): nessun carattere viene letto, salvato o inviato. Dopo uno di
//! questi tasti, con un piccolo ritardo, si controllano stato del Bloc Maiusc e
//! layout della finestra in primo piano.

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Globalization::{
    GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SLOCALIZEDDISPLAYNAME,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, GetKeyboardLayout, VK_CAPITAL, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    VK_SPACE,
};
use windows::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT,
    RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEKEYBOARD, RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
    RI_KEY_BREAK,
};
use windows::core::{PCWSTR, w};

pub enum KeyEvent {
    CapsLock(bool),
    Language(String),
}

#[derive(Default)]
pub struct Keyboard {
    registered: bool,
    caps: bool,
    caps_down: bool,
    /// Pressioni di Bloc Maiusc dall'ultimo controllo.
    caps_presses: u32,
    foreground: usize,
    layout: usize,
}

impl Keyboard {
    pub fn new() -> Self {
        let mut k = Self {
            caps: caps_on(),
            ..Self::default()
        };
        k.rebase();
        k
    }

    pub fn set_enabled(&mut self, hwnd: HWND, enabled: bool) {
        if enabled == self.registered {
            return;
        }
        let device = RAWINPUTDEVICE {
            usUsagePage: 0x01, // Generic Desktop
            usUsage: 0x06,     // Keyboard
            dwFlags: if enabled {
                RIDEV_INPUTSINK
            } else {
                RIDEV_REMOVE
            },
            hwndTarget: if enabled { hwnd } else { HWND::default() },
        };
        let ok = unsafe { RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32) }
            .is_ok();
        if ok {
            self.registered = enabled;
        }
    }

    /// `WM_INPUT`: restituisce dopo quanti millisecondi controllare, se serve.
    pub fn on_raw_input(&mut self, lp: LPARAM) -> Option<u32> {
        let mut raw = RAWINPUT::default();
        let mut size = size_of::<RAWINPUT>() as u32;
        let read = unsafe {
            GetRawInputData(
                HRAWINPUT(lp.0 as *mut _),
                RID_INPUT,
                Some(&mut raw as *mut RAWINPUT as *mut _),
                &mut size,
                size_of::<RAWINPUTHEADER>() as u32,
            )
        };
        if read == u32::MAX || raw.header.dwType != RIM_TYPEKEYBOARD.0 {
            return None;
        }
        let key = unsafe { raw.data.keyboard };
        let released = key.Flags as u32 & RI_KEY_BREAK != 0;
        match key.VKey {
            k if k == VK_CAPITAL.0 => {
                // Le ripetizioni del tasto tenuto premuto non contano.
                if !released && !self.caps_down {
                    self.caps_presses += 1;
                }
                self.caps_down = !released;
                (!released).then_some(80)
            }
            k if released
                && [
                    VK_SHIFT.0,
                    VK_CONTROL.0,
                    VK_MENU.0,
                    VK_LWIN.0,
                    VK_RWIN.0,
                    VK_SPACE.0,
                ]
                .contains(&k) =>
            {
                Some(300)
            }
            _ => None,
        }
    }

    pub fn check(&mut self) -> Vec<KeyEvent> {
        let mut events = Vec::new();
        if self.caps_presses > 0 {
            // Se Windows ci dà uno stato aggiornato si usa quello, altrimenti si
            // deduce dal numero di pressioni (pari = invariato).
            let reported = caps_on();
            let deduced = self.caps ^ (self.caps_presses % 2 == 1);
            let now = if reported != self.caps {
                reported
            } else {
                deduced
            };
            self.caps_presses = 0;
            if now != self.caps {
                self.caps = now;
                events.push(KeyEvent::CapsLock(now));
            }
        }
        let (foreground, layout) = foreground_layout();
        if foreground == self.foreground && layout != self.layout && layout != 0 {
            events.push(KeyEvent::Language(language_name(layout)));
        }
        self.foreground = foreground;
        self.layout = layout;
        events
    }

    /// Cambio di finestra: ogni app ha il suo layout, non è un cambio di lingua.
    pub fn rebase(&mut self) {
        (self.foreground, self.layout) = foreground_layout();
    }
}

fn caps_on() -> bool {
    unsafe { GetKeyState(VK_CAPITAL.0 as i32) & 1 != 0 }
}

fn foreground_layout() -> (usize, usize) {
    unsafe {
        let window = GetForegroundWindow();
        let thread = focus_thread(window);
        (window.0 as usize, GetKeyboardLayout(thread).0 as usize)
    }
}

/// Il thread che riceve davvero la tastiera: quello della finestra col focus.
/// Nelle app UWP (Impostazioni, Calcolatrice…) la finestra in primo piano è solo
/// la cornice di ApplicationFrameHost e la lingua cambia nella CoreWindow figlia.
unsafe fn focus_thread(window: HWND) -> u32 {
    unsafe {
        let thread = GetWindowThreadProcessId(window, None);
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(thread, &mut info).is_ok() && !info.hwndFocus.is_invalid() {
            return GetWindowThreadProcessId(info.hwndFocus, None);
        }
        match FindWindowExW(
            Some(window),
            None,
            w!("Windows.UI.Core.CoreWindow"),
            PCWSTR::null(),
        ) {
            Ok(core) if !core.is_invalid() => GetWindowThreadProcessId(core, None),
            _ => thread,
        }
    }
}

/// Nome della lingua nella lingua di Windows, es. "Inglese (Stati Uniti)".
fn language_name(layout: usize) -> String {
    let lang_id = (layout & 0xFFFF) as u32;
    let mut locale = [0u16; 85];
    let len = unsafe { LCIDToLocaleName(lang_id, Some(&mut locale), 0) };
    if len <= 0 {
        return format!("Layout {lang_id:04X}");
    }
    let mut name = [0u16; 128];
    let len = unsafe {
        GetLocaleInfoEx(
            PCWSTR(locale.as_ptr()),
            LOCALE_SLOCALIZEDDISPLAYNAME,
            Some(&mut name),
        )
    };
    if len <= 1 {
        return String::from_utf16_lossy(
            &locale[..locale.iter().position(|&c| c == 0).unwrap_or(0)],
        );
    }
    let text = String::from_utf16_lossy(&name[..len as usize - 1]);
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or(text)
}
