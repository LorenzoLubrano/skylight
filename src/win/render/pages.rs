//! Pagine del notch aperto: intestazione (titolo, batteria, data e ora),
//! frecce ai lati, pallini in basso che diventano la barra delle icone, e il
//! contenuto di ogni pagina, che scorre di lato quando si cambia pagina.

use windows::core::Result;

use super::*;
use crate::features::pomodoro::Phase;
use crate::features::shelf::ShelfItem;
use crate::features::sysinfo::{
    Heat, PowerSource, Stats, TempSource, fmt_bytes, fmt_percent, fmt_rate, fmt_temp, fmt_watts,
    heat,
};
use crate::features::water::{WaterPlan, fmt_liters};
use crate::time::{Clock, civil, fmt_countdown, fmt_hm};
use crate::win::app::{HIST_CPU, HIST_DOWN, HIST_GPU, HIST_UP};
use crate::win::sys;

const GLYPH_SETTINGS: char = '\u{E713}';
const GLYPH_RESET: char = '\u{E72C}';
const GLYPH_SKIP: char = '\u{E893}';
const GLYPH_UNDO: char = '\u{E7A7}';
const GLYPH_DOCUMENT: char = '\u{E8A5}';
const GLYPH_FOLDER: char = '\u{E8B7}';
const GLYPH_TEXT: char = '\u{E7C3}';
const GLYPH_CANCEL: char = '\u{E711}';
const GLYPH_ADD: char = '\u{E710}';
const GLYPH_DELETE: char = '\u{E74D}';
const GLYPH_CHECK: char = '\u{E73E}';
const GLYPH_LEFT: char = '\u{E76B}';
const GLYPH_RIGHT: char = '\u{E76C}';
const GLYPH_DROP: char = '\u{EB42}';
const GLYPH_BELL: char = '\u{EA8F}';
const GLYPH_CHECKLIST: char = '\u{E9D5}';
const GLYPH_DOWNLOAD: char = '\u{E896}';

/// Di quanto scorre di lato il contenuto quando si cambia pagina (DIP).
const SLIDE: f32 = 28.0;

/// Tutto il notch aperto; `alpha` è la sua opacità durante apertura e chiusura.
pub(super) fn draw(g: &mut Gfx, app: &App, alpha: f32) -> Result<()> {
    let content_h = app.content_height();
    draw_header(g, app, alpha)?;

    // Cambio pagina: la vecchia esce da un lato, la nuova entra dall'altro.
    let f = app.page_fade.value().clamp(0.0, 1.0);
    let slide = if app.reduce_motion() { 0.0 } else { SLIDE };
    if let Some(from) = app.slide_from.filter(|_| f < 0.999) {
        // Se la pagina che esce era a metà di un'altra entrata, parte dalla sua opacità.
        let out = (1.0 - smooth(0.0, 0.6, f)) * app.slide_from_alpha;
        if out > 0.005 {
            shifted(g, -f * slide * app.slide_dir, |g| {
                draw_page(g, app, from, alpha * out)
            })?;
        }
    }
    let incoming = smooth(0.15, 1.0, f);
    shifted(g, (1.0 - f) * slide * app.slide_dir, |g| {
        draw_page(g, app, app.page, alpha * incoming)
    })?;

    draw_arrows(g, app, alpha);
    draw_footer(g, app, content_h, alpha);
    Ok(())
}

/// Disegna spostando tutto di `dx` DIP in orizzontale.
fn shifted(g: &mut Gfx, dx: f32, draw: impl FnOnce(&mut Gfx) -> Result<()>) -> Result<()> {
    if dx.abs() < 0.01 {
        return draw(g);
    }
    let mut before = Matrix3x2::identity();
    unsafe {
        g.dc.GetTransform(&mut before);
        g.dc.SetTransform(&(Matrix3x2::translation(dx, 0.0) * before));
    }
    let result = draw(g);
    unsafe { g.dc.SetTransform(&before) };
    result
}

fn draw_page(g: &mut Gfx, app: &App, page: Page, alpha: f32) -> Result<()> {
    if alpha <= 0.005 {
        return Ok(());
    }
    match page {
        Page::Music => super::music::draw(g, app, alpha)?,
        Page::Focus => focus(g, app, alpha)?,
        Page::Water => water(g, app, alpha)?,
        Page::Shelf => shelf(g, app, alpha)?,
        Page::Todo => todo(g, app, alpha),
        Page::System => system(g, app, alpha)?,
    }
    Ok(())
}

// ---- Cornice ----------------------------------------------------------------------------

