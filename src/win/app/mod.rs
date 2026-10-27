//! Finestra del notch: ciclo dei messaggi, stato dell'interfaccia, animazioni e
//! posizione sullo schermo. L'input è in `input.rs`, i dati in `data.rs`.
//!
//! Regola d'oro per la leggerezza: si disegna solo quando qualcosa cambia.
//! Il ciclo dei messaggi dorme in `GetMessageW` (0% CPU) e passa a un ciclo
//! sincronizzato con il vsync solo finché c'è un'animazione in corso.

mod data;
mod input;
mod preview_icons;
mod snapshot;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::{IDropTarget, OleInitialize, RegisterDragDrop, RevokeDragDrop};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Shell::{
    ABM_NEW, ABM_REMOVE, APPBARDATA, QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN, SHAppBarMessage,
    SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Error, PCWSTR, Result, w};

use super::dnd::{self, DropTarget, Payload};
use super::gfx::Gfx;
use super::keyboard::Keyboard;
use super::monitors::{self, Events};
use super::power::{self, Battery, Power};
use super::settings_window::{self, SettingsWindow};
use super::sysmon::{self, Control};
use super::thumbs::{self, THUMB_KEY, Thumbnails};
use super::{render, settings_store, sys, tray};
use crate::anim::Spring;
use crate::features::alerts::{AlertKind, Alerts};
use crate::features::pomodoro::Pomodoro;
use crate::features::shelf::Shelf;
use crate::features::sysinfo::{History, Stats};
use crate::features::todo::TodoList;
use crate::features::water::WaterLog;
use crate::layout::*;
use crate::lyrics::fetch::{self, Replies};
use crate::lyrics::{LyricsState, lrclib::Query};
use crate::media::gsmtc::{self, MediaControl};
use crate::media::{Artwork, MediaSnapshot};
use crate::settings::{FULLSCREEN_HIDE, FULLSCREEN_NORMAL, FULLSCREEN_SHRINK, Settings};
use crate::textedit::TextEdit;
use crate::util::Inbox;
use crate::{store, time};

const CLASS_NAME: PCWSTR = w!("Skylight.Notch");
/// Un thread di lavoro ha dati nuovi.
const WM_APP_WAKE: u32 = WM_APP + 1;
const WM_APP_TRAY: u32 = WM_APP + 2;
/// Notifiche della shell, tra cui "un'app è andata a schermo intero".
const WM_APP_APPBAR: u32 = WM_APP + 3;
/// Definito in `Win32::UI::Controls`, una feature molto pesante da compilare.
const WM_MOUSELEAVE: u32 = 0x02A3;

const TIMER_COLLAPSE: usize = 1;
const TIMER_TICK: usize = 2;
const TIMER_LYRICS: usize = 3;
const TIMER_KEYS: usize = 4;
/// Durante i cicli modali (menu, trascinamento, spostamento finestra) il nostro
/// ciclo dei messaggi è fermo: i frame arrivano da questo timer.
const TIMER_FRAME: usize = 5;
/// Controllo leggero dello schermo intero: una finestra già in primo piano può
/// passare a schermo intero (F11, il tasto "f" di YouTube) senza nessun evento.
const TIMER_FULLSCREEN: usize = 6;
/// Fa girare l'arco del download, circa 30 volte al secondo, solo mentre si vede.
const TIMER_SPIN: usize = 7;
/// Durata di un giro dell'arco del download, in secondi.
const SPIN_PERIOD_S: f32 = 1.2;

/// Molle in stile Apple, come (durata di un'oscillazione in s, smorzamento):
/// apertura morbida con un piccolo rimbalzo, chiusura rapida e ferma, avvisi
/// un po' più vivaci. Con "Mostra animazioni" spento in Windows nessuna rimbalza.
const OPEN_SPRING: (f32, f32) = (0.46, 0.78);
const CLOSE_SPRING: (f32, f32) = (0.34, 1.0);
const ALERT_SPRING: (f32, f32) = (0.42, 0.7);
const OPEN_FADE: (f32, f32) = (0.42, 1.0);
const CLOSE_FADE: (f32, f32) = (0.26, 1.0);
const PAGE_SPRING: (f32, f32) = (0.38, 0.92);
const REVEAL_SPRING: (f32, f32) = (0.24, 1.0);

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Accesso all'app dal thread dell'interfaccia. Restituisce `None` se l'app è
/// già in uso più in alto nello stack (Windows a volte invia messaggi in modo
/// sincrono mentre ne stiamo gestendo un altro): in quel caso il messaggio
/// passa alla gestione predefinita di Windows.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

