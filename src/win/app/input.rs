//! Input: messaggi della finestra, mouse, rotella, tastiera (solo mentre si
//! scrive un'attività), clic sulle pagine, menu della tray e trascinamento.

use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{ScreenToClient, ValidateRect};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetDoubleClickTime, GetFocus, GetKeyState, SetFocus, TME_LEAVE, TRACKMOUSEEVENT,
    TrackMouseEvent, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_DELETE, VK_END, VK_ESCAPE, VK_HOME,
    VK_LEFT, VK_RETURN, VK_RIGHT,
};
use windows::Win32::UI::Shell::{ABM_NEW, ABN_FULLSCREENAPP, ABN_POSCHANGED};
use windows::Win32::UI::WindowsAndMessaging::*;

use std::time::Duration;

use super::*;
use crate::features::alerts::{AlertAction, AlertKind};
use crate::features::pomodoro::Durations;
use crate::features::todo::parse_entry;
use crate::media::MediaCommand;
use crate::settings::Field;
use crate::win::{power, volume};

const COLLAPSE_DELAY_MS: u32 = 260;
/// Spostamento minimo (DIP) per trasformare un clic in un trascinamento.
const DRAG_THRESHOLD: f32 = 6.0;
/// Pressione del tasto sinistro nel `wParam` di `WM_MOUSEMOVE`.
const MK_LBUTTON: usize = 0x0001;
const MK_CONTROL: usize = 0x0008;