fn draw_header(g: &Gfx, app: &App, alpha: f32) -> Result<()> {
    let h = header();
    text(g, app.page.title(), &g.fonts.header, h.title, TEXT, alpha);

    // Al centro: batteria, data e ora (o un breve messaggio, es. "Copiato").
    if let Some((note, _)) = app.note {
        text(g, note, &g.fonts.date_center, h.status, TEXT_DIM, alpha);
    } else {
        let date = sys::short_date_text(true);
        let clock = sys::clock_text();
        let percent = app.battery.map(|b| format!("{}%", b.percent));
        let date_w = g.measure(&date, &g.fonts.date);
        let clock_w = g.measure(&clock, &g.fonts.status_bold);
        let battery_w = percent
            .as_ref()
            .map_or(0.0, |p| 14.0 + 4.0 + g.measure(p, &g.fonts.small) + 10.0);
        let download_w = if app.showing_download() {
            10.0 + 14.0
        } else {
            0.0
        };
        let total = battery_w + date_w + 8.0 + clock_w + download_w;
        let mut x = h.status.center_x() - total / 2.0;
        let (y, hh) = (h.status.y, h.status.h);
        if let (Some(battery), Some(percent)) = (app.battery, &percent) {
            battery_ring(
                g,
                Rect::new(x, y + (hh - 14.0) / 2.0, 14.0, 14.0),
                battery,
                alpha,
            )?;
            let w = g.measure(percent, &g.fonts.small);
            text(
                g,
                percent,
                &g.fonts.small,
                Rect::new(x + 18.0, y, w + 2.0, hh),
                TEXT_DIM,
                alpha,
            );
            x += battery_w;
        }
        text(
            g,
            &date,
            &g.fonts.date,
            Rect::new(x, y, date_w + 2.0, hh),
            TEXT,
            alpha,
        );
        x += date_w + 8.0;
        text(
            g,
            &clock,
            &g.fonts.status_bold,
            Rect::new(x, y, clock_w + 2.0, hh),
            TEXT,
            alpha,
        );
        if app.showing_download() {
            let r = Rect::new(x + clock_w + 10.0, y + (hh - 14.0) / 2.0, 14.0, 14.0);
            download_ring(g, r, app.download_turn(), alpha)?;
        }
    }

    // A destra: l'azione della pagina.
    if app.page == Page::Shelf && !app.shelf.is_empty() {
        let label = if app.selection.is_empty() {
            "Svuota".to_string()
        } else {
            format!("Rimuovi ({})", app.selection.len())
        };
        pill(
            g,
            h.action,
            None,
            &label,
            None,
            app.hot == Some(Hit::ShelfAction),
            alpha,
        );
    }
    Ok(())
}

fn draw_arrows(g: &Gfx, app: &App, alpha: f32) {
    let (prev, next) = arrows();
    for (r, c, hit) in [
        (prev, GLYPH_LEFT, Hit::PrevPage),
        (next, GLYPH_RIGHT, Hit::NextPage),
    ] {
        let hot = app.hot == Some(hit);
        fill_round(g, r, r.w / 2.0, if hot { PRESSED } else { HOVER }, alpha);
        glyph(
            g,
            c,
            &g.fonts.icon_small,
            r,
            if hot { TEXT } else { TEXT_DIM },
            alpha,
        );
    }
}

fn draw_footer(g: &Gfx, app: &App, content_h: f32, alpha: f32) {
    let f = footer(content_h);
    let reveal = app.footer_reveal.value().clamp(0.0, 1.0);

    // Pallini: quello della pagina attuale è una piccola pillola.
    let dots = alpha * (1.0 - reveal);
    if dots > 0.005 {
        for (page, slot) in f.dots {
            let current = page == app.page;
            let (w, h) = (if current { 16.0 } else { 5.0 }, 5.0);
            let r = Rect::new(slot.center_x() - w / 2.0, slot.center_y() - h / 2.0, w, h);
            let hot = app.hot == Some(Hit::Page(page));
            let color = if current || hot { TEXT } else { TEXT_FAINT };
            fill_round(g, r, h / 2.0, color, dots);
        }
    }

    // Barra delle icone: compare crescendo leggermente dal centro.
    let bar = alpha * reveal;
    if bar > 0.005 {
        let k = 0.92 + 0.08 * reveal;
        let grow = |r: Rect| {
            let cx = f.bar.center_x();
            let cy = f.bar.center_y();
            Rect::new(cx + (r.x - cx) * k, cy + (r.y - cy) * k, r.w * k, r.h * k)
        };
        let shape = grow(f.bar);
        fill_round(g, shape, shape.h / 2.0, SURFACE, bar);
        stroke_round(
            g,
            shape,
            shape.h / 2.0,
            rgba(1.0, 1.0, 1.0, 0.06),
            bar,
            1.0,
            false,
        );
        for (page, r) in f.tabs {
            let r = grow(r);
            let current = app.page == page;
            let hot = app.hot == Some(Hit::Page(page));
            if hot {
                fill_round(g, r, r.h / 2.0, HOVER, bar);
            }
            let color = if current {
                page_color(page)
            } else if hot {
                TEXT
            } else {
                TEXT_DIM
            };
            glyph(g, page.glyph(), &g.fonts.icon, r, color, bar);
        }
        fill_round(g, grow(f.divider), 0.5, TEXT_FAINT, bar);
        let r = grow(f.settings);
        let hot = app.hot == Some(Hit::Settings);
        if hot {
            fill_round(g, r, r.h / 2.0, HOVER, bar);
        }
        glyph(
            g,
            GLYPH_SETTINGS,
            &g.fonts.icon,
            r,
            if hot { TEXT } else { TEXT_DIM },
            bar,
        );
    }
}