// ---- Punti di ingresso per il trascinamento (chiamati da `dnd`) ---------------------

pub(super) fn drag_enter() -> bool {
    with_app(App::on_drag_enter).unwrap_or(false)
}

pub(super) fn drag_leave() {
    with_app(App::on_drag_leave);
}

pub(super) fn drop(payload: Option<Payload>) -> bool {
    with_app(|app| app.on_drop(payload)).unwrap_or(false)
}

pub fn run() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // OLE (e quindi COM) serve per il trascinamento da e verso lo Shelf.
        OleInitialize(None)?;

        // `--snapshot <cartella>`: anteprime della grafica, anche con Skylight già aperto.
        let mut args = std::env::args().skip(1);
        let snapshot = (args.next().as_deref() == Some("--snapshot"))
            .then(|| PathBuf::from(args.next().unwrap_or_else(|| "snapshots".into())));

        // Una sola istanza: il mutex resta aperto finché il processo vive.
        let _single_instance = CreateMutexW(None, true, w!("Local\\Skylight.SingleInstance"))?;
        if snapshot.is_none() && GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        // Prima di leggere dati e impostazioni: chi arriva dal nome vecchio li ritrova.
        // Le anteprime non toccano niente.
        if snapshot.is_none() {
            super::migration::run();
        }

        let module = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(Error::from_thread());
        }
        settings_window::register(module.into(), Some(settings_wndproc))?;

        // Finestra senza bordi, sempre in primo piano, fuori dalla barra delle
        // applicazioni, che non ruba il focus e senza bitmap di sistema:
        // il contenuto arriva da DirectComposition.
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            CLASS_NAME,
            w!("Skylight"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(module.into()),
            None,
        )?;

        APP.with(|cell| *cell.borrow_mut() = Some(App::new(hwnd)));
        // Anteprime della grafica: si disegna in memoria e si esce.
        if let Some(dir) = snapshot {
            let result = with_app(|app| app.snapshot(&dir)).unwrap_or(Ok(()));
            APP.with(|cell| cell.borrow_mut().take());
            return result;
        }
        with_app(|app| {
            app.place();
            app.refresh_fullscreen();
            app.apply_keyboard_setting();
            app.request_thumbnails();
            app.tick_features();
            app.schedule_tick();
            SetTimer(Some(app.hwnd), TIMER_FULLSCREEN, 1_000, None);
        });

        let target: IDropTarget = DropTarget::default().into();
        let _ = RegisterDragDrop(hwnd, &target);
        // Schermo intero: la shell avvisa le "appbar" registrate (evento, nessun
        // polling); il cambio di finestra in primo piano copre l'Alt+Tab.
        appbar(hwnd, ABM_NEW);
        let foreground_hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );

        message_loop();

        let _ = UnhookWinEvent(foreground_hook);
        appbar(hwnd, ABM_REMOVE);
        let _ = RevokeDragDrop(hwnd);
        APP.with(|cell| cell.borrow_mut().take()); // rimuove l'icona dalla tray
    }
    Ok(())
}

fn message_loop() {
    let mut msg = MSG::default();
    loop {
        unsafe {
            if with_app(|app| app.wants_frame()).unwrap_or(false) {
                // Animazione in corso: smaltisci i messaggi e disegna un frame
                // (Present aspetta il vsync, quindi il ciclo non gira a vuoto).
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        return;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                with_app(|app| {
                    app.sync();
                    app.frame();
                });
            } else {
                // Nulla da animare: dormi finché arriva un messaggio.
                if GetMessageW(&mut msg, None, 0, 0).0 <= 0 {
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
                with_app(App::sync);
            }
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_APP_TRAY {
        let event = (lp.0 & 0xFFFF) as u32;
        if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
            // Il menu ha un proprio ciclo di messaggi: non teniamo l'app
            // "occupata" mentre è aperto, altrimenti perderemmo eventi.
            if let Some(state) = with_app(|app| app.menu_state()) {
                set_modal(hwnd, true);
                let chosen = tray::show_menu(hwnd, state);
                set_modal(hwnd, false);
                with_app(|app| app.on_menu(chosen));
            }
        }
        return LRESULT(0);
    }
    let result = with_app(|app| app.handle(msg, wp, lp)).flatten();
    // Anche il trascinamento ha un proprio ciclo: parte fuori dal prestito dell'app.
    if let Some(drag) = with_app(|app| app.pending_drag.take()).flatten() {
        with_app(|app| app.dragging_out = true);
        set_modal(hwnd, true);
        let _ = dnd::drag_out(&drag.files, drag.text.as_deref());
        set_modal(hwnd, false);
        with_app(App::drag_finished);
    }
    // Aprire un file scaricato può mostrare l'avviso di sicurezza di Windows,
    // che gira sul nostro thread: i messaggi che arrivano intanto non vanno persi.
    if let Some(path) = with_app(|app| app.pending_open.take()).flatten() {
        set_modal(hwnd, true);
        dnd::open_path(&path);
        set_modal(hwnd, false);
    }
    result.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, msg, wp, lp) })
}

