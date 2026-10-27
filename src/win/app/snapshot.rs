//! Anteprime: `skylight.exe --snapshot <cartella>` disegna il notch in vari stati
//! (con dati di esempio) e salva ogni immagine come `<nome>.bgra`
//! (larghezza e altezza in u32 little-endian, poi i pixel BGRA premoltiplicati).
//! Serve a controllare la grafica anche dove non c'è uno schermo vero, ad esempio
//! con Wine; i dati dell'utente non vengono né letti né salvati.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::Win32::Graphics::DirectWrite::IDWriteFactory;
use windows::core::{Result, w};

use super::*;
use crate::features::alerts::{Alert, AlertAction};
use crate::features::pomodoro::Durations;
use crate::features::sysinfo::{PowerSource, TempSource};
use crate::layout::settings::{SETTINGS_H, SETTINGS_W, settings_layout};
use crate::win::gfx::{Families, pick_family};

/// Frame al secondo delle sequenze animate.
const FPS: f32 = 60.0;

impl App {
    pub(super) fn snapshot(&mut self, dir: &Path) -> Result<()> {
        let _ = std::fs::create_dir_all(dir);
        // Niente salvataggi: i dati di esempio non devono finire tra quelli veri.
        self.data_dir = None;
        self.dpi = 192;
        self.visible = true;
        self.todo = TodoList::default();
        self.shelf = Shelf::default();
        self.water = WaterLog::default();
        self.alerts = Alerts::default();
        self.media = None;
        self.settings.lyrics = false;

        let (w, h) = self.canvas_px();
        let mut gfx = Gfx::offscreen(w, h, 96.0 * self.scale(), preview_families)?;
        // Wine non disegna gli effetti di Direct2D: le anteprime sono senza sfocatura.
        gfx.blur = None;
        self.battery = Some(Battery {
            percent: 78,
            charging: false,
        });
        self.volume = Some((0.62, false));

        // ---- Notch chiuso ----
        self.shot(&mut gfx, dir, "closed-idle", |_| {})?;
        self.shot(&mut gfx, dir, "closed-idle-date", |app| {
            app.settings.closed_date = true
        })?;
        self.settings.closed_date = false;
        self.shot(&mut gfx, dir, "closed-mini", |app| app.mini = true)?;
        self.mini = false;
        // Download in corso, con l'arco fermo a un punto preciso del giro.
        self.reduce_motion = false;
        let spinning_since = Instant::now() - Duration::from_secs_f32(0.3 * SPIN_PERIOD_S);
        self.shot(&mut gfx, dir, "closed-download", |app| {
            app.downloading = true;
            app.download_since = spinning_since;
        })?;
        self.shot(&mut gfx, dir, "closed-download-music", |app| {
            app.media = Some(sample_track())
        })?;
        self.media = None;
        self.downloading = false;
        self.shot(&mut gfx, dir, "closed-music", |app| {
            app.media = Some(sample_track())
        })?;
        self.shot(&mut gfx, dir, "alert-volume", |app| {
            app.media = None;
            app.alerts.push(
                Alert::new(AlertKind::Volume, '\u{E995}', "Volume", "").with_level(0.62),
                time::now_ms(),
            );
        })?;
        self.shot(&mut gfx, dir, "alert-todo", |app| {
            app.alerts = Alerts::default();
            app.alerts.push(
                Alert::new(AlertKind::Todo, '\u{EB50}', "È ora", "Chiamare il dentista")
                    .with_action("Fatto", AlertAction::CompleteTask(1))
                    .with_action("Tra 10 minuti", AlertAction::SnoozeTask(1)),
                time::now_ms(),
            );
        })?;
        self.alerts = Alerts::default();

        // ---- Pagine ----
        let now = sys::clock().now;
        self.open = true;
        self.shot(&mut gfx, dir, "page-music-empty", |app| {
            app.page = Page::Music
        })?;
        self.shot(&mut gfx, dir, "page-music", |app| {
            app.media = Some(sample_track())
        })?;
        self.shot(&mut gfx, dir, "page-music-volume", |app| {
            app.volume_mode = true
        })?;
        self.volume_mode = false;
        self.shot(&mut gfx, dir, "page-music-download", |app| {
            app.downloading = true;
            app.download_since = spinning_since;
        })?;
        self.downloading = false;
        self.shot(&mut gfx, dir, "page-focus", |app| app.page = Page::Focus)?;
        self.shot(&mut gfx, dir, "page-focus-running", |app| {
            let d = Durations::from_settings(&app.settings);
            app.pomodoro.toggle(time::now_ms() - 9 * 60_000, &d);
        })?;
        self.pomodoro = Pomodoro::default();
        self.shot(&mut gfx, dir, "page-water", |app| {
            app.page = Page::Water;
            for k in 1..=4 {
                app.water.drink(now - k * 3_000, 250);
            }
        })?;
        self.shot(&mut gfx, dir, "page-water-chart", |app| {
            app.water_chart = true
        })?;
        self.shot(&mut gfx, dir, "page-shelf-empty", |app| {
            app.page = Page::Shelf
        })?;
        self.shot(&mut gfx, dir, "page-shelf", |app| {
            app.shelf.add_files(
                vec![
                    "C:\\Documenti\\Relazione finale.pdf".into(),
                    "C:\\Documenti\\Foto vacanze".into(),
                    "C:\\Documenti\\budget.xlsx".into(),
                ],
                now,
            );
            app.shelf
                .add_text("Indirizzo: via Roma 12, Milano".into(), now);
        })?;
        self.shot(&mut gfx, dir, "page-todo-empty", |app| {
            app.page = Page::Todo
        })?;
        self.shot(&mut gfx, dir, "page-todo", |app| {
            let clock = sys::clock();
            app.todo
                .add("Chiamare il dentista".into(), Some(now + 3_600), now);
            app.todo.add(
                "Palestra".into(),
                Some(clock.at(clock.today() + 1, 8 * 60)),
                now,
            );
            app.todo.add("Comprare il latte".into(), None, now);
            let done = app.todo.add("Rispondere alle email".into(), None, now);
            app.todo.complete(done, now);
        })?;
        self.shot(&mut gfx, dir, "page-system", |app| {
            app.page = Page::System;
            // Senza il campionatore vero: i valori sono quelli di esempio.
            app.sysmon_running = true;
            app.stats = sample_stats();
            for i in 0..40 {
                let t = i as f32 / 6.0;
                app.history[HIST_CPU].push(0.25 + 0.15 * t.sin());
                app.history[HIST_GPU].push(0.4 + 0.2 * (t * 0.7).cos());
                app.history[HIST_DOWN].push(2.0e6 * (1.2 + (t * 1.3).sin()));
                app.history[HIST_UP].push(3.0e5 * (1.1 + (t * 0.9).cos()));
            }
        })?;
        self.shot(&mut gfx, dir, "page-footer-bar", |app| {
            app.page = Page::Focus;
            app.footer_reveal.set_target(1.0);
            app.hot = Some(Hit::Page(Page::Water));
        })?;
        self.hot = None;
        self.footer_reveal.snap(0.0);

        // ---- Animazioni ----
        self.open = false;
        self.page = Page::Focus;
        self.media = None;
        self.retarget();
        self.settle();
        self.film(&mut gfx, dir, "anim-open", 36, |app| app.open = true)?;
        self.film(&mut gfx, dir, "anim-close", 30, |app| app.open = false)?;
        self.open = true;
        self.retarget();
        self.settle();
        self.film(&mut gfx, dir, "anim-page", 24, |app| {
            app.set_page(Page::Water)
        })?;
        self.film(&mut gfx, dir, "anim-footer", 16, |app| {
            app.footer_reveal.set_target(1.0)
        })?;
        self.footer_reveal.snap(0.0);
        self.open = false;
        self.retarget();
        self.settle();
        self.film(&mut gfx, dir, "anim-alert", 30, |app| {
            app.alerts.push(
                Alert::new(AlertKind::Volume, '\u{E995}', "Volume", "").with_level(0.62),
                time::now_ms(),
            );
        })?;
        self.alerts = Alerts::default();
        self.retarget();
        self.settle();
        self.film(&mut gfx, dir, "anim-mini", 24, |app| app.mini = true)?;
        self.mini = false;

        // ---- Finestra Impostazioni ----
        let k = self.scale();
        let (w, h) = ((SETTINGS_W * k) as u32, (SETTINGS_H * k) as u32);
        let mut gfx = Gfx::offscreen(w, h, 96.0 * k, preview_families)?;
        let layout = settings_layout();
        for (name, scroll) in [("settings-top", 0.0), ("settings-bottom", layout.content_h)] {
            let view = render::settings::View {
                layout: &layout,
                scroll: scroll.min((layout.content_h - SETTINGS_H).max(0.0)),
                hot: None,
                settings: &self.settings,
                monitors: 2,
                view_h: SETTINGS_H,
            };
            render::settings::draw(&mut gfx, &view)?;
            save_pixels(&gfx, dir, name)?;
        }
        Ok(())
    }