/// Pulsante rotondo grigio con un glifo (reset, salta, annulla...).
fn round_button(g: &Gfx, r: Rect, c: char, hot: bool, alpha: f32) {
    fill_round(g, r, r.w / 2.0, if hot { PRESSED } else { HOVER }, alpha);
    glyph(g, c, &g.fonts.icon_small, r, TEXT, alpha);
}

// ---- Focus ----------------------------------------------------------------------------

fn phase_color(phase: Phase) -> D2D1_COLOR_F {
    match phase {
        Phase::Focus => RED,
        Phase::ShortBreak => GREEN,
        Phase::LongBreak => BLUE,
    }
}

fn phase_chip(phase: Phase) -> (char, &'static str) {
    match phase {
        // Cronometro per la concentrazione, luna per le pause.
        Phase::Focus => ('\u{E916}', "Focus"),
        Phase::ShortBreak => ('\u{E708}', "Pausa breve"),
        Phase::LongBreak => ('\u{E708}', "Pausa lunga"),
    }
}

fn focus(g: &Gfx, app: &App, alpha: f32) -> Result<()> {
    let l = FocusLayout::new();
    let now = time::now_ms();
    let d = Durations::from_settings(&app.settings);
    let p = &app.pomodoro;
    let color = phase_color(p.phase());
    let hot = |h: Hit| app.hot == Some(h);

    // Anello: il tempo che resta, con un alone del colore della fase.
    let remaining = 1.0 - p.progress(now, &d);
    let ring_r = l.ring.inset(5.0);
    let glow = if p.is_running() { 1.0 } else { 0.6 };
    for (w, a) in [(18.0, 0.07), (13.0, 0.12)] {
        arc(g, ring_r, w, remaining, color, alpha * a * glow)?;
    }
    arc(g, ring_r, 8.0, 1.0, with_alpha(color, 0.18), alpha)?;
    arc(g, ring_r, 8.0, remaining, color, alpha)?;
    glyph(g, GLYPH_STOPWATCH, &g.fonts.icon_xl, l.ring, TEXT, alpha);

    let paused = p.is_active() && !p.is_running();
    text(
        g,
        &fmt_countdown(p.remaining(now, &d)),
        &g.fonts.timer,
        l.timer,
        if paused { TEXT_DIM } else { TEXT },
        alpha,
    );
    let caption = match p.phase() {
        Phase::Focus => {
            let (n, total) = p.session(&d);
            format!("Concentrazione · sessione {n} di {total}")
        }
        phase => phase.label().to_string(),
    };
    let caption = if paused {
        format!("In pausa · {caption}")
    } else {
        caption
    };
    text(
        g,
        &caption,
        &g.fonts.body_center,
        l.caption,
        TEXT_DIM,
        alpha,
    );

    round_button(g, l.reset, GLYPH_RESET, hot(Hit::FocusReset), alpha);
    round_button(g, l.skip, GLYPH_SKIP, hot(Hit::FocusSkip), alpha);
    for (r, phase) in l.chips.iter().zip(FocusLayout::other_phases(p.phase())) {
        let (icon, label) = phase_chip(phase);
        pill(
            g,
            *r,
            Some(icon),
            label,
            None,
            hot(Hit::FocusPhase(phase)),
            alpha,
        );
    }
    let fill = if hot(Hit::FocusToggle) {
        brighter(color)
    } else {
        color
    };
    fill_round(g, l.toggle, l.toggle.h / 2.0, fill, alpha);
    let play = if p.is_running() {
        GLYPH_PAUSE
    } else {
        GLYPH_PLAY
    };
    glyph(g, play, &g.fonts.icon, l.toggle, TEXT, alpha);
    Ok(())
}

