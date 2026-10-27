//! Dati: risultati dei thread di lavoro, testi sincronizzati, avvisi,
//! promemoria (Focus, acqua, to-do), salvataggi e impostazioni.

use windows::Win32::UI::WindowsAndMessaging::SetTimer;

use super::*;
use crate::features::alerts::{Alert, AlertAction};
use crate::features::pomodoro::{Durations, Phase};
use crate::features::water::{self, WaterPlan, fmt_liters};
use crate::lyrics::Lyrics;
use crate::settings::Field;
use crate::win::keyboard::KeyEvent;
use crate::win::monitors::SystemEvent;
use crate::win::power::PowerEvent;

/// Anticipo con cui mostrare una riga del testo, per compensare l'animazione.
const LYRICS_LEAD_MS: u64 = 120;
/// Al cambio brano titolo e durata arrivano in momenti diversi: si aspetta un
/// attimo prima di cercare il testo, altrimenti la durata sarebbe quella vecchia.
const LYRICS_SETTLE_MS: u32 = 700;

// Glifi di Segoe Fluent Icons / MDL2 Assets per gli avvisi.
const GLYPH_MUTE: char = '\u{E74F}';
const GLYPH_VOLUME: [char; 4] = ['\u{E992}', '\u{E993}', '\u{E994}', '\u{E995}'];
const GLYPH_BRIGHTNESS: char = '\u{E706}';
const GLYPH_WIFI: char = '\u{E701}';
const GLYPH_ETHERNET: char = '\u{E968}';
const GLYPH_OFFLINE: char = '\u{F384}';
const GLYPH_HEADPHONE: char = '\u{E7F6}';
const GLYPH_KEYBOARD: char = '\u{E765}';
const GLYPH_GLOBE: char = '\u{E774}';
const GLYPH_STOPWATCH: char = '\u{E916}';
const GLYPH_DROP: char = '\u{EB42}';
const GLYPH_REMINDER: char = '\u{EB50}';

impl App {
    /// Legge i risultati dei thread di lavoro. È economico: senza novità non fa nulla.
    pub(super) fn sync(&mut self) {
        let mut changed = false;

        if let Some(latest) = self.media_inbox.drain().pop() {
            let old_key = self.media.as_ref().map(MediaSnapshot::track_key);
            let new_key = latest.as_ref().map(MediaSnapshot::track_key);
            if old_key != new_key {
                self.line = None;
                self.scroll.snap(0.0);
                self.track_changed_at = Some(Instant::now());
                unsafe { SetTimer(Some(self.hwnd), TIMER_LYRICS, LYRICS_SETTLE_MS, None) };
                // Gli errori di rete si riprovano al prossimo ascolto.
                self.lyrics
                    .retain(|_, state| !matches!(state, LyricsState::Failed));
            }
            self.media = latest;
            changed = true;
        }
        for (key, state) in self.lyrics_inbox.drain() {
            match state {
                Some(state) => self.lyrics.insert(key, state),
                None => self.lyrics.remove(&key),
            };
            changed = true;
        }
        for (id, art) in self.thumb_inbox.drain() {
            if let Some(art) = art {
                self.thumbs.insert(id, Arc::new(art));
            }
            changed = true;
        }
        for event in self.system_inbox.drain() {
            self.system_alert(event);
            changed = true;
        }
        if let Some(stats) = self.stats_inbox.drain().pop() {
            self.record_stats(stats);
            changed = true;
        }

        if changed {
            self.request_lyrics();
            self.update_line();
            self.retarget();
        }
    }

    fn record_stats(&mut self, stats: Stats) {
        let samples = [
            (HIST_CPU, stats.cpu),
            (HIST_GPU, stats.gpu),
            (HIST_DOWN, stats.download.map(|v| v as f32)),
            (HIST_UP, stats.upload.map(|v| v as f32)),
        ];
        for (index, value) in samples {
            if let Some(value) = value {
                self.history[index].push(value);
            }
        }
        self.stats = stats;
    }

