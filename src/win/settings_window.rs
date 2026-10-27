//! Finestra Impostazioni: una normale finestra di Windows (con barra del titolo
//! scura) il cui contenuto è disegnato con Direct2D nello stesso stile del notch.
//! Ogni modifica si applica subito, senza pulsante "Salva".

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint, ValidateRect,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, Error, PCWSTR, Result, w};

use super::gfx::Gfx;
use super::render;
use crate::layout::settings::{
    SETTINGS_H, SETTINGS_W, SettingsHit, SettingsLayout, settings_hit, settings_layout,
};
use crate::settings::{Field, Settings};

const CLASS_NAME: PCWSTR = w!("Skylight.Settings");
const WM_MOUSELEAVE: u32 = 0x02A3;
const STYLE: WINDOW_STYLE =
    WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0);

pub fn register(instance: HINSTANCE, proc: WNDPROC) -> Result<()> {
    // L'icona incorporata nell'exe (risorsa 1): barra del titolo, barra delle applicazioni, Alt+Tab.
    let icon = |metric_x, metric_y| unsafe {
        LoadImageW(
            Some(instance),
            // MAKEINTRESOURCE(1)
            PCWSTR(std::ptr::without_provenance(1)),
            IMAGE_ICON,
            GetSystemMetrics(metric_x),
            GetSystemMetrics(metric_y),
            LR_DEFAULTCOLOR,
        )
        .map(|h| HICON(h.0))
        .unwrap_or_default()
    };
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: proc,
        hInstance: instance,
        hIcon: icon(SM_CXICON, SM_CYICON),
        hIconSm: icon(SM_CXSMICON, SM_CYSMICON),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        return Err(Error::from_thread());
    }
    Ok(())
}

/// Risposta a un messaggio: risultato per Windows, eventuale impostazione da
/// cambiare (campo e direzione) e se la finestra si è chiusa.
#[derive(Default)]
pub struct Reply {
    pub result: Option<LRESULT>,
    pub change: Option<(Field, i32)>,
    pub closed: bool,
}

pub struct SettingsWindow {
    hwnd: HWND,
    gfx: Option<Gfx>,
    layout: SettingsLayout,
    scroll: f32,
    hot: Option<SettingsHit>,
    /// Ultima posizione del mouse nella finestra, per aggiornare l'evidenziazione
    /// quando il contenuto scorre sotto un cursore fermo.
    mouse: Option<LPARAM>,
    tracking: bool,
    dirty: bool,
    failures: u8,
}