// ---- Acqua ----------------------------------------------------------------------------

/// "1,25" per 1250 ml: il numero senza unità, per la cifra grande.
fn liters_number(ml: u32) -> String {
    let text = fmt_liters(ml);
    let number = text.trim_end_matches(" L");
    if number.contains(',') {
        number.to_string()
    } else {
        format!("{number},0")
    }
}

fn water(g: &Gfx, app: &App, alpha: f32) -> Result<()> {
    let l = WaterLayout::new();
    let clock = sys::clock();
    let plan = WaterPlan::from_settings(&app.settings);
    let total = app.water.total(&clock, clock.today());
    let fraction = total as f32 / plan.goal_ml.max(1) as f32;
    let hot = |h: Hit| app.hot == Some(h);

    // "1,0 / 2 L"
    let amount = liters_number(total);
    let amount_w = g.measure(&amount, &g.fonts.amount);
    text(g, &amount, &g.fonts.amount, l.amount, TEXT, alpha);
    let goal = Rect::new(
        l.amount.x + amount_w + 5.0,
        l.amount.y + 6.0,
        120.0,
        l.amount.h,
    );
    text(
        g,
        &format!("/ {}", fmt_liters(plan.goal_ml)),
        &g.fonts.title,
        goal,
        TEXT_DIM,
        alpha,
    );

    // Interruttore dei promemoria, con la campanella nel pomello.
    let on = app.settings.water_reminders;
    let track = if on { BLUE } else { HOVER };
    let track = if hot(Hit::WaterReminders) {
        brighter(track)
    } else {
        track
    };
    fill_round(g, l.reminders, l.reminders.h / 2.0, track, alpha);
    let knob_x = if on {
        l.reminders.right() - l.reminders.h + 2.0
    } else {
        l.reminders.x + 2.0
    };
    let knob = Rect::new(
        knob_x,
        l.reminders.y + 2.0,
        l.reminders.h - 4.0,
        l.reminders.h - 4.0,
    );
    fill_round(g, knob, knob.w / 2.0, TEXT, alpha);
    glyph(
        g,
        GLYPH_BELL,
        &g.fonts.icon_tiny,
        knob,
        rgba(0.2, 0.2, 0.22, 1.0),
        alpha,
    );

    let percent = (fraction * 100.0).min(999.0);
    let reminder = if !on {
        "promemoria spenti".to_string()
    } else {
        match app.water_next_reminder() {
            Some(t) if clock.day_of(t) == clock.today() => {
                format!("prossimo alle {}", fmt_hm(clock.minute_of_day(t)))
            }
            _ => "promemoria attivi".to_string(),
        }
    };
    text(
        g,
        &format!("Oggi · {percent:.0}% · {reminder}"),
        &g.fonts.artist,
        l.caption,
        TEXT_DIM,
        alpha,
    );
    progress(g, l.bar, fraction, LIGHT_BLUE, alpha);

    let drink_fill = if hot(Hit::Drink) {
        brighter(BLUE)
    } else {
        BLUE
    };
    pill(
        g,
        l.drink,
        Some(GLYPH_DROP),
        &format!("Bevi {} ml", plan.glass_ml),
        Some(drink_fill),
        false,
        alpha,
    );
    round_button(g, l.undo, GLYPH_UNDO, hot(Hit::UndoDrink), alpha);
    fill_round(g, l.divider, 0.5, rgba(1.0, 1.0, 1.0, 0.1), alpha);

    // Riquadro a destra: indicatore circolare o grafico ora per ora (clic per cambiare).
    if hot(Hit::WaterView) {
        fill_round(g, l.panel.inset(-4.0), 14.0, SURFACE, alpha);
    }
    let dots = l.view_dots;
    for (i, active) in [!app.water_chart, app.water_chart].iter().enumerate() {
        let r = Rect::new(dots.x + 4.0 + i as f32 * 9.0, dots.y + 1.5, 5.0, 5.0);
        fill_round(g, r, 2.5, if *active { TEXT } else { TEXT_FAINT }, alpha);
    }
    if !app.water_chart {
        // 270 gradi, aperto in basso, come un indicatore di carburante.
        let size = 86.0;
        let gauge = Rect::new(l.panel.center_x() - size / 2.0, l.panel.y + 9.0, size, size);
        sweep(g, gauge, 8.0, 135.0, 270.0, TRACK, alpha)?;
        sweep(
            g,
            gauge,
            8.0,
            135.0,
            270.0 * fraction.min(1.0),
            LIGHT_BLUE,
            alpha,
        )?;
        let value = Rect::new(gauge.x, gauge.y + 20.0, gauge.w, 30.0);
        text(
            g,
            &format!("{percent:.0}"),
            &g.fonts.amount_center,
            value,
            TEXT,
            alpha,
        );
        let unit = Rect::new(gauge.x, gauge.y + 50.0, gauge.w, 16.0);
        text(g, "% di oggi", &g.fonts.small_center, unit, TEXT_DIM, alpha);
    } else {
        let hours = app
            .water
            .hourly(&clock, clock.today(), plan.from_h, plan.to_h);
        let slots = l.chart_slots(hours.len());
        let max = hours.iter().copied().max().unwrap_or(0).max(plan.glass_ml) as f32;
        let current = (clock.minute_of_day(clock.now) / 60)
            .checked_sub(plan.from_h)
            .map(|h| h as usize);
        for (i, (slot, ml)) in slots.iter().zip(&hours).enumerate() {
            let track = Rect::new(slot.x, slot.bottom() - 3.0, slot.w, 3.0);
            fill_round(g, track, 1.5, TRACK, alpha);
            if *ml > 0 {
                let h = (*ml as f32 / max * slot.h).max(5.0);
                let bar = Rect::new(slot.x, slot.bottom() - h, slot.w, h);
                let color = if Some(i) == current {
                    LIGHT_BLUE
                } else {
                    with_alpha(LIGHT_BLUE, 0.5)
                };
                fill_round(g, bar, (slot.w / 2.0).min(3.0), color, alpha);
            }
            let hour = plan.from_h + i as u32;
            if hour.is_multiple_of(3) {
                let label = Rect::new(slot.center_x() - 16.0, l.labels_y, 32.0, 14.0);
                text(
                    g,
                    &hour.to_string(),
                    &g.fonts.small_center,
                    label,
                    TEXT_FAINT,
                    alpha,
                );
            }
        }
    }
    Ok(())
}