    /// Il campionatore lavora solo con la pagina Sistema aperta.
    pub(super) fn update_sysmon(&mut self) {
        let wanted = self.open && self.page == Page::System && self.visible;
        if wanted != self.sysmon_running {
            self.sysmon_running = wanted;
            if wanted {
                self.history.iter_mut().for_each(History::clear);
                self.stats = Stats::default();
            }
            let _ = self.sysmon.send(if wanted {
                Control::Start
            } else {
                Control::Stop
            });
        }
    }

    // ---- Testi sincronizzati --------------------------------------------------------------

    pub(super) fn request_lyrics(&mut self) {
        let settling = self
            .track_changed_at
            .is_some_and(|t| t.elapsed().as_millis() < LYRICS_SETTLE_MS as u128);
        if !self.settings.lyrics || settling {
            return; // TIMER_LYRICS richiamerà questa funzione
        }
        let Some(media) = self.media.as_ref().filter(|m| m.has_track()) else {
            return;
        };
        let key = media.track_key();
        if self.lyrics.contains_key(&key) {
            return;
        }
        if self.lyrics.len() > 256 {
            self.lyrics.clear();
        }
        let query = Query {
            title: media.title.clone(),
            artist: media.artist.clone(),
            album: media.album.clone(),
            duration_s: (media.duration_ms > 0).then(|| media.duration_ms as f64 / 1000.0),
        };
        self.lyrics.insert(key, LyricsState::Loading);
        let _ = self.lyrics_requests.send((key, query));
    }

    pub(in crate::win) fn current_lyrics(&self) -> Option<&LyricsState> {
        let media = self.media.as_ref()?;
        self.lyrics.get(&media.track_key())
    }

    /// Testo sincronizzato del brano corrente, se i testi sono attivi.
    pub(super) fn synced(&self) -> Option<Arc<Lyrics>> {
        match self.current_lyrics()? {
            LyricsState::Synced(lyrics) if self.settings.lyrics => Some(lyrics.clone()),
            _ => None,
        }
    }

    /// Quali righe del testo (precedente, attuale, successiva) sono disegnate e cliccabili.
    pub(super) fn lyric_targets(&self) -> [bool; 3] {
        let Some(lyrics) = self.synced() else {
            return [false; 3];
        };
        let base = self.line.map_or(-1, |i| i as isize);
        std::array::from_fn(|k| {
            let target = base + k as isize - 1;
            target >= 0 && (target as usize) < lyrics.lines.len()
        })
    }

    /// Ricalcola la riga corrente; se cambia, avvia scorrimento e ridimensionamento.
    pub(super) fn update_line(&mut self) {
        let line = match (self.synced(), &self.media) {
            (Some(lyrics), Some(media)) => {
                lyrics.index_at(media.position_at(sys::now_filetime()) + LYRICS_LEAD_MS)
            }
            _ => None,
        };
        if line == self.line {
            return;
        }
        let one_step_forward = match (self.line, line) {
            (Some(a), Some(b)) => b == a + 1,
            (None, Some(b)) => b == 0,
            _ => false,
        };
        self.line = line;
        if one_step_forward && self.open {
            self.scroll.snap(1.0);
            self.scroll.set_target(0.0);
        } else {
            self.scroll.snap(0.0);
        }
        self.retarget();
    }

    /// Riga mostrata nella pillola chiusa, mentre la musica suona.
    pub(in crate::win) fn pill_line(&self) -> Option<&str> {
        let media = self.media.as_ref()?;
        if !self.settings.lyrics || !media.playing {
            return None;
        }
        let LyricsState::Synced(lyrics) = self.current_lyrics()? else {
            return None;
        };
        let text = lyrics.lines.get(self.line?)?.text.as_str();
        (!text.trim().is_empty()).then_some(text)
    }

    /// Misura la riga mostrata nella pillola, solo quando cambia (brano o riga).
    pub(super) fn measure_pill_if_needed(&mut self) {
        let key = self
            .media
            .as_ref()
            .map(MediaSnapshot::track_key)
            .zip(self.line);
        if key.is_none() || key == self.pill_measured {
            return;
        }
        if let Some(width) = self.pill_line().map(|text| self.measure_pill(text)) {
            self.pill_text_w = width;
            self.pill_measured = key;
        }
    }