/// Accende o spegne il timer che disegna i frame durante un ciclo modale.
fn set_modal(hwnd: HWND, modal: bool) {
    unsafe {
        if modal {
            SetTimer(Some(hwnd), TIMER_FRAME, 16, None);
        } else {
            let _ = KillTimer(Some(hwnd), TIMER_FRAME);
        }
    }
}

extern "system" fn settings_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    with_app(|app| app.settings_message(hwnd, msg, wp, lp))
        .flatten()
        .unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, msg, wp, lp) })
}

extern "system" fn on_foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    with_app(|app| {
        app.keyboard.rebase();
        app.refresh_fullscreen();
    });
}

/// Registra (o rimuove) la finestra come appbar senza riservare spazio sullo
/// schermo: serve solo a ricevere `ABN_FULLSCREENAPP`.
fn appbar(hwnd: HWND, message: u32) {
    let mut data = APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        uCallbackMessage: WM_APP_APPBAR,
        ..Default::default()
    };
    unsafe { SHAppBarMessage(message, &mut data) };
}

/// Cosa mostra il notch chiuso: cambiare tipo fa una breve dissolvenza.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClosedKind {
    Mini,
    Idle,
    Active,
    Alert(AlertKind),
}

/// Trascinamento verso l'esterno in attesa di partire.
struct Drag {
    files: Vec<String>,
    text: Option<String>,
}

pub(super) struct App {
    pub(super) hwnd: HWND,
    gfx: Option<Gfx>,
    render_failures: u32,
    dpi: u32,
    monitor: HMONITOR,
    visible: bool,
    /// Un'app a schermo intero copre il monitor del notch.
    covered: bool,
    /// Un'app è a schermo intero: il notch chiuso è solo una linguetta.
    pub(super) mini: bool,
    pub(super) settings: Settings,
    data_dir: Option<PathBuf>,

    // Animazione
    pub(super) width: Spring,
    pub(super) height: Spring,
    pub(super) openness: Spring,
    /// Scorrimento delle righe del testo (da 1 a 0).
    pub(super) scroll: Spring,
    /// Dissolvenza quando cambia il contenuto del notch chiuso.
    pub(super) swap: Spring,
    /// Passaggio da una pagina all'altra (da 0 a 1): la nuova entra di lato.
    pub(super) page_fade: Spring,
    /// Pagina che sta uscendo e verso dove (+1 la nuova arriva da destra).
    pub(super) slide_from: Option<Page>,
    pub(super) slide_dir: f32,
    /// Opacità della pagina che esce all'inizio del passaggio.
    pub(super) slide_from_alpha: f32,
    /// Barra delle icone al posto dei pallini (il cursore è in fondo al notch).
    bar_shown: bool,
    /// Dopo un cambio pagina il notch non si chiude finché il cursore resta dove
    /// era il notch prima (scadenza, bordo inferiore in DIP).
    collapse_grace: Option<(Instant, f32)>,
    /// Barra delle icone in basso (da 0 = pallini a 1 = barra).
    pub(super) footer_reveal: Spring,
    reduce_motion: bool,
    last_frame: Option<Instant>,
    dirty: bool,
    region: [i32; 4],
    closed_kind: ClosedKind,