// ---- Shelf ----------------------------------------------------------------------------

fn shelf(g: &mut Gfx, app: &App, alpha: f32) -> Result<()> {
    if app.shelf.is_empty() {
        let zone = shelf_drop_zone();
        let color = if app.drop_hover { PURPLE } else { TEXT_FAINT };
        if app.drop_hover {
            fill_round(g, zone, 16.0, with_alpha(PURPLE, 0.12), alpha);
        }
        stroke_round(g, zone, 16.0, color, alpha, 1.5, true);
        let icon = Rect::new(zone.center_x() - 18.0, zone.y + 10.0, 36.0, 30.0);
        glyph(
            g,
            GLYPH_DOWNLOAD,
            &g.fonts.icon_xl,
            icon,
            if app.drop_hover { PURPLE } else { TEXT_DIM },
            alpha,
        );
        let message = if app.drop_hover {
            "Rilascia per aggiungere"
        } else {
            "Trascina qui file, cartelle o testo"
        };
        text(
            g,
            message,
            &g.fonts.button,
            Rect::new(zone.x, zone.y + 44.0, zone.w, 20.0),
            TEXT,
            alpha,
        );
        text(
            g,
            "Poi trascinali fuori quando vuoi · clic per aprire o copiare",
            &g.fonts.small_center,
            Rect::new(zone.x, zone.y + 64.0, zone.w, 16.0),
            TEXT_DIM,
            alpha,
        );
        return Ok(());
    }

    for (i, entry) in app.shelf_visible().iter().enumerate() {
        let tile = shelf_tile(i);
        let parts = tile_parts(tile);
        let hot = matches!(app.hot, Some(Hit::ShelfItem(j) | Hit::ShelfRemove(j)) if j == i);
        let selected = app.selection.contains(&entry.id);
        let fill = if selected {
            with_alpha(PURPLE, 0.22)
        } else if hot {
            HOVER
        } else {
            SURFACE
        };
        fill_round(g, tile, 12.0, fill, alpha);
        if selected {
            stroke_round(g, tile, 12.0, PURPLE, alpha, 1.5, false);
        }
        match (app.thumbs.get(&entry.id), &entry.item) {
            (Some(art), _) => image(g, art, parts.thumb, 6.0, alpha, true)?,
            (None, ShelfItem::Text { .. }) => glyph(
                g,
                GLYPH_TEXT,
                &g.fonts.icon_huge,
                parts.thumb,
                TEXT_DIM,
                alpha,
            ),
            (None, ShelfItem::File { .. }) => {
                let is_folder = !entry.name().contains('.');
                let (c, color) = if is_folder {
                    (GLYPH_FOLDER, LIGHT_BLUE)
                } else {
                    (GLYPH_DOCUMENT, TEXT_DIM)
                };
                glyph(g, c, &g.fonts.icon_huge, parts.thumb, color, alpha);
            }
        }
        text(
            g,
            &entry.name(),
            &g.fonts.small_center,
            parts.name,
            TEXT,
            alpha,
        );
        if hot {
            let remove_hot = app.hot == Some(Hit::ShelfRemove(i));
            fill_round(
                g,
                parts.remove,
                parts.remove.w / 2.0,
                if remove_hot { PRESSED } else { HOVER },
                alpha,
            );
            glyph(
                g,
                GLYPH_CANCEL,
                &g.fonts.icon_tiny,
                parts.remove,
                TEXT,
                alpha,
            );
        }
    }
    if app.drop_hover {
        let area = Rect::new(
            content_x() - 4.0,
            HEADER_H,
            CONTENT_W + 8.0,
            app.content_height() - 4.0,
        );
        stroke_round(g, area, 16.0, PURPLE, alpha, 1.5, true);
    }
    Ok(())
}