    fn measure_pill(&self, text: &str) -> f32 {
        match &self.gfx {
            Some(gfx) => gfx.measure(text, &gfx.fonts.pill),
            None => text.chars().count() as f32 * 7.5,
        }
    }

    /// Larghezza di un testo con il carattere dell'orologio del notch chiuso.
    pub(super) fn measure_clock(&self, text: &str) -> f32 {
        match &self.gfx {
            Some(gfx) => gfx.measure(text, &gfx.fonts.clock),
            None => text.chars().count() as f32 * 9.0,
        }
    }

    // ---- Promemoria e scadenze -----------------------------------------------------------

    /// Controlla timer e promemoria. Viene chiamata a ogni risveglio programmato.
    pub(super) fn tick_features(&mut self) {
        let now_ms = time::now_ms();
        let clock = sys::clock();

        // Giorno nuovo: via le attività fatte ieri e lo storico vecchio dell'acqua.
        if clock.today() != self.today {
            self.today = clock.today();
            self.todo.prune(&clock);
            self.clamp_todo_scroll();
            self.water.prune(&clock, 30);
            self.save_todo();
            self.save_water();
        }

        self.battery = power::battery();

        let durations = Durations::from_settings(&self.settings);
        if let Some(finished) =
            self.pomodoro
                .tick(now_ms, &durations, self.settings.focus_auto_start)
        {
            self.focus_alert(finished);
        }

        let due = self.todo.due_now(clock.now);
        for id in due {
            self.todo.mark_notified(id);
            if let Some(task) = self.todo.get(id) {
                let alert =
                    Alert::new(AlertKind::Todo, GLYPH_REMINDER, "È ora", task.title.clone())
                        .with_action("Fatto", AlertAction::CompleteTask(id))
                        .with_action("Tra 10 minuti", AlertAction::SnoozeTask(id));
                self.alerts.push(alert, now_ms);
                sys::chime();
            }
            self.save_todo();
        }

        let plan = WaterPlan::from_settings(&self.settings);
        if let Some(due) = water::next_reminder(&self.water, &plan, &clock, self.water_snooze)
            && due <= clock.now
        {
            let total = self.water.total(&clock, clock.today());
            let detail = format!("{} di {} oggi", fmt_liters(total), fmt_liters(plan.goal_ml));
            let alert = Alert::new(
                AlertKind::Water,
                GLYPH_DROP,
                "Bevi un bicchiere d'acqua",
                detail,
            )
            .with_action(format!("Bevuto +{} ml", plan.glass_ml), AlertAction::Drink)
            .with_action("Più tardi", AlertAction::SnoozeWater);
            // Un solo promemoria dell'acqua alla volta, con i totali aggiornati.
            self.alerts.retain(now_ms, |a| a.kind != AlertKind::Water);
            self.alerts.push(alert, now_ms);
            sys::chime();
            // Senza risposta il promemoria torna dopo un intervallo.
            self.water_snooze = Some(clock.now + plan.interval_min as i64 * 60);
        }

        if self.note.is_some_and(|(_, until)| now_ms >= until) {
            self.note = None;
        }
        // Gli avvisi si vedono solo a notch chiuso e visibile. A schermo intero o
        // con il notch aperto un promemoria con pulsanti aspetta di essere visto,
        // poi ha tutto il suo tempo.
        if self.visible && !self.open && !self.mini {
            self.alerts.resume(now_ms);
        } else {
            self.alerts.freeze();
        }
        self.alerts.tick(now_ms);
        self.retarget();
    }

    fn focus_alert(&mut self, finished: Phase) {
        let next = self.pomodoro.phase();
        let started = self.pomodoro.is_running();
        let (title, detail) = match finished {
            Phase::Focus => ("Concentrazione finita", "È ora di una pausa"),
            _ => ("Pausa finita", "Si torna a concentrarsi"),
        };
        let mut alert = Alert::new(AlertKind::Focus, GLYPH_STOPWATCH, title, detail);
        if !started {
            alert = alert.with_action(
                format!("Avvia {}", next.label().to_lowercase()),
                AlertAction::StartFocusPhase,
            );
        }
        self.alerts.push(alert, time::now_ms());
        sys::chime();
    }