impl SettingsWindow {
    pub fn create() -> Result<Self> {
        unsafe {
            let module = GetModuleHandleW(None)?;
            // Posizione e misura calcolate prima, con la scala del monitor di
            // destinazione: la finestra nasce già giusta, senza spostarsi dopo.
            let (x, y, w, h) = initial_placement();
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP,
                CLASS_NAME,
                w!("Impostazioni di Skylight"),
                STYLE,
                x,
                y,
                w,
                h,
                None,
                None,
                Some(module.into()),
                None,
            )?;
            let dark = BOOL(1);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const BOOL as *const _,
                size_of::<BOOL>() as u32,
            );
            let window = Self {
                hwnd,
                gfx: None,
                layout: settings_layout(),
                scroll: 0.0,
                hot: None,
                mouse: None,
                tracking: false,
                dirty: true,
                failures: 0,
            };
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            Ok(window)
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn focus(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(self.hwnd);
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    fn dpi(&self) -> u32 {
        unsafe { GetDpiForWindow(self.hwnd) }.max(96)
    }

    fn scale(&self) -> f32 {
        self.dpi() as f32 / 96.0
    }

    fn client_size(&self) -> (u32, u32) {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rect);
        }
        (
            (rect.right - rect.left).max(1) as u32,
            (rect.bottom - rect.top).max(1) as u32,
        )
    }

    fn view_height(&self) -> f32 {
        self.client_size().1 as f32 / self.scale()
    }

    fn max_scroll(&self) -> f32 {
        (self.layout.content_h - self.view_height()).max(0.0)
    }

    fn point(&self, lp: LPARAM) -> (f32, f32) {
        let x = (lp.0 & 0xFFFF) as u16 as i16 as f32;
        let y = ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as f32;
        let k = self.scale();
        (x / k, y / k + self.scroll)
    }

    pub fn handle(&mut self, msg: u32, wp: WPARAM, lp: LPARAM, _settings: &Settings) -> Reply {
        let mut reply = Reply {
            result: Some(LRESULT(0)),
            ..Default::default()
        };
        match msg {
            WM_MOUSEMOVE => {
                self.mouse = Some(lp);
                if !self.tracking {
                    self.tracking = true;
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: self.hwnd,
                        dwHoverTime: 0,
                    };
                    unsafe {
                        let _ = TrackMouseEvent(&mut track);
                    }
                }
                let (x, y) = self.point(lp);
                let hot = settings_hit(&self.layout, x, y);
                if hot != self.hot {
                    self.hot = hot;
                    self.dirty = true;
                }
            }
            WM_MOUSELEAVE => {
                self.tracking = false;
                self.mouse = None;
                if self.hot.take().is_some() {
                    self.dirty = true;
                }
            }
            WM_LBUTTONUP => {
                let (x, y) = self.point(lp);
                reply.change = settings_hit(&self.layout, x, y).map(|hit| match hit {
                    SettingsHit::Toggle(field) | SettingsHit::Plus(field) => (field, 1),
                    SettingsHit::Minus(field) => (field, -1),
                });
            }
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as u16 as i16 as f32;
                self.scroll = (self.scroll - delta / 120.0 * 56.0).clamp(0.0, self.max_scroll());
                // Il contenuto si è mosso sotto il cursore: cambia la riga evidenziata.
                if let Some(lp) = self.mouse {
                    let (x, y) = self.point(lp);
                    self.hot = settings_hit(&self.layout, x, y);
                }
                self.dirty = true;
            }
            WM_SETCURSOR if (lp.0 & 0xFFFF) as u32 == HTCLIENT => {
                let cursor = if self.hot.is_some() {
                    IDC_HAND
                } else {
                    IDC_ARROW
                };
                unsafe { SetCursor(LoadCursorW(None, cursor).ok()) };
                reply.result = Some(LRESULT(1));
            }
            WM_SIZE => {
                let (w, h) = self.client_size();
                let dpi = self.dpi() as f32;
                if let Some(gfx) = &mut self.gfx
                    && gfx.resize(w, h, dpi).is_err()
                {
                    self.gfx = None;
                }
                self.scroll = self.scroll.min(self.max_scroll());
                self.dirty = true;
            }
            WM_DPICHANGED => {
                // Si usa il rettangolo proposto da Windows così com'è: è quello che
                // tiene la finestra sotto il cursore mentre la si trascina tra due monitor.
                let r = unsafe { *(lp.0 as *const RECT) };
                unsafe {
                    let _ = SetWindowPos(
                        self.hwnd,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                // Il WM_SIZE di SetWindowPos arriva mentre l'app è occupata: si ridimensiona qui.
                let (w, h) = self.client_size();
                let dpi = self.dpi() as f32;
                if let Some(gfx) = &mut self.gfx
                    && gfx.resize(w, h, dpi).is_err()
                {
                    self.gfx = None;
                }
                self.scroll = self.scroll.min(self.max_scroll());
                self.dirty = true;
            }
            WM_PAINT => {
                unsafe {
                    let _ = ValidateRect(Some(self.hwnd), None);
                }
                self.dirty = true;
            }
            WM_ERASEBKGND => reply.result = Some(LRESULT(1)),
            WM_CLOSE => {
                self.gfx = None;
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
                reply.closed = true;
            }
            _ => reply.result = None,
        }
        reply
    }

    pub fn render(&mut self, settings: &Settings, monitors: u32) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let (w, h) = self.client_size();
        if self.gfx.is_none() {
            self.gfx = Gfx::new(self.hwnd, w, h, self.dpi() as f32).ok();
        }
        let view_h = h as f32 / self.scale();
        let Some(gfx) = &mut self.gfx else { return };
        let view = render::settings::View {
            layout: &self.layout,
            scroll: self.scroll,
            hot: self.hot,
            settings,
            monitors,
            view_h,
        };
        if render::settings::draw(gfx, &view).is_ok() {
            self.failures = 0;
        } else if self.failures < 3 {
            // Scheda video resettata: si ricrea tutto al prossimo frame.
            self.failures += 1;
            self.gfx = None;
            self.dirty = true;
        } else {
            self.gfx = None;
        }
    }
}

/// Finestra centrata nell'area di lavoro del monitor principale, grande quanto
/// serve alla sua scala. Su schermi piccoli con scala alta si accorcia (il
/// contenuto si scorre con la rotella) e la barra del titolo resta raggiungibile.
fn initial_placement() -> (i32, i32, i32, i32) {
    unsafe {
        let monitor = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
        let (mut dpi, mut dpi_y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y);
        let dpi = dpi.max(96);
        let k = dpi as f32 / 96.0;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: (SETTINGS_W * k) as i32,
            bottom: (SETTINGS_H * k) as i32,
        };
        let _ = AdjustWindowRectExForDpi(&mut rect, STYLE, false, WS_EX_NOREDIRECTIONBITMAP, dpi);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let work = info.rcWork;
        let w = (rect.right - rect.left).min(work.right - work.left);
        let h = (rect.bottom - rect.top).min(work.bottom - work.top);
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.top + (work.bottom - work.top - h) / 2;
        (x, y, w, h)
    }
}