    /// Applica `setup`, porta le animazioni alla fine e salva un'immagine.
    fn shot(
        &mut self,
        gfx: &mut Gfx,
        dir: &Path,
        name: &str,
        setup: impl FnOnce(&mut App),
    ) -> Result<()> {
        setup(self);
        self.measure_pill_with(gfx);
        self.retarget();
        self.settle();
        self.save(gfx, dir, name)
    }

    /// Applica `change` e salva `frames` immagini dell'animazione che ne segue.
    fn film(
        &mut self,
        gfx: &mut Gfx,
        dir: &Path,
        name: &str,
        frames: usize,
        change: impl FnOnce(&mut App),
    ) -> Result<()> {
        change(self);
        self.retarget();
        for frame in 0..frames {
            self.save(gfx, dir, &format!("{name}-{frame:02}"))?;
            for spring in self.springs() {
                spring.step(1.0 / FPS);
            }
        }
        Ok(())
    }

    fn settle(&mut self) {
        for spring in self.springs() {
            spring.settle();
        }
    }

    fn measure_pill_with(&mut self, gfx: &Gfx) {
        self.pill_text_w = self
            .pill_line()
            .map_or(0.0, |line| gfx.measure(line, &gfx.fonts.pill));
    }

    fn save(&mut self, gfx: &mut Gfx, dir: &Path, name: &str) -> Result<()> {
        render::draw(gfx, self)?;
        save_pixels(gfx, dir, name)
    }
}