    /// Esegue il pulsante di un avviso.
    pub(super) fn perform(&mut self, action: AlertAction) {
        let now_ms = time::now_ms();
        let clock = sys::clock();
        // Prima si chiude l'avviso del pulsante, poi si agisce (che può toglierne altri).
        self.alerts.dismiss(now_ms);
        match action {
            AlertAction::CompleteTask(id) => {
                self.todo.complete(id, clock.now);
                self.save_todo();
            }
            AlertAction::SnoozeTask(id) => {
                self.todo.snooze(id, clock.now + 10 * 60);
                self.save_todo();
            }
            AlertAction::Drink => {
                self.water.drink(clock.now, self.settings.water_glass_ml);
                self.water_snooze = None;
                self.alerts.retain(now_ms, |a| a.kind != AlertKind::Water);
                self.save_water();
            }
            AlertAction::SnoozeWater => self.water_snooze = Some(clock.now + 15 * 60),
            AlertAction::StartFocusPhase => {
                if !self.pomodoro.is_running() {
                    self.pomodoro
                        .toggle(now_ms, &Durations::from_settings(&self.settings));
                }
            }
        }
        self.retarget();
    }

    // ---- Avvisi di sistema ----------------------------------------------------------------

    fn system_alert(&mut self, event: SystemEvent) {
        if let SystemEvent::Downloading(on) = event {
            self.set_downloading(on);
            return;
        }
        if let SystemEvent::Volume { level, muted } = event {
            self.volume = Some((level, muted));
            // Col notch aperto il volume si vede già nella pagina Musica.
            if self.open {
                self.dirty = true;
                return;
            }
        }
        // La luminosità cambia anche da sola (sensore di luce, schermo che si
        // attenua, profilo batteria): si avvisa solo se l'utente sta usando il PC.
        let recent_power_change = self
            .last_power_change
            .is_some_and(|t| t.elapsed().as_secs() < 3);
        let brightness_by_user = sys::idle_ms() < 10_000 && !recent_power_change;
        let s = &self.settings;
        let alert = match event {
            SystemEvent::Volume { level, muted } if s.hud_volume => {
                let glyph = if muted {
                    GLYPH_MUTE
                } else {
                    GLYPH_VOLUME[((level * 3.0).ceil() as usize).min(3)]
                };
                let title = if muted { "Muto" } else { "Volume" };
                Alert::new(AlertKind::Volume, glyph, title, "").with_level(if muted {
                    0.0
                } else {
                    level
                })
            }
            SystemEvent::Brightness(level) if s.hud_brightness && brightness_by_user => {
                Alert::new(AlertKind::Brightness, GLYPH_BRIGHTNESS, "Luminosità", "")
                    .with_level(level as f32 / 100.0)
            }
            SystemEvent::Network {
                connected: true,
                wifi,
                name,
            } if s.hud_network => {
                let title = if wifi {
                    "Wi-Fi connesso"
                } else {
                    "Rete connessa"
                };
                Alert::new(
                    AlertKind::Network,
                    if wifi { GLYPH_WIFI } else { GLYPH_ETHERNET },
                    title,
                    name,
                )
            }
            SystemEvent::Network {
                connected: false, ..
            } if s.hud_network => Alert::new(
                AlertKind::Network,
                GLYPH_OFFLINE,
                "Connessione persa",
                "Nessun accesso alla rete",
            ),
            SystemEvent::Bluetooth {
                name,
                connected,
                battery,
            } if s.hud_bluetooth => {
                let detail = match (connected, battery) {
                    (true, Some(level)) => format!("Connesso · batteria {level}%"),
                    (true, None) => "Connesso".to_string(),
                    (false, _) => "Disconnesso".to_string(),
                };
                Alert::new(AlertKind::Bluetooth, GLYPH_HEADPHONE, name, detail)
            }
            _ => return,
        };
        self.alerts.push(alert, time::now_ms());
    }