    // Input
    hover: bool,
    pub(super) open: bool,
    pub(super) hot: Option<Hit>,
    pressed: Option<(Hit, f32, f32)>,
    pub(super) page: Page,
    /// Quando si è cambiata pagina: il secondo clic di un doppio clic sulla
    /// barra non deve finire sul contenuto nuovo, ancora invisibile.
    page_switched_at: Option<Instant>,
    /// Posizione del cursore quando la forma del notch è cambiata da sola (un
    /// avviso che si allarga, la finestra che ricompare): il WM_MOUSEMOVE che
    /// Windows manda allora non è un movimento vero e non deve aprire il notch.
    parked_cursor: Option<POINT>,
    wheel: i32,
    pub(super) shelf_scroll: usize,
    pub(super) todo_scroll: usize,
    pub(super) selection: HashSet<u64>,
    /// Finestra che aveva il focus prima di scrivere un'attività: le si restituisce dopo.
    previous_focus: Option<HWND>,
    /// Breve messaggio nell'intestazione dello Shelf (es. "Copiato") e sua scadenza.
    pub(super) note: Option<(&'static str, u64)>,
    pending_drag: Option<Drag>,
    /// File da aprire fuori dal prestito dell'app: ShellExecute può mostrare
    /// finestre di avviso con un proprio ciclo dei messaggi.
    pending_open: Option<String>,
    dragging_out: bool,
    pub(super) drop_hover: bool,
    pub(super) editing: Option<TextEdit>,
    /// Scorrimento orizzontale del campo di testo (DIP), aggiornato mentre si disegna.
    pub(super) edit_scroll: Cell<f32>,
    high_surrogate: Option<u16>,

    // Musica e testi
    media_inbox: Arc<Inbox<Option<MediaSnapshot>>>,
    lyrics_inbox: Arc<Replies>,
    pub(super) media: Option<MediaSnapshot>,
    control: MediaControl,
    lyrics_requests: Sender<(u64, Query)>,
    lyrics: HashMap<u64, LyricsState>,
    pub(super) line: Option<usize>,
    pill_text_w: f32,
    /// Per quale (brano, riga) è stata misurata `pill_text_w`.
    pill_measured: Option<(u64, usize)>,
    track_changed_at: Option<Instant>,
    pub(super) music: MusicLayout,

    // Funzioni
    pub(super) pomodoro: Pomodoro,
    pub(super) water: WaterLog,
    water_snooze: Option<i64>,
    pub(super) todo: TodoList,
    pub(super) shelf: Shelf,
    pub(super) alerts: Alerts,
    today: i64,
    thumb_inbox: Arc<Thumbnails>,
    thumb_requests: Sender<(u64, String)>,
    thumb_requested: HashSet<u64>,
    pub(super) thumbs: HashMap<u64, Arc<Artwork>>,

    /// Volume dell'uscita predefinita (livello, silenzioso) e se la barra della
    /// pagina Musica lo sta regolando.
    pub(super) volume: Option<(f32, bool)>,
    pub(super) volume_mode: bool,
    /// Pagina Acqua: grafico per ora invece dell'indicatore circolare.
    pub(super) water_chart: bool,

    // Sistema
    pub(super) battery: Option<Battery>,
    /// Un file sta arrivando nella cartella Download (vedi `showing_download`).
    downloading: bool,
    download_since: Instant,
    system_inbox: Arc<Events>,
    power: Power,
    last_power_change: Option<Instant>,
    pub(super) keyboard: Keyboard,
    stats_inbox: Arc<Inbox<Stats>>,
    sysmon: Sender<Control>,
    sysmon_running: bool,
    pub(super) stats: Stats,
    pub(super) history: [History; 4],

    // Finestre
    settings_window: Option<SettingsWindow>,
    tray: tray::Tray,
    taskbar_created: u32,
}

/// Indici di `App::history`.
pub(super) const HIST_CPU: usize = 0;
pub(super) const HIST_GPU: usize = 1;
pub(super) const HIST_DOWN: usize = 2;
pub(super) const HIST_UP: usize = 3;

impl App {
    fn new(hwnd: HWND) -> Self {
        let notifier = sys::Notifier::new(hwnd, WM_APP_WAKE);
        let media_inbox = Arc::new(Inbox::new());
        let lyrics_inbox = Arc::new(Inbox::new());
        let thumb_inbox = Arc::new(Inbox::new());
        let system_inbox = Arc::new(Inbox::new());
        let stats_inbox = Arc::new(Inbox::new());
        monitors::spawn_all(&system_inbox, notifier);

        let data_dir = store::data_dir();
        let clock = sys::clock();
        let mut todo: TodoList = load(data_dir.as_deref(), "todo.json");
        todo.prune(&clock);
        let mut water: WaterLog = load(data_dir.as_deref(), "water.json");
        water.prune(&clock, 30);
        let shelf: Shelf = load(data_dir.as_deref(), "shelf.json");

        Self {
            hwnd,
            gfx: None,
            render_failures: 0,
            dpi: 96,
            monitor: HMONITOR::default(),
            visible: false,
            covered: false,
            mini: false,
            settings: settings_store::load(),
            data_dir,
            width: Spring::size(CLOSED_IDLE_W),
            height: Spring::size(CLOSED_H),
            openness: Spring::unit(0.0),
            scroll: Spring::unit(0.0),
            swap: Spring::unit(1.0),
            page_fade: Spring::unit(1.0),
            slide_from: None,
            slide_dir: 1.0,
            slide_from_alpha: 1.0,
            bar_shown: false,
            collapse_grace: None,
            footer_reveal: Spring::unit(0.0),
            reduce_motion: sys::reduce_motion(),
            last_frame: None,
            dirty: true,
            region: [0; 4],
            closed_kind: ClosedKind::Idle,
            hover: false,
            open: false,
            hot: None,
            pressed: None,
            page: Page::Music,
            page_switched_at: None,
            parked_cursor: None,
            wheel: 0,
            shelf_scroll: 0,
            todo_scroll: 0,
            selection: HashSet::new(),
            previous_focus: None,
            note: None,
            pending_drag: None,
            pending_open: None,
            dragging_out: false,
            drop_hover: false,
            editing: None,
            edit_scroll: Cell::new(0.0),
            high_surrogate: None,
            control: gsmtc::spawn(media_inbox.clone(), notifier),
            lyrics_requests: fetch::spawn(lyrics_inbox.clone(), notifier),
            media_inbox,
            lyrics_inbox,
            media: None,
            lyrics: HashMap::new(),
            line: None,
            pill_text_w: 0.0,
            pill_measured: None,
            track_changed_at: None,
            music: MusicLayout::new(),
            pomodoro: Pomodoro::default(),
            water,
            // Niente promemoria appena acceso il PC.
            water_snooze: Some(clock.now + 120),
            todo,
            shelf,
            alerts: Alerts::default(),
            today: clock.today(),
            thumb_requests: thumbs::spawn(thumb_inbox.clone(), notifier),
            thumb_inbox,
            thumb_requested: HashSet::new(),
            thumbs: HashMap::new(),
            volume: None,
            volume_mode: false,
            water_chart: false,
            battery: power::battery(),
            downloading: false,
            download_since: Instant::now(),
            system_inbox,
            power: Power::register(hwnd),
            last_power_change: None,
            keyboard: Keyboard::new(),
            sysmon: sysmon::spawn(stats_inbox.clone(), notifier),
            stats_inbox,
            sysmon_running: false,
            stats: Stats::default(),
            history: std::array::from_fn(|_| History::new(40)),
            settings_window: None,
            tray: tray::Tray::new(hwnd, WM_APP_TRAY),
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        }
    }