// ---- To-do ----------------------------------------------------------------------------

fn todo(g: &Gfx, app: &App, alpha: f32) {
    let clock = sys::clock();
    let s = todo_summary();

    // A sinistra: "1/4 fatte oggi", la prossima scadenza e "Pulisci".
    let done = app.todo.done_today(&clock);
    let total = app.todo.tasks.len();
    let count = format!("{done}/{total}");
    text(g, &count, &g.fonts.amount, s.count, TEXT, alpha);
    text(g, "fatte oggi", &g.fonts.artist, s.caption, TEXT_DIM, alpha);
    let next = match app.todo.next_due() {
        Some(t) if clock.day_of(t) == clock.today() => {
            format!("Prossima alle {}", fmt_hm(clock.minute_of_day(t)))
        }
        Some(_) => "Prossima nei giorni dopo".to_string(),
        None => "Nessuna scadenza".to_string(),
    };
    text(g, &next, &g.fonts.small, s.next, GREEN, alpha);
    if app.todo.has_done() {
        pill(
            g,
            s.clear,
            Some(GLYPH_CHECK),
            "Pulisci",
            None,
            app.hot == Some(Hit::TodoAction),
            alpha,
        );
    }

    // A destra: il riquadro con le attività.
    let tasks = app.todo_visible();
    let panel = todo_panel(tasks.len());
    fill_round(g, panel, 14.0, SURFACE, alpha);
    if tasks.is_empty() {
        let r = todo_empty();
        let icon = Rect::new(r.center_x() - 15.0, r.y + 4.0, 30.0, 28.0);
        fill_round(g, icon, 8.0, HOVER, alpha);
        glyph(g, GLYPH_CHECKLIST, &g.fonts.icon_big, icon, TEXT_DIM, alpha);
        text(
            g,
            "Nessuna attività",
            &g.fonts.body_center,
            Rect::new(r.x, r.y + 34.0, r.w, 18.0),
            TEXT_DIM,
            alpha,
        );
    }
    for (i, task) in tasks.iter().enumerate() {
        let r = todo_row(i);
        let hot = matches!(app.hot, Some(Hit::TaskCheck(j) | Hit::TaskDelete(j)) if j == i);
        if hot {
            fill_round(g, r.row, 9.0, HOVER, alpha);
        }
        if task.done {
            fill_round(g, r.check, r.check.w / 2.0, GREEN, alpha);
            glyph(g, GLYPH_CHECK, &g.fonts.icon_tiny, r.check, TEXT, alpha);
        } else {
            stroke_round(
                g,
                r.check,
                r.check.w / 2.0,
                if hot { TEXT } else { TEXT_DIM },
                alpha,
                1.5,
                false,
            );
        }
        text(
            g,
            &task.title,
            &g.fonts.body,
            r.title,
            if task.done { TEXT_FAINT } else { TEXT },
            alpha,
        );

        if hot {
            let delete_hot = app.hot == Some(Hit::TaskDelete(i));
            icon_button(
                g,
                r.delete,
                GLYPH_DELETE,
                &g.fonts.icon_small,
                delete_hot,
                if delete_hot { RED } else { TEXT_DIM },
                alpha,
            );
        } else if let Some(due) = task.due {
            let minute = clock.minute_of_day(due);
            let day = clock.day_of(due);
            let ahead = day - clock.today();
            let label = if ahead == 0 {
                fmt_hm(minute)
            } else if (1..7).contains(&ahead) {
                // "mar 08:00": il giorno della settimana, non "dom" (che si legge domenica).
                const DAYS: [&str; 7] = ["lun", "mar", "mer", "gio", "ven", "sab", "dom"];
                format!("{} {}", DAYS[Clock::weekday(day) as usize], fmt_hm(minute))
            } else {
                let (_, month, d) = civil(day);
                format!("{d}/{month}")
            };
            let late = !task.done && due <= clock.now;
            let (fill, color) = if task.done {
                (HOVER, TEXT_FAINT)
            } else if late {
                (with_alpha(RED, 0.2), RED)
            } else {
                (with_alpha(GREEN, 0.18), GREEN)
            };
            fill_round(g, r.due, r.due.h / 2.0, fill, alpha);
            text(g, &label, &g.fonts.chip, r.due, color, alpha);
        }
    }

    let add = todo_add_row(tasks.len());
    let field = Rect::new(add.x + 10.0, add.y, add.w - 20.0, add.h);
    match &app.editing {
        Some(edit) => {
            fill_round(g, add, 9.0, HOVER, alpha);
            stroke_round(g, add, 9.0, GREEN, alpha, 1.0, false);
            if edit.is_empty() && edit.text().is_empty() {
                text(
                    g,
                    "es. Chiama Dina alle 15:30",
                    &g.fonts.body,
                    field,
                    TEXT_FAINT,
                    alpha,
                );
            }
            // Il testo scorre di lato perché il cursore resti sempre visibile.
            let font = &g.fonts.input;
            let caret = g.caret_x(edit.text(), font, edit.caret_utf16());
            let width = g.measure(edit.text(), font);
            let margin = 4.0;
            let mut offset = app.edit_scroll.get();
            if caret - offset > field.w - margin {
                offset = caret - field.w + margin;
            } else if caret - offset < margin {
                offset = caret - margin;
            }
            offset = offset.min(width - field.w + margin).max(0.0);
            app.edit_scroll.set(offset);
            if !edit.text().is_empty() {
                unsafe {
                    g.dc.PushAxisAlignedClip(&d2d_rect(field), D2D1_ANTIALIAS_MODE_ALIASED);
                }
                let line = Rect::new(field.x - offset, field.y, width + field.w, field.h);
                text(g, edit.text(), font, line, TEXT, alpha);
                unsafe { g.dc.PopAxisAlignedClip() };
            }
            fill_round(
                g,
                Rect::new(field.x + caret - offset, add.y + 7.0, 1.5, add.h - 14.0),
                0.75,
                GREEN,
                alpha,
            );
        }
        None => {
            let hot = app.hot == Some(Hit::TaskAdd);
            if hot {
                fill_round(g, add, 9.0, HOVER, alpha);
            }
            let color = if hot { TEXT } else { TEXT_DIM };
            glyph(
                g,
                GLYPH_ADD,
                &g.fonts.icon_small,
                Rect::new(add.x + 4.0, add.y, 20.0, add.h),
                GREEN,
                alpha,
            );
            text(
                g,
                "Nuova attività",
                &g.fonts.body,
                Rect::new(add.x + 28.0, add.y, add.w - 28.0, add.h),
                color,
                alpha,
            );
        }
    }
}