    pub(super) fn power_alert(&mut self, event: PowerEvent) {
        if matches!(event, PowerEvent::Plugged(_) | PowerEvent::Unplugged(_)) {
            self.last_power_change = Some(Instant::now());
        }
        if !self.settings.hud_power {
            return;
        }
        let battery_glyph = |percent: u32, charging: bool| {
            let step = (percent.min(100) / 10) as usize;
            let normal = [
                '\u{E850}', '\u{E851}', '\u{E852}', '\u{E853}', '\u{E854}', '\u{E855}', '\u{E856}',
                '\u{E857}', '\u{E858}', '\u{E859}', '\u{E83F}',
            ];
            let plugged = [
                '\u{E85A}', '\u{E85B}', '\u{E85C}', '\u{E85D}', '\u{E85E}', '\u{E85F}', '\u{E860}',
                '\u{E861}', '\u{E862}', '\u{E83E}', '\u{EA93}',
            ];
            if charging {
                plugged[step]
            } else {
                normal[step]
            }
        };
        let alert = match event {
            PowerEvent::Plugged(p) => {
                Alert::new(AlertKind::Power, battery_glyph(p, true), "In carica", "")
                    .with_level(p as f32 / 100.0)
            }
            PowerEvent::Unplugged(p) => {
                Alert::new(AlertKind::Power, battery_glyph(p, false), "A batteria", "")
                    .with_level(p as f32 / 100.0)
            }
            PowerEvent::Low(p) => Alert::new(
                AlertKind::Power,
                battery_glyph(p, false),
                "Batteria scarica",
                format!("{p}% rimanente · collega il caricatore"),
            ),
        };
        self.alerts.push(alert, time::now_ms());
        self.retarget();
    }

    pub(super) fn keyboard_check(&mut self) {
        if !self.settings.hud_keyboard {
            return;
        }
        // Di solito non è cambiato niente (era solo uno Spazio o uno Shift): niente da ridisegnare.
        let events = self.keyboard.check();
        if events.is_empty() {
            return;
        }
        for event in events {
            let alert = match event {
                KeyEvent::CapsLock(true) => Alert::new(
                    AlertKind::Keyboard,
                    GLYPH_KEYBOARD,
                    "Bloc Maiusc attivo",
                    "Stai scrivendo in maiuscolo",
                ),
                KeyEvent::CapsLock(false) => Alert::new(
                    AlertKind::Keyboard,
                    GLYPH_KEYBOARD,
                    "Bloc Maiusc disattivato",
                    "Lettere minuscole",
                ),
                KeyEvent::Language(name) => Alert::new(
                    AlertKind::Keyboard,
                    GLYPH_GLOBE,
                    name,
                    "Lingua della tastiera",
                ),
            };
            self.alerts.push(alert, time::now_ms());
        }
        self.retarget();
    }

    pub(super) fn apply_keyboard_setting(&mut self) {
        let enabled = self.settings.hud_keyboard;
        self.keyboard.set_enabled(self.hwnd, enabled);
    }

    // ---- Shelf ---------------------------------------------------------------------------

    pub(super) fn after_shelf_change(&mut self) {
        let rows = self.shelf.len().div_ceil(SHELF_COLS);
        self.shelf_scroll = self.shelf_scroll.min(rows.saturating_sub(SHELF_ROWS));
        self.selection.retain(|id| self.shelf.get(*id).is_some());
        self.thumbs.retain(|id, _| self.shelf.get(*id).is_some());
        self.thumb_requested
            .retain(|id| self.shelf.get(*id).is_some());
        self.request_thumbnails();
        self.save_shelf();
    }

    /// Chiede le anteprime dei file che ancora non ce l'hanno.
    pub(super) fn request_thumbnails(&mut self) {
        for entry in &self.shelf.entries {
            if let Some(path) = entry.path()
                && self.thumb_requested.insert(entry.id)
            {
                let _ = self.thumb_requests.send((entry.id, path.to_string()));
            }
        }
    }

