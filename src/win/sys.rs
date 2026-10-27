//! Piccoli wrapper su API di sistema: tempo, formati locali, notifiche tra thread.

use windows::Win32::Foundation::{FILETIME, HWND, LPARAM, RECT, SYSTEMTIME, WPARAM};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
use windows::Win32::System::SystemServices::TIME_ZONE_ID_DAYLIGHT;
use windows::Win32::System::Time::{
    FileTimeToSystemTime, GetTimeZoneInformation, SystemTimeToFileTime,
    SystemTimeToTzSpecificLocalTime, TIME_ZONE_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MONITORINFOF_PRIMARY, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SystemParametersInfoW,
};
use windows::core::BOOL;

use crate::time::{Clock, now_ms};
use windows::Win32::Globalization::{
    ENUM_DATE_FORMATS_FLAGS, GetDateFormatEx, GetTimeFormatEx, TIME_NOSECONDS,
};
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::{PCWSTR, w};

/// Sveglia il thread dell'interfaccia da un altro thread.
///
/// `HWND` non è `Send`, quindi conserviamo il valore grezzo: la finestra
/// vive quanto il processo, perciò resta valido.
#[derive(Clone, Copy)]
pub struct Notifier {
    hwnd: usize,
    msg: u32,
}

impl Notifier {
    pub fn new(hwnd: HWND, msg: u32) -> Self {
        Self {
            hwnd: hwnd.0 as usize,
            msg,
        }
    }

    pub fn notify(&self) {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.hwnd as *mut _)),
                self.msg,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

/// Ora attuale in unità FILETIME (100 ns dal 1601), la scala delle API multimediali.
pub fn now_filetime() -> i64 {
    let ft = unsafe { GetSystemTimePreciseAsFileTime() };
    ((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64
}

/// Millisecondi che mancano al prossimo minuto (per aggiornare l'orologio).
pub fn ms_to_next_minute() -> u32 {
    let ms_in_minute = (now_filetime() / 10_000) % 60_000;
    (60_000 - ms_in_minute) as u32
}

/// Ora nel formato scelto in Windows (24 ore, oppure 12 ore con AM/PM).
pub fn clock_text() -> String {
    let mut buf = [0u16; 64];
    let n = unsafe {
        GetTimeFormatEx(
            PCWSTR::null(),
            TIME_NOSECONDS,
            None,
            PCWSTR::null(),
            Some(&mut buf),
        )
    };
    from_buf(&buf, n)
}

/// Data breve nella lingua di Windows, es. "lun 26 ott 2026".
pub fn short_date_text(with_year: bool) -> String {
    let mut buf = [0u16; 64];
    let format = if with_year {
        w!("ddd d MMM yyyy")
    } else {
        w!("ddd d MMM")
    };
    let n = unsafe {
        GetDateFormatEx(
            PCWSTR::null(),
            ENUM_DATE_FORMATS_FLAGS(0),
            None,
            format,
            Some(&mut buf),
            PCWSTR::null(),
        )
    };
    from_buf(&buf, n)
}

/// "Mostra animazioni in Windows" è spento: niente rimbalzi né scorrimenti.
pub fn reduce_motion() -> bool {
    let mut animate = BOOL(1);
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut animate as *mut BOOL as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_ok() && !animate.as_bool()
}

/// Le API restituiscono la lunghezza compreso il terminatore (0 se falliscono).
fn from_buf(buf: &[u16], written: i32) -> String {
    let len = (written.max(1) as usize - 1).min(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Orologio "da calendario" con il fuso orario attuale (ora legale compresa).
pub fn clock() -> Clock {
    let now = now_ms() as i64 / 1000;
    Clock::new(now, local_offset(now)).with_zone(local_offset)
}

/// Ora locale meno ora UTC nell'istante `t` (secondi Unix), con le regole
/// dell'ora legale del fuso orario di Windows.
fn local_offset(t: i64) -> i64 {
    // Secondi tra il 1° gennaio 1601 (inizio dei FILETIME) e il 1970.
    const FROM_1601: i64 = 11_644_473_600;
    let ticks = (t + FROM_1601).max(0) as u64 * 10_000_000;
    let utc_ft = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    let mut local_ft = FILETIME::default();
    let converted = unsafe {
        FileTimeToSystemTime(&utc_ft, &mut utc).is_ok()
            && SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_ok()
            && SystemTimeToFileTime(&local, &mut local_ft).is_ok()
    };
    if !converted {
        return current_offset();
    }
    let local_ticks = ((local_ft.dwHighDateTime as u64) << 32) | local_ft.dwLowDateTime as u64;
    (local_ticks as i64 - ticks as i64) / 10_000_000
}

fn current_offset() -> i64 {
    let mut tz = TIME_ZONE_INFORMATION::default();
    let mode = unsafe { GetTimeZoneInformation(&mut tz) };
    let bias = tz.Bias
        + if mode == TIME_ZONE_ID_DAYLIGHT {
            tz.DaylightBias
        } else {
            tz.StandardBias
        };
    -(bias as i64) * 60
}

/// Il suono di notifica scelto in Windows (rispetta "Non disturbare").
pub fn chime() {
    unsafe {
        let _ = PlaySoundW(
            w!("Notification.Default"),
            None,
            SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

pub struct Monitor {
    pub handle: HMONITOR,
    pub rect: RECT,
    pub work: RECT,
    pub primary: bool,
}

/// Monitor collegati, nell'ordine in cui li elenca Windows.
pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn collect(
        handle: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let list = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(handle, &mut info) }.as_bool() {
            list.push(Monitor {
                handle,
                rect: info.rcMonitor,
                work: info.rcWork,
                primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
        true.into()
    }
    let mut list: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut list as *mut Vec<Monitor> as isize),
        );
    }
    list
}

/// Millisecondi dall'ultimo input dell'utente (tastiera, mouse, touch).
pub fn idle_ms() -> u64 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe {
        if !GetLastInputInfo(&mut info).as_bool() {
            return 0;
        }
        // Entrambi i contatori ricominciano da zero ogni ~49 giorni: la sottrazione "a giro" è giusta.
        GetTickCount().wrapping_sub(info.dwTime) as u64
    }
}