impl App {
    pub(super) fn handle(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_MOUSEMOVE => {
                let (x, y) = self.to_dip(lp);
                self.on_mouse_move(x, y, wp.0 & MK_LBUTTON != 0);
            }
            WM_MOUSELEAVE => self.on_mouse_leave(),
            WM_LBUTTONDOWN => {
                let (x, y) = self.to_dip(lp);
                self.pressed = self.hit(x, y).map(|hit| (hit, x, y));
            }
            WM_LBUTTONUP => {
                let (x, y) = self.to_dip(lp);
                let released_on = self.hit(x, y);
                if let Some((hit, _, _)) = self
                    .pressed
                    .take()
                    .filter(|(p, _, _)| released_on == Some(*p))
                {
                    // Lo stato di Ctrl arriva col messaggio: affidabile anche senza focus.
                    self.on_click(hit, x, wp.0 & MK_CONTROL != 0);
                }
            }
            WM_MOUSEWHEEL => self.on_wheel(wp, lp),
            // Cliccare il notch non deve togliere il focus alla finestra in uso.
            WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_SETCURSOR => {
                let cursor = if self.hot.is_some() {
                    IDC_HAND
                } else {
                    IDC_ARROW
                };
                unsafe { SetCursor(LoadCursorW(None, cursor).ok()) };
                return Some(LRESULT(1));
            }
            WM_KEYDOWN if self.editing.is_some() => self.on_key(VIRTUAL_KEY(wp.0 as u16)),
            WM_CHAR if self.editing.is_some() => self.on_char(wp.0 as u16),
            WM_KILLFOCUS => self.stop_editing(true),
            WM_TIMER => match wp.0 {
                TIMER_COLLAPSE => {
                    self.kill_timer(TIMER_COLLAPSE);
                    // Se il focus è finito altrove (anche senza WM_KILLFOCUS) la scrittura è finita.
                    if self.editing.is_some() && unsafe { GetFocus() } != self.hwnd {
                        self.stop_editing(true);
                    }
                    if self.cursor_in_grace() {
                        unsafe { SetTimer(Some(self.hwnd), TIMER_COLLAPSE, 120, None) };
                    } else if !self.hover && self.editing.is_none() && !self.drop_hover {
                        self.set_open(false);
                    }
                }
                TIMER_TICK => {
                    // Viene riprogrammato subito con l'intervallo giusto.
                    self.kill_timer(TIMER_TICK);
                    self.tick_features();
                    self.dirty = true;
                    self.schedule_tick();
                }
                TIMER_LYRICS => {
                    self.kill_timer(TIMER_LYRICS);
                    // L'attesa è finita: il controllo "brano appena cambiato" non serve più.
                    self.track_changed_at = None;
                    self.request_lyrics();
                    self.dirty = true;
                }
                TIMER_FRAME => {
                    if self.wants_frame() {
                        self.sync();
                        self.frame();
                    }
                }
                TIMER_FULLSCREEN => self.refresh_fullscreen(),
                TIMER_SPIN => self.dirty = true,
                TIMER_KEYS => {
                    self.kill_timer(TIMER_KEYS);
                    self.keyboard_check();
                }
                _ => return None,
            },
            WM_INPUT => {
                if let Some(delay) = self.keyboard.on_raw_input(lp) {
                    unsafe { SetTimer(Some(self.hwnd), TIMER_KEYS, delay, None) };
                }
                // Windows vuole DefWindowProc per liberare i dati del raw input.
                return None;
            }
            WM_POWERBROADCAST => {
                if let Some(event) = self.power.on_message(wp, lp) {
                    self.power_alert(event);
                }
                self.battery = power::battery();
                self.retarget();
                return Some(LRESULT(1));
            }
            // I dati nuovi vengono letti in `sync`, dopo ogni messaggio.
            WM_APP_WAKE => {}
            WM_APP_APPBAR => match wp.0 as u32 {
                ABN_FULLSCREENAPP => self.refresh_fullscreen(),
                ABN_POSCHANGED => self.place(),
                _ => {}
            },
            WM_DPICHANGED | WM_DISPLAYCHANGE => self.place(),
            WM_SETTINGCHANGE => {
                // La barra delle applicazioni si è spostata o nascosta: cambia l'area di lavoro.
                if wp.0 as u32 == SPI_SETWORKAREA.0 {
                    self.place();
                }
                self.reduce_motion = sys::reduce_motion();
                self.update_spinner();
                return None;
            }
            WM_PAINT => {
                unsafe {
                    let _ = ValidateRect(Some(self.hwnd), None);
                }
                self.dirty = true;
            }
            WM_ERASEBKGND => return Some(LRESULT(1)),
            // Alt+F4 con il focus sul notch chiuderebbe Skylight: si esce solo dalla tray.
            WM_CLOSE => {}
            WM_DESTROY => unsafe { PostQuitMessage(0) },
            // Explorer è stato riavviato: icona e appbar vanno registrate di nuovo.
            m if m == self.taskbar_created => {
                self.tray.add();
                appbar(self.hwnd, ABM_NEW);
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }

    pub(super) fn kill_timer(&self, id: usize) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), id);
        }
    }

    fn to_dip(&self, lp: LPARAM) -> (f32, f32) {
        let x = (lp.0 & 0xFFFF) as u16 as i16 as f32;
        let y = ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as f32;
        let k = self.scale();
        (x / k, y / k)
    }

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        if !self.open {
            return self
                .alerts
                .current()
                .filter(|_| !self.mini)
                .and_then(|alert| alert_hit(AlertShape::of(alert), x, y));
        }
        if self.openness.value() < 0.85 {
            return None;
        }
        let content_h = self.content_height();
        let (prev, next) = arrows();
        if prev.contains(x, y) {
            return Some(Hit::PrevPage);
        }
        if next.contains(x, y) {
            return Some(Hit::NextPage);
        }
        let footer = footer(content_h);
        if footer.zone.contains(x, y) {
            return footer.hit(x, y, self.bar_shown);
        }
        let double_click = unsafe { GetDoubleClickTime() } as u128;
        if self
            .page_switched_at
            .is_some_and(|t| t.elapsed().as_millis() < double_click)
        {
            return None;
        }
        match self.page {
            Page::Music => self.music.hit(x, y, self.lyric_targets(), self.volume_mode),
            Page::System => None,
            Page::Focus => FocusLayout::new().hit(x, y, self.pomodoro.phase()),
            Page::Water => WaterLayout::new().hit(x, y),
            Page::Shelf => shelf_hit(x, y, self.shelf_visible().len(), !self.shelf.is_empty()),
            Page::Todo => todo_hit(x, y, self.todo_visible().len(), self.todo.has_done()),
        }
    }

    fn on_mouse_move(&mut self, x: f32, y: f32, left_button: bool) {
        if !self.hover {
            // Il notch si è allargato sotto un cursore fermo: non è l'utente che arriva.
            let cursor = cursor_pos();
            if cursor.is_some() && cursor == self.parked_cursor {
                return;
            }
            self.parked_cursor = None;
            self.hover = true;
            self.collapse_grace = None;
        }
        // A ogni movimento, non solo il primo: WM_MOUSELEAVE arriva una volta sola e,
        // se un ciclo modale lo perde, così il tracciamento riparte.
        let mut track = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.hwnd,
            dwHoverTime: 0,
        };
        unsafe {
            let _ = TrackMouseEvent(&mut track);
        }
        self.kill_timer(TIMER_COLLAPSE);
        // In fondo al notch aperto i pallini diventano la barra delle icone.
        // Cosa si clicca dipende da dove è il cursore, non da quanto è avanzata l'animazione.
        let reveal = self.open && footer(self.content_height()).zone.contains(x, y);
        self.bar_shown = reveal;
        self.footer_reveal
            .set_target(if reveal { 1.0 } else { 0.0 });

        // Tenendo premuto su un elemento dello Shelf e spostandosi, parte il trascinamento.
        if let Some((Hit::ShelfItem(i), px, py)) = self.pressed
            && left_button
            && (x - px).hypot(y - py) > DRAG_THRESHOLD
        {
            self.pressed = None;
            self.begin_drag(i);
            return;
        }

        // Da linguetta (app a schermo intero) gli avvisi non si vedono: si apre e basta.
        if !self.open
            && !self.mini
            && let Some(alert) = self.alerts.current()
        {
            if alert.is_actionable() {
                // Un avviso che aspetta una risposta resta lì sotto il cursore.
                self.alerts.hold(time::now_ms());
                self.set_hot(self.hit(x, y));
                return;
            }
            self.alerts.dismiss(time::now_ms());
        }
        self.set_open(true);
        self.set_hot(self.hit(x, y));
    }

    fn set_hot(&mut self, hot: Option<Hit>) {
        if hot != self.hot {
            self.hot = hot;
            self.dirty = true;
        }
    }

    fn on_mouse_leave(&mut self) {
        self.hover = false;
        self.pressed = None;
        self.bar_shown = false;
        self.footer_reveal.set_target(0.0);
        // Appena scelta una pagina più bassa, il notch si è accorciato sotto il
        // cursore fermo: per un attimo non si chiude, se il cursore è ancora lì.
        let just_switched = self
            .page_switched_at
            .is_some_and(|t| t.elapsed() < Duration::from_millis(800));
        if self.open && just_switched {
            let from = self.slide_from.unwrap_or(self.page);
            let bottom = open_height(self.page_height(from)).max(self.height.value());
            self.collapse_grace = Some((Instant::now() + Duration::from_secs(10), bottom));
        }
        self.set_hot(None);
        // Piccolo ritardo: uscire per un attimo dal bordo non chiude il notch.
        unsafe { SetTimer(Some(self.hwnd), TIMER_COLLAPSE, COLLAPSE_DELAY_MS, None) };
    }

    /// Il cursore è ancora dove c'era il notch prima di accorciarsi per il cambio pagina?
    fn cursor_in_grace(&mut self) -> bool {
        let Some((until, bottom)) = self.collapse_grace else {
            return false;
        };
        let mut point = POINT::default();
        let inside = Instant::now() < until
            && unsafe { GetCursorPos(&mut point) }.is_ok()
            && unsafe { ScreenToClient(self.hwnd, &mut point) }.as_bool()
            && {
                let k = self.scale();
                let (x, y) = (point.x as f32 / k, point.y as f32 / k);
                x >= open_x() && x <= open_x() + OPEN_W && (0.0..=bottom + 4.0).contains(&y)
            };
        if !inside {
            self.collapse_grace = None;
        }
        inside
    }

    pub(super) fn set_open(&mut self, open: bool) {
        if self.open != open {
            self.open = open;
            if !open {
                self.hot = None;
                self.selection.clear();
                self.volume_mode = false;
                self.footer_reveal.set_target(0.0);
                self.stop_editing(true);
                // Un promemoria arrivato mentre era aperto si vede solo adesso.
                if self.visible && !self.mini {
                    self.alerts.resume(time::now_ms());
                }
            }
            self.retarget();
        }
    }

    pub(super) fn set_page(&mut self, page: Page) {
        let direction = if page.index() >= self.page.index() {
            1.0
        } else {
            -1.0
        };
        self.switch_page(page, direction);
    }

    /// Cambia pagina: la nuova entra dal lato `direction` (+1 da destra).
    fn switch_page(&mut self, page: Page, direction: f32) {
        if page != self.page {
            // Se la pagina attuale stava ancora entrando, esce dalla sua opacità di adesso.
            self.slide_from_alpha = if self.page_fade.is_settled() {
                1.0
            } else {
                crate::anim::smooth(0.15, 1.0, self.page_fade.value())
            };
            self.slide_from = Some(self.page);
            self.slide_dir = direction;
            self.page = page;
            self.page_switched_at = Some(Instant::now());
            self.volume_mode = false;
            self.page_fade.snap(0.0);
            self.page_fade.set_target(1.0);
            self.stop_editing(true);
            self.retarget();
        }
    }

    fn on_wheel(&mut self, wp: WPARAM, lp: LPARAM) {
        if !self.open {
            return;
        }
        let delta = ((wp.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
        // La posizione di WM_MOUSEWHEEL è in coordinate dello schermo.
        let mut point = POINT {
            x: (lp.0 & 0xFFFF) as u16 as i16 as i32,
            y: ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as i32,
        };
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut point);
        }
        let y = point.y as f32 / self.scale();

        // I touchpad mandano tanti piccoli scatti: si accumulano fino a uno "tacco".
        self.wheel += delta;
        let steps = self.wheel / 120;
        if steps == 0 {
            return;
        }
        self.wheel -= steps * 120;
        let direction = -steps.signum();

        let over_list = y > HEADER_H && y < HEADER_H + self.content_height();
        let shelf_rows = self.shelf.len().div_ceil(SHELF_COLS);
        let tasks = self.todo.tasks.len();
        if self.page == Page::Music && self.volume_mode && over_list {
            let level = self.volume.map_or(0.5, |(level, _)| level);
            self.set_volume(level - direction as f32 * 0.05);
        } else if self.page == Page::Shelf && over_list && shelf_rows > SHELF_ROWS {
            self.shelf_scroll = (self.shelf_scroll as i32 + direction)
                .clamp(0, (shelf_rows - SHELF_ROWS) as i32)
                as usize;
        } else if self.page == Page::Todo && over_list && tasks > TODO_ROWS {
            self.todo_scroll =
                (self.todo_scroll as i32 + direction).clamp(0, (tasks - TODO_ROWS) as i32) as usize;
        } else {
            self.set_page(self.page.step(direction));
        }
        self.dirty = true;
    }

    fn on_click(&mut self, hit: Hit, x: f32, ctrl: bool) {
        let now_ms = time::now_ms();
        let clock = sys::clock();
        let durations = Durations::from_settings(&self.settings);
        match hit {
            Hit::Prev => self.control.send(MediaCommand::Previous),
            Hit::Next => self.control.send(MediaCommand::Next),
            Hit::PlayPause => {
                // Risposta immediata; lo stato vero arriva dal player subito dopo.
                if let Some(media) = &mut self.media {
                    media.rebase(sys::now_filetime());
                    media.playing = !media.playing;
                }
                self.control.send(MediaCommand::TogglePlayPause);
            }
            Hit::Seek => {
                let duration = self.media.as_ref().map_or(0, |m| m.duration_ms);
                if duration > 0 {
                    let fraction = self.music.seek_fraction(x) as f64;
                    self.seek_to((duration as f64 * fraction) as u64);
                }
            }
            Hit::Lyrics => self.toggle_field(Field::Lyrics),
            Hit::Volume => {
                self.volume_mode = !self.volume_mode;
                if self.volume_mode {
                    self.volume = volume::get().or(self.volume);
                }
            }
            Hit::VolumeBar => {
                let fraction = self.music.seek_fraction(x);
                self.set_volume(fraction);
            }
            Hit::PrevPage => self.switch_page(self.page.cycle(-1), -1.0),
            Hit::NextPage => self.switch_page(self.page.cycle(1), 1.0),
            Hit::WaterReminders => self.toggle_field(Field::WaterReminders),
            Hit::WaterView => self.water_chart = !self.water_chart,
            Hit::LyricLine(offset) => {
                if let Some(lyrics) = self.synced() {
                    let base = self.line.map_or(-1, |i| i as isize);
                    let target = base + offset as isize;
                    if let Some(line) = usize::try_from(target)
                        .ok()
                        .and_then(|i| lyrics.lines.get(i))
                    {
                        self.seek_to(line.time_ms);
                    }
                }
            }
            Hit::Page(page) => self.set_page(page),
            Hit::Settings => {
                // La finestra prende il focus: la scrittura in corso va chiusa prima.
                self.stop_editing(true);
                self.open_settings();
            }
            Hit::FocusToggle => self.pomodoro.toggle(now_ms, &durations),
            Hit::FocusReset => self.pomodoro.reset(),
            Hit::FocusSkip => {
                self.pomodoro
                    .skip(now_ms, &durations, self.settings.focus_auto_start)
            }
            Hit::FocusPhase(phase) => self.pomodoro.select(phase),
            Hit::Drink => {
                self.water.drink(clock.now, self.settings.water_glass_ml);
                self.water_snooze = None;
                self.alerts.retain(now_ms, |a| a.kind != AlertKind::Water);
                self.save_water();
            }
            Hit::UndoDrink => {
                if self.water.undo_today(&clock) {
                    self.save_water();
                }
            }
            Hit::ShelfItem(i) => {
                let Some(entry) = self.shelf_visible().get(i).cloned() else {
                    return;
                };
                if ctrl {
                    if !self.selection.remove(&entry.id) {
                        self.selection.insert(entry.id);
                    }
                } else if let Some(path) = entry.path() {
                    self.pending_open = Some(path.to_string());
                } else if let Some(text) = entry.text()
                    && dnd::copy_text(self.hwnd, text).is_ok()
                {
                    self.note = Some(("Copiato negli appunti", now_ms + 1_500));
                }
            }
            Hit::ShelfRemove(i) => {
                if let Some(id) = self.shelf_visible().get(i).map(|e| e.id) {
                    self.shelf.remove(&[id]);
                    self.selection.remove(&id);
                    self.after_shelf_change();
                }
            }
            Hit::ShelfAction => {
                if self.selection.is_empty() {
                    self.shelf.clear();
                } else {
                    let ids: Vec<u64> = self.selection.drain().collect();
                    self.shelf.remove(&ids);
                }
                self.after_shelf_change();
            }
            Hit::TaskCheck(i) => {
                if let Some(id) = self.todo_visible().get(i).map(|t| t.id) {
                    self.todo.toggle(id, clock.now);
                    self.alerts.retain(now_ms, |a| {
                        !a.actions
                            .iter()
                            .any(|(_, act)| *act == AlertAction::CompleteTask(id))
                    });
                    self.save_todo();
                }
            }
            Hit::TaskDelete(i) => {
                if let Some(id) = self.todo_visible().get(i).map(|t| t.id) {
                    self.todo.remove(id);
                    self.clamp_todo_scroll();
                    self.save_todo();
                }
            }
            Hit::TaskAdd => {
                if self.editing.is_none() {
                    self.start_editing();
                }
            }
            Hit::TodoAction => {
                self.todo.clear_done();
                self.clamp_todo_scroll();
                self.save_todo();
            }
            Hit::AlertAction(i) => {
                if let Some(action) = self
                    .alerts
                    .current()
                    .and_then(|a| a.actions.get(i))
                    .map(|(_, a)| *a)
                {
                    self.perform(action);
                }
            }
            Hit::AlertBody => self.alerts.dismiss(now_ms),
        }
        self.retarget();
    }

    fn seek_to(&mut self, ms: u64) {
        if let Some(media) = &mut self.media {
            media.position_ms = ms;
            media.updated_at = sys::now_filetime();
        }
        self.control.send(MediaCommand::Seek(ms));
        self.update_line();
    }

    fn set_volume(&mut self, level: f32) {
        let level = level.clamp(0.0, 1.0);
        if volume::set(level).is_ok() {
            self.volume = Some((level, false));
        }
        self.dirty = true;
    }

    pub(super) fn clamp_todo_scroll(&mut self) {
        self.todo_scroll = self
            .todo_scroll
            .min(self.todo.tasks.len().saturating_sub(TODO_ROWS));
    }

    pub(super) fn toggle_field(&mut self, field: Field) {
        field.step(&mut self.settings, 1, 0);
        self.apply_setting(field);
    }

    // ---- Campo di testo per le attività -------------------------------------------------

    fn start_editing(&mut self) {
        self.editing = Some(TextEdit::new(120));
        self.edit_scroll.set(0.0);
        // Per scrivere serve il focus della tastiera (il notch di solito non lo prende).
        unsafe {
            let previous = GetForegroundWindow();
            self.previous_focus =
                (!previous.is_invalid() && previous != self.hwnd).then_some(previous);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
        }
        self.dirty = true;
    }

    pub(super) fn stop_editing(&mut self, commit: bool) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        if commit && !edit.is_empty() {
            let clock = sys::clock();
            let (title, due) = parse_entry(edit.text(), &clock);
            self.todo.add(title, due, clock.now);
            self.save_todo();
        }
        // Il focus torna all'app di prima, se nel frattempo non l'ha preso altro.
        if let Some(previous) = self.previous_focus.take() {
            unsafe {
                if GetForegroundWindow() == self.hwnd && IsWindow(Some(previous)).as_bool() {
                    let _ = SetForegroundWindow(previous);
                }
            }
        }
        if !self.hover {
            unsafe { SetTimer(Some(self.hwnd), TIMER_COLLAPSE, COLLAPSE_DELAY_MS, None) };
        }
        self.retarget();
    }

    fn on_key(&mut self, key: VIRTUAL_KEY) {
        let ctrl = ctrl_down();
        match key {
            VK_RETURN => {
                self.stop_editing(true);
                return;
            }
            VK_ESCAPE => {
                self.stop_editing(false);
                return;
            }
            _ => {}
        }
        let pasted = (ctrl && key.0 == u16::from(b'V'))
            .then(|| dnd::paste_text(self.hwnd))
            .flatten();
        let Some(edit) = &mut self.editing else {
            return;
        };
        match key {
            VK_BACK => edit.backspace(ctrl),
            VK_DELETE => edit.delete(ctrl),
            VK_LEFT => edit.left(ctrl),
            VK_RIGHT => edit.right(ctrl),
            VK_HOME => edit.home(),
            VK_END => edit.end(),
            _ => {
                if let Some(text) = pasted {
                    edit.insert(&text);
                }
            }
        }
        self.dirty = true;
    }

    fn on_char(&mut self, unit: u16) {
        // I caratteri fuori dal piano base (es. emoji) arrivano in due metà.
        let text = match unit {
            0xD800..=0xDBFF => {
                self.high_surrogate = Some(unit);
                return;
            }
            0xDC00..=0xDFFF => match self.high_surrogate.take() {
                Some(high) => String::from_utf16(&[high, unit]).ok(),
                None => None,
            },
            _ => char::from_u32(unit as u32).map(String::from),
        };
        // Invio, Esc, Backspace e Ctrl+lettera producono caratteri di controllo.
        let Some(text) = text.filter(|t| !t.chars().any(char::is_control)) else {
            return;
        };
        if let Some(edit) = &mut self.editing {
            edit.insert(&text);
            self.dirty = true;
        }
    }

    // ---- Trascinamento -------------------------------------------------------------------

    pub(super) fn on_drag_enter(&mut self) -> bool {
        if self.dragging_out {
            return false; // si sta trascinando fuori dallo Shelf stesso
        }
        self.drop_hover = true;
        self.kill_timer(TIMER_COLLAPSE);
        self.alerts.dismiss(time::now_ms());
        self.set_page(Page::Shelf);
        self.set_open(true);
        self.retarget();
        true
    }

    pub(super) fn on_drag_leave(&mut self) {
        self.drop_hover = false;
        unsafe { SetTimer(Some(self.hwnd), TIMER_COLLAPSE, COLLAPSE_DELAY_MS, None) };
        self.dirty = true;
    }

    pub(super) fn on_drop(&mut self, payload: Option<Payload>) -> bool {
        self.drop_hover = false;
        let now = sys::clock().now;
        let accepted = match payload {
            Some(Payload::Files(files)) => self.shelf.add_files(files, now) > 0,
            Some(Payload::Text(text)) => self.shelf.add_text(text, now),
            None => false,
        };
        if accepted {
            self.shelf_scroll = 0;
            self.after_shelf_change();
        }
        // Resta aperto un attimo per far vedere cosa è stato aggiunto.
        unsafe { SetTimer(Some(self.hwnd), TIMER_COLLAPSE, 1_200, None) };
        self.retarget();
        accepted
    }

    fn begin_drag(&mut self, i: usize) {
        let Some(entry) = self.shelf_visible().get(i).cloned() else {
            return;
        };
        let ids: Vec<u64> = if self.selection.contains(&entry.id) {
            self.selection.iter().copied().collect()
        } else {
            vec![entry.id]
        };
        let files: Vec<String> = ids
            .iter()
            .filter_map(|id| self.shelf.get(*id))
            .filter_map(|e| e.path().map(String::from))
            .collect();
        let text = if files.is_empty() {
            entry.text().map(String::from)
        } else {
            None
        };
        self.pending_drag = Some(Drag { files, text });
    }

    pub(super) fn drag_finished(&mut self) {
        self.dragging_out = false;
        // Durante il trascinamento non arrivano messaggi del mouse: si ricomincia da capo.
        self.hover = false;
        unsafe { SetTimer(Some(self.hwnd), TIMER_COLLAPSE, COLLAPSE_DELAY_MS, None) };
        self.dirty = true;
    }

    // ---- Tray ---------------------------------------------------------------------------

    pub(super) fn menu_state(&self) -> tray::MenuState {
        tray::MenuState {
            lyrics: self.settings.lyrics,
            autostart: self.settings.autostart,
        }
    }

    pub(super) fn on_menu(&mut self, chosen: u32) {
        match chosen {
            tray::ID_SETTINGS => self.open_settings(),
            tray::ID_LYRICS => self.toggle_field(Field::Lyrics),
            tray::ID_AUTOSTART => self.toggle_field(Field::Autostart),
            tray::ID_EXIT => unsafe { PostQuitMessage(0) },
            _ => {}
        }
    }
}

fn ctrl_down() -> bool {
    unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 }
}