    // ---- Salvataggi ----------------------------------------------------------------------

    pub(super) fn save_todo(&self) {
        if let Some(dir) = &self.data_dir {
            let _ = store::save(dir, "todo.json", &self.todo);
        }
    }

    pub(super) fn save_water(&self) {
        if let Some(dir) = &self.data_dir {
            let _ = store::save(dir, "water.json", &self.water);
        }
    }

    fn save_shelf(&self) {
        if let Some(dir) = &self.data_dir {
            let _ = store::save(dir, "shelf.json", &self.shelf);
        }
    }

    // ---- Impostazioni --------------------------------------------------------------------

    /// Salva un'impostazione appena cambiata e ne applica subito gli effetti.
    pub(super) fn apply_setting(&mut self, field: Field) {
        settings_store::save(&self.settings, field);
        match field {
            Field::Monitor | Field::NotchScale => {
                self.place();
                self.refresh_fullscreen();
            }
            Field::Fullscreen | Field::ShrinkMaximized => self.refresh_fullscreen(),
            Field::Downloads => {
                self.refresh_fullscreen();
                self.update_spinner();
            }
            Field::Lyrics => {
                self.line = None;
                self.request_lyrics();
                self.update_line();
            }
            Field::HudKeyboard => self.apply_keyboard_setting(),
            Field::WaterReminders | Field::WaterInterval | Field::WaterFrom | Field::WaterTo => {
                self.water_snooze = None;
            }
            _ => {}
        }
        // Anche dalla tray o dal pulsante dei testi: la finestra Impostazioni mostra il valore nuovo.
        if let Some(window) = &mut self.settings_window {
            window.invalidate();
        }
        self.retarget();
        self.schedule_tick();
    }

    // ---- Risvegli ------------------------------------------------------------------------

    /// Prossimo risveglio: il minuto dell'orologio, la barra di avanzamento (solo
    /// se visibile), la prossima riga del testo, il secondo del Pomodoro, la
    /// scadenza di un avviso o di un promemoria. Nient'altro.
    pub(super) fn schedule_tick(&self) {
        let now_ms = time::now_ms();
        let clock = sys::clock();
        let mut wait = sys::ms_to_next_minute() as u64 + 20;
        let until = |wait: &mut u64, deadline_ms: u64| {
            *wait = (*wait).min(deadline_ms.saturating_sub(now_ms).max(15))
        };

        if let Some(media) = self.media.as_ref().filter(|m| m.playing && m.has_track()) {
            if self.open && self.page == Page::Music {
                wait = wait.min(500);
            }
            if let Some(lyrics) = self.synced() {
                let pos = media.position_at(sys::now_filetime()) + LYRICS_LEAD_MS;
                if let Some(next) = lyrics.next_time_after(pos) {
                    let ms = ((next - pos) as f64 / media.rate.max(0.1)).ceil() as u64 + 5;
                    wait = wait.min(ms.max(15));
                }
            }
        }
        if let Some(end) = self.pomodoro.deadline() {
            // Il conto alla rovescia è visibile: si aggiorna allo scatto di ogni secondo.
            let remaining = end.saturating_sub(now_ms);
            wait = wait.min(remaining % 1000 + 5);
            until(&mut wait, end);
        }
        if let Some(deadline) = self.alerts.deadline() {
            until(&mut wait, deadline);
        }
        if let Some((_, deadline)) = self.note {
            until(&mut wait, deadline);
        }
        if let Some(due) = self.todo.next_due() {
            until(&mut wait, due.max(0) as u64 * 1000);
        }
        let plan = WaterPlan::from_settings(&self.settings);
        if let Some(due) = water::next_reminder(&self.water, &plan, &clock, self.water_snooze) {
            until(&mut wait, due.max(0) as u64 * 1000);
        }
        unsafe {
            SetTimer(
                Some(self.hwnd),
                TIMER_TICK,
                wait.min(u32::MAX as u64) as u32,
                None,
            )
        };
    }
}