    // ---- Stato calcolato ------------------------------------------------------------

    /// "Mostra animazioni" è spento in Windows: niente rimbalzi, scorrimenti o sfocature.
    pub(in crate::win) fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }

    /// Si vede l'anello del download, e il notch resta normale anche a schermo intero.
    pub(super) fn showing_download(&self) -> bool {
        self.downloading && self.settings.downloads
    }

    /// Posizione dell'arco del download (0..1 di giro), `None` se resta fermo.
    pub(super) fn download_turn(&self) -> Option<f32> {
        (!self.reduce_motion)
            .then(|| (self.download_since.elapsed().as_secs_f32() / SPIN_PERIOD_S).fract())
    }

    /// Accende o spegne il timer che fa girare l'arco del download.
    pub(super) fn update_spinner(&self) {
        let spin = self.showing_download() && self.visible && !self.mini && !self.reduce_motion;
        unsafe {
            if spin {
                SetTimer(Some(self.hwnd), TIMER_SPIN, 33, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), TIMER_SPIN);
            }
        }
    }

    /// Un download è iniziato o finito.
    pub(super) fn set_downloading(&mut self, on: bool) {
        if on && !self.downloading {
            self.download_since = Instant::now();
        }
        self.downloading = on;
        self.refresh_fullscreen();
        self.update_spinner();
        self.dirty = true;
    }

    /// Prossimo promemoria dell'acqua, se attivi.
    pub(in crate::win) fn water_next_reminder(&self) -> Option<i64> {
        let plan = crate::features::water::WaterPlan::from_settings(&self.settings);
        crate::features::water::next_reminder(&self.water, &plan, &sys::clock(), self.water_snooze)
    }

    pub(super) fn has_track(&self) -> bool {
        self.media.as_ref().is_some_and(MediaSnapshot::has_track)
    }

    /// C'è qualcosa da mostrare nel notch chiuso oltre all'orologio.
    pub(super) fn is_active(&self) -> bool {
        self.has_track() || self.pomodoro.is_active()
    }

    /// Elementi dello Shelf visibili (dopo lo scorrimento).
    pub(super) fn shelf_visible(&self) -> &[crate::features::shelf::Entry] {
        let start = (self.shelf_scroll * SHELF_COLS).min(self.shelf.len());
        let end = (start + SHELF_COLS * SHELF_ROWS).min(self.shelf.len());
        &self.shelf.entries[start..end]
    }

    /// Attività visibili (dopo lo scorrimento), nell'ordine mostrato.
    pub(super) fn todo_visible(&self) -> Vec<&crate::features::todo::Task> {
        self.todo
            .ordered()
            .into_iter()
            .skip(self.todo_scroll)
            .take(TODO_ROWS)
            .collect()
    }

    /// Altezza del contenuto della pagina corrente (intestazione e piè di pagina esclusi).
    pub(super) fn content_height(&self) -> f32 {
        self.page_height(self.page)
    }

    pub(super) fn page_height(&self, page: Page) -> f32 {
        match page {
            Page::Music if self.has_track() && self.settings.lyrics => MUSIC_H + LYRICS_H,
            Page::Music => MUSIC_H,
            Page::Focus => FOCUS_H,
            Page::Water => WATER_H,
            Page::Shelf => shelf_height(self.shelf.len()),
            Page::Todo => todo_height(self.todo_visible().len()),
            Page::System => SYSTEM_H,
        }
    }

    /// Larghezza del notch chiuso a riposo: orologio, data se scelta, batteria.
    fn idle_width(&self) -> f32 {
        // L'ora resta al centro: con l'anello della batteria a sinistra (o del
        // download a destra) serve lo stesso margine su entrambi i lati.
        let text = self.idle_text();
        let ring = self.battery.is_some() || self.showing_download();
        let margin = if ring { 42.0 } else { 24.0 };
        (self.measure_clock(&text) + 2.0 * margin).max(CLOSED_IDLE_W)
    }

    /// Testo del notch chiuso a riposo: l'ora, con la data se scelta.
    pub(in crate::win) fn idle_text(&self) -> String {
        if self.settings.closed_date {
            format!("{}  {}", sys::short_date_text(false), sys::clock_text())
        } else {
            sys::clock_text()
        }
    }

    fn closed_kind(&self) -> ClosedKind {
        if self.mini {
            return ClosedKind::Mini;
        }
        match self.alerts.current() {
            Some(alert) => ClosedKind::Alert(alert.kind),
            None if self.is_active() => ClosedKind::Active,
            None => ClosedKind::Idle,
        }
    }

    // ---- Animazione e disegno ---------------------------------------------------------

    /// Dimensioni a cui il notch deve tendere nello stato attuale, con la molla
    /// adatta: quella di apertura, di chiusura o degli avvisi.
    fn retarget(&mut self) {
        self.measure_pill_if_needed();
        let ((w, h), shape, fade) = if self.open {
            (
                (OPEN_W, open_height(self.content_height())),
                OPEN_SPRING,
                OPEN_FADE,
            )
        } else if self.mini {
            ((MINI_W, MINI_H), CLOSE_SPRING, CLOSE_FADE)
        } else if let Some(alert) = self.alerts.current() {
            (
                (ALERT_W, AlertShape::of(alert).height()),
                ALERT_SPRING,
                CLOSE_FADE,
            )
        } else if self.is_active() {
            let w = if self.pill_line().is_some() {
                pill_width(self.pill_text_w)
            } else {
                CLOSED_PLAYING_W
            };
            ((w, CLOSED_H), CLOSE_SPRING, CLOSE_FADE)
        } else {
            ((self.idle_width(), CLOSED_H), CLOSE_SPRING, CLOSE_FADE)
        };
        let calm = self.reduce_motion;
        let tune = |spring: &mut Spring, (response, damping): (f32, f32)| {
            spring.tune(response, if calm { 1.0 } else { damping });
        };
        tune(&mut self.width, shape);
        tune(&mut self.height, shape);
        tune(&mut self.openness, fade);
        tune(&mut self.page_fade, PAGE_SPRING);
        tune(&mut self.footer_reveal, REVEAL_SPRING);
        self.width.set_target(w);
        self.height.set_target(h);
        self.openness.set_target(if self.open { 1.0 } else { 0.0 });

        let kind = self.closed_kind();
        if kind != self.closed_kind {
            self.closed_kind = kind;
            self.swap.snap(0.0);
            self.swap.set_target(1.0);
        }
        self.update_sysmon();
        self.dirty = true;
    }

    fn springs(&mut self) -> [&mut Spring; 7] {
        [
            &mut self.width,
            &mut self.height,
            &mut self.openness,
            &mut self.scroll,
            &mut self.swap,
            &mut self.page_fade,
            &mut self.footer_reveal,
        ]
    }

    fn animating(&self) -> bool {
        ![
            &self.width,
            &self.height,
            &self.openness,
            &self.scroll,
            &self.swap,
            &self.page_fade,
            &self.footer_reveal,
        ]
        .iter()
        .all(|s| s.is_settled())
    }

    fn wants_frame(&self) -> bool {
        let notch = self.visible && (self.dirty || self.animating());
        notch
            || self
                .settings_window
                .as_ref()
                .is_some_and(SettingsWindow::is_dirty)
    }

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = self
            .last_frame
            .map_or(1.0 / 60.0, |t| (now - t).as_secs_f32());
        self.update_line();
        for spring in self.springs() {
            spring.step(dt);
        }
        if self.page_fade.is_settled() {
            self.slide_from = None;
        }
        self.last_frame = self.animating().then_some(now);
        self.dirty = false;
        if self.visible {
            self.update_region();
            self.render();
        }
        if let Some(window) = self.settings_window.as_mut().filter(|w| w.is_dirty()) {
            window.render(&self.settings, sys::monitors().len() as u32);
        }
        self.schedule_tick();
    }

    fn render(&mut self) {
        if self.gfx.is_none() {
            let (w, h) = self.canvas_px();
            self.gfx = Gfx::new(self.hwnd, w, h, 96.0 * self.scale()).ok();
            // Con DirectWrite pronto la misura della riga del testo è esatta.
            self.pill_measured = None;
        }
        let Some(mut gfx) = self.gfx.take() else {
            return;
        };
        let drawn = render::draw(&mut gfx, self).is_ok();
        if drawn {
            // Anteprime di elementi tolti dallo Shelf: si liberano dalla GPU.
            gfx.retain_images(|id| {
                id & THUMB_KEY == 0 || self.thumbs.contains_key(&(id & !THUMB_KEY))
            });
            self.gfx = Some(gfx);
            self.render_failures = 0;
        } else if self.render_failures < 3 {
            // Scheda video resettata o driver aggiornato: si ricrea tutto al prossimo frame.
            self.render_failures += 1;
            self.dirty = true;
        }
    }

    /// Scala totale: DPI del monitor per la dimensione scelta nelle impostazioni.
    fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0 * self.settings.notch_scale as f32 / 100.0
    }

    /// La finestra è grande quanto la tela, ma riceve il mouse solo dentro il notch.
    fn update_region(&mut self) {
        let notch = notch_rect(self.width.value(), self.height.value());
        let k = self.scale();
        let px = [
            ((notch.x - EAR) * k).floor() as i32,
            0,
            ((notch.right() + EAR) * k).ceil() as i32,
            (notch.bottom() * k).ceil() as i32,
        ];
        if px != self.region {
            self.region = px;
            unsafe {
                let region = CreateRectRgn(px[0], px[1], px[2], px[3]);
                SetWindowRgn(self.hwnd, Some(region), false);
            }
            if !self.hover {
                self.parked_cursor = cursor_pos();
            }
        }
    }

    // ---- Posizione sullo schermo ---------------------------------------------------

    fn canvas_px(&self) -> (u32, u32) {
        let k = self.scale();
        ((CANVAS_W * k).ceil() as u32, (CANVAS_H * k).ceil() as u32)
    }

    /// Centra la tela in cima al monitor scelto, alla scala di quel monitor.
    fn place(&mut self) {
        let monitors = sys::monitors();
        let chosen = match self.settings.monitor {
            0 => None,
            n => monitors.get(n as usize - 1),
        };
        let Some(m) = chosen
            .or_else(|| monitors.iter().find(|m| m.primary))
            .or(monitors.first())
        else {
            return;
        };
        unsafe {
            let (mut dpi_x, mut dpi_y) = (96, 96);
            let _ = GetDpiForMonitor(m.handle, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            self.monitor = m.handle;
            self.dpi = dpi_x.max(96);

            let (w, h) = self.canvas_px();
            let x = m.rect.left + (m.rect.right - m.rect.left - w as i32) / 2;
            // Area di lavoro: se la barra delle applicazioni è in alto, il notch sta sotto.
            // A schermo intero la barra non c'è: il notch torna al bordo dello schermo.
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                if self.covered { m.rect.top } else { m.work.top },
                w as i32,
                h as i32,
                SWP_NOACTIVATE,
            );
            let dpi = 96.0 * self.scale();
            if let Some(gfx) = &mut self.gfx
                && gfx.resize(w, h, dpi).is_err()
            {
                self.gfx = None;
            }
        }
        self.region = [0; 4];
        self.update_region();
        self.dirty = true;
    }

    /// Un'app a schermo intero copre il monitor del notch? A seconda
    /// dell'impostazione il notch si riduce a una linguetta, si nasconde o resta.
    fn refresh_fullscreen(&mut self) {
        let mode = self.settings.fullscreen_mode;
        let covered = mode != FULLSCREEN_NORMAL
            && unsafe {
                fullscreen_app_on(self.monitor, self.hwnd, self.settings.shrink_maximized)
            };
        // Durante un download il notch resta normale, perché l'anello si veda.
        let hold = self.showing_download();
        let visible = !(covered && mode == FULLSCREEN_HIDE && !hold);
        let mini = covered && mode == FULLSCREEN_SHRINK && !hold;
        if covered != self.covered {
            self.covered = covered;
            self.place();
        }
        if visible == self.visible && mini == self.mini {
            return;
        }
        let shown_again = visible && !self.visible;
        self.visible = visible;
        self.mini = mini;
        if shown_again {
            self.parked_cursor = cursor_pos();
        }
        // Un promemoria rimasto in attesa a schermo intero ha di nuovo tutto il suo tempo.
        if visible && !mini && !self.open {
            self.alerts.resume(time::now_ms());
        }
        unsafe {
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
            if visible {
                // Sopra anche all'app a schermo intero.
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
            }
        }
        self.update_spinner();
        self.retarget();
    }

    // ---- Finestra Impostazioni --------------------------------------------------------

    fn open_settings(&mut self) {
        match &self.settings_window {
            Some(window) => window.focus(),
            None => self.settings_window = SettingsWindow::create().ok(),
        }
    }

    fn settings_message(
        &mut self,
        hwnd: HWND,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
    ) -> Option<LRESULT> {
        let window = self.settings_window.as_mut().filter(|w| w.hwnd() == hwnd)?;
        // Spostando la finestra Windows entra in un ciclo modale: il notch continua a vivere.
        match msg {
            WM_ENTERSIZEMOVE => set_modal(self.hwnd, true),
            WM_EXITSIZEMOVE => set_modal(self.hwnd, false),
            _ => {}
        }
        let reply = window.handle(msg, wp, lp, &self.settings);
        if let Some((field, direction)) = reply.change {
            field.step(&mut self.settings, direction, sys::monitors().len() as u32);
            self.apply_setting(field);
            if let Some(window) = &mut self.settings_window {
                window.invalidate();
            }
        }
        if reply.closed {
            self.settings_window = None;
        }
        reply.result
    }
}