fn save_pixels(gfx: &Gfx, dir: &Path, name: &str) -> Result<()> {
    let (w, h, pixels) = gfx.read_pixels()?;
    let mut data = Vec::with_capacity(8 + pixels.len());
    data.extend_from_slice(&w.to_le_bytes());
    data.extend_from_slice(&h.to_le_bytes());
    data.extend_from_slice(&pixels);
    let _ = std::fs::write(dir.join(format!("{name}.bgra")), data);
    Ok(())
}

/// Fuori da Windows mancano i font Segoe: si usano Selawik (metriche di
/// Segoe UI, licenza OFL) e le Fluent System Icons (MIT), con i codici delle
/// icone tradotti.
fn preview_families(dwrite: &IDWriteFactory) -> Families {
    let system = Families::system(dwrite);
    let icons = pick_family(
        dwrite,
        &[
            w!("Segoe Fluent Icons"),
            w!("Segoe MDL2 Assets"),
            w!("FluentSystemIcons-Regular"),
        ],
    );
    let segoe_icons = unsafe { icons.to_string() }.is_ok_and(|name| name.starts_with("Segoe"));
    let text = pick_family(dwrite, &[system.text, w!("Selawik"), system.text]);
    Families {
        text,
        display: pick_family(dwrite, &[system.display, w!("Selawik"), system.display]),
        icons,
        icon_map: (!segoe_icons).then_some(fluent_icon as fn(char) -> char),
    }
}

/// Codici di Segoe Fluent Icons → Fluent System Icons (solo per le anteprime).
fn fluent_icon(c: char) -> char {
    super::preview_icons::map(c)
}

fn sample_track() -> MediaSnapshot {
    let (w, h) = (144u32, 144u32);
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let t = (x + y) as f32 / (w + h) as f32;
            let r = (255.0 * (0.95 - 0.5 * t)) as u8;
            let g = (255.0 * (0.35 + 0.2 * t)) as u8;
            let b = (255.0 * (0.45 + 0.45 * t)) as u8;
            pixels.extend_from_slice(&[b, g, r, 255]);
        }
    }
    MediaSnapshot {
        title: "Notte di luce".into(),
        artist: "Gli Esempi".into(),
        album: "Anteprima".into(),
        playing: true,
        position_ms: 72_000,
        duration_ms: 214_000,
        updated_at: sys::now_filetime(),
        rate: 1.0,
        art: Some(Arc::new(Artwork {
            id: 7,
            width: w,
            height: h,
            pixels,
        })),
    }
}

fn sample_stats() -> Stats {
    Stats {
        cpu: Some(0.23),
        ram_used: 9_800_000_000,
        ram_total: 16_000_000_000,
        disk_busy: Some(0.04),
        disk_io: Some(1.4e6),
        gpu: Some(0.41),
        gpu_memory: Some(2_100_000_000),
        download: Some(3.2e6),
        upload: Some(2.8e5),
        power_w: Some(18.4),
        power_source: Some(PowerSource::Battery),
        cpu_temp: Some(58.0),
        cpu_temp_source: Some(TempSource::ThermalZone),
        gpu_temp: Some(51.0),
        gpu_temp_source: Some(TempSource::Driver),
    }
}