// ---- Sistema --------------------------------------------------------------------------

enum Bottom {
    Text(String),
    Spark(usize, f32),
}

struct Tile {
    label: &'static str,
    corner: Option<String>,
    value: Option<String>,
    color: D2D1_COLOR_F,
    bottom: Bottom,
}

fn system(g: &Gfx, app: &App, alpha: f32) -> Result<()> {
    let s = &app.stats;
    let waiting = *s == Stats::default();
    let missing = if waiting { "…" } else { "non disponibile" };
    let or_missing = |source: Option<&str>, value: &Option<String>| -> String {
        match (value, source) {
            (None, _) => missing.to_string(),
            (Some(_), Some(source)) => source.to_string(),
            (Some(_), None) => String::new(),
        }
    };
    let rate_scale = |index: usize| app.history[index].max().max(64.0 * 1024.0);
    let temp_color = |t: Option<f32>| match t.map(heat) {
        Some(Heat::Hot) => DANGER,
        Some(Heat::Warm) => WARN,
        _ => TEXT,
    };
    let temp_source = |source: Option<TempSource>| {
        source.map(|s| match s {
            TempSource::Sensor => "sensore",
            TempSource::ThermalZone => "zona termica (stima)",
            TempSource::Driver => "driver video",
        })
    };
    let power_source = s.power_source.map(|p| match p {
        PowerSource::Battery => "dalla batteria",
        PowerSource::Meter => "misuratore Windows",
        PowerSource::Sensors => "CPU + GPU, sensori",
        PowerSource::Platform => "piattaforma (RAPL)",
        PowerSource::Processor => "processore (RAPL)",
    });
    // Senza nessuna fonte: col caricatore collegato si misura solo a batteria.
    let power_missing = match app.battery {
        _ if waiting => missing,
        Some(b) if b.charging => "stacca il caricatore",
        _ => missing,
    };

    let cpu_temp = s.cpu_temp.map(fmt_temp);
    let gpu_temp = s.gpu_temp.map(fmt_temp);
    let power = s.power_w.map(fmt_watts);
    let ram_detail = if s.ram_total > 0 {
        format!("{} di {}", fmt_bytes(s.ram_used), fmt_bytes(s.ram_total))
    } else {
        missing.to_string()
    };
    let disk = s.disk_busy.map(fmt_percent);
    let tiles = [
        Tile {
            label: "CPU",
            corner: None,
            value: s.cpu.map(fmt_percent),
            color: TEXT,
            bottom: Bottom::Spark(HIST_CPU, 1.0),
        },
        Tile {
            label: "RAM",
            corner: None,
            value: s.ram().map(fmt_percent),
            color: TEXT,
            bottom: Bottom::Text(ram_detail),
        },
        Tile {
            label: "Disco",
            corner: None,
            bottom: Bottom::Text(
                s.disk_io
                    .map(fmt_rate)
                    .unwrap_or_else(|| or_missing(None, &disk)),
            ),
            value: disk,
            color: TEXT,
        },
        Tile {
            label: "GPU",
            corner: s.gpu_memory.map(fmt_bytes),
            value: s.gpu.map(fmt_percent),
            color: TEXT,
            bottom: Bottom::Spark(HIST_GPU, 1.0),
        },
        Tile {
            label: "Download",
            corner: None,
            value: s.download.map(fmt_rate),
            color: TEXT,
            bottom: Bottom::Spark(HIST_DOWN, rate_scale(HIST_DOWN)),
        },
        Tile {
            label: "Upload",
            corner: None,
            value: s.upload.map(fmt_rate),
            color: TEXT,
            bottom: Bottom::Spark(HIST_UP, rate_scale(HIST_UP)),
        },
        Tile {
            label: "Consumo",
            corner: None,
            bottom: Bottom::Text(match (&power, power_source) {
                (None, _) => power_missing.to_string(),
                (Some(_), source) => source.unwrap_or_default().to_string(),
            }),
            value: power,
            color: TEXT,
        },
        Tile {
            label: "Temp. CPU",
            corner: None,
            bottom: Bottom::Text(or_missing(temp_source(s.cpu_temp_source), &cpu_temp)),
            value: cpu_temp,
            color: temp_color(s.cpu_temp),
        },
        Tile {
            label: "Temp. GPU",
            corner: None,
            bottom: Bottom::Text(or_missing(temp_source(s.gpu_temp_source), &gpu_temp)),
            value: gpu_temp,
            color: temp_color(s.gpu_temp),
        },
    ];

    for (i, tile) in tiles.iter().enumerate() {
        let r = system_tile(i);
        let parts = system_tile_parts(r);
        fill_round(g, r, 12.0, SURFACE, alpha);
        text(g, tile.label, &g.fonts.small, parts.label, TEXT_DIM, alpha);
        if let Some(corner) = &tile.corner {
            text(
                g,
                corner,
                &g.fonts.small_right,
                parts.label,
                TEXT_FAINT,
                alpha,
            );
        }
        let value = tile.value.as_deref().unwrap_or("—");
        let color = if tile.value.is_some() {
            tile.color
        } else {
            TEXT_FAINT
        };
        text(g, value, &g.fonts.metric, parts.value, color, alpha);
        match &tile.bottom {
            Bottom::Text(detail) => {
                text(g, detail, &g.fonts.small, parts.bottom, TEXT_FAINT, alpha)
            }
            Bottom::Spark(index, max) => {
                let values: Vec<f32> = app.history[*index].values().collect();
                let color = match *index {
                    HIST_DOWN => LIGHT_BLUE,
                    HIST_UP => PURPLE,
                    _ => TEAL,
                };
                sparkline(g, parts.bottom, &values, *max, color, alpha)?;
            }
        }
    }
    Ok(())
}