/// Posizione del cursore sullo schermo.
fn cursor_pos() -> Option<POINT> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.ok().map(|_| point)
}

/// Dati salvati (o valori vuoti se manca la cartella o il file).
fn load<T: serde::de::DeserializeOwned + Default>(dir: Option<&std::path::Path>, name: &str) -> T {
    dir.map(|d| store::load(d, name)).unwrap_or_default()
}

/// La finestra in primo piano copre tutto il monitor del notch? (video, giochi, slide)
/// Con `maximized` contano anche le finestre ingrandite (con la barra delle applicazioni visibile).
unsafe fn fullscreen_app_on(monitor: HMONITOR, ours: HWND, maximized: bool) -> bool {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() || foreground == ours {
            return false;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(foreground, &mut class).max(0) as usize;
        // Desktop e barre delle applicazioni coprono lo schermo ma non contano.
        if matches!(
            String::from_utf16_lossy(&class[..len]).as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return false;
        }
        if MonitorFromWindow(foreground, MONITOR_DEFAULTTONULL) != monitor {
            return false;
        }
        let zoomed = IsZoomed(foreground).as_bool();
        if zoomed && maximized {
            return true;
        }
        let mut rect = RECT::default();
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetWindowRect(foreground, &mut rect).is_ok()
            && GetMonitorInfoW(monitor, &mut info).as_bool()
        {
            let m = info.rcMonitor;
            let covers = rect.left <= m.left
                && rect.top <= m.top
                && rect.right >= m.right
                && rect.bottom >= m.bottom;
            // Una finestra ingrandita con la barra a scomparsa copre lo schermo ma
            // sborda di qualche pixel; a schermo intero (F11, video) è grande esattamente
            // quanto il monitor, anche se Windows la considera ancora "ingrandita".
            if covers && (!zoomed || rect == m) {
                return true;
            }
        }
        // Windows stesso sa se c'è un'app a schermo intero (giochi, video, presentazioni).
        matches!(
            SHQueryUserNotificationState(),
            Ok(QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN)
        )
    }
}
