//! Disegno con Direct2D. Le funzioni leggono lo stato dell'app e non lo
//! modificano: tutto ciò che si vede dipende solo da quello stato.

mod music;
mod pages;
pub mod settings;

use std::mem::ManuallyDrop;

use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::{DWRITE_MEASURING_MODE_NATURAL, IDWriteTextFormat};
use windows::core::Result;
use windows_numerics::{Matrix3x2, Vector2};

use super::app::App;
use super::gfx::Gfx;
use super::power::Battery;
use crate::features::alerts::{Alert, AlertKind};
use crate::features::pomodoro::Durations;
use crate::layout::*;
use crate::media::Artwork;
use crate::time;

pub(super) const fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

// Colori di sistema di Apple in modalità scura: etichette, riempimenti e tinte.
pub(super) const NOTCH: D2D1_COLOR_F = rgba(0.0, 0.0, 0.0, 1.0);
pub(super) const TEXT: D2D1_COLOR_F = rgba(1.0, 1.0, 1.0, 1.0);
pub(super) const TEXT_DIM: D2D1_COLOR_F = rgba(0.92, 0.92, 0.96, 0.6);
pub(super) const TEXT_FAINT: D2D1_COLOR_F = rgba(0.92, 0.92, 0.96, 0.3);
pub(super) const TRACK: D2D1_COLOR_F = rgba(0.47, 0.47, 0.5, 0.36);
/// Fondo dei riquadri (#1C1C1E), dei pulsanti (#2C2C2E) e dei pulsanti col mouse sopra.
pub(super) const SURFACE: D2D1_COLOR_F = rgba(0.11, 0.11, 0.118, 1.0);
pub(super) const HOVER: D2D1_COLOR_F = rgba(0.173, 0.173, 0.18, 1.0);
pub(super) const PRESSED: D2D1_COLOR_F = rgba(0.227, 0.227, 0.235, 1.0);
pub(super) const RED: D2D1_COLOR_F = rgba(1.0, 0.271, 0.227, 1.0);
pub(super) const ORANGE: D2D1_COLOR_F = rgba(1.0, 0.624, 0.039, 1.0);
pub(super) const YELLOW: D2D1_COLOR_F = rgba(1.0, 0.839, 0.039, 1.0);
pub(super) const GREEN: D2D1_COLOR_F = rgba(0.188, 0.82, 0.345, 1.0);
pub(super) const TEAL: D2D1_COLOR_F = rgba(0.251, 0.784, 0.878, 1.0);
pub(super) const BLUE: D2D1_COLOR_F = rgba(0.039, 0.518, 1.0, 1.0);
pub(super) const LIGHT_BLUE: D2D1_COLOR_F = rgba(0.392, 0.71, 1.0, 1.0);
pub(super) const PURPLE: D2D1_COLOR_F = rgba(0.749, 0.353, 0.949, 1.0);
pub(super) const PINK: D2D1_COLOR_F = rgba(1.0, 0.216, 0.373, 1.0);
pub(super) const WARN: D2D1_COLOR_F = ORANGE;
pub(super) const DANGER: D2D1_COLOR_F = RED;

// Glifi di Segoe Fluent Icons / Segoe MDL2 Assets (verificati su entrambi).
pub(super) const GLYPH_PLAY: char = '\u{E768}';
pub(super) const GLYPH_PAUSE: char = '\u{E769}';
pub(super) const GLYPH_STOPWATCH: char = '\u{E916}';
const GLYPH_MUSIC: char = '\u{EC4F}';

/// Fattore per approssimare un quarto di cerchio con una curva di Bézier cubica.
const KAPPA: f32 = 0.5523;

/// Colore di ogni pagina, come le tinte delle app di Apple.
pub(super) fn page_color(page: Page) -> D2D1_COLOR_F {
    match page {
        Page::Music => PINK,
        Page::Focus => RED,
        Page::Water => LIGHT_BLUE,
        Page::Shelf => PURPLE,
        Page::Todo => GREEN,
        Page::System => TEAL,
    }
}

pub(super) use crate::anim::smooth;

pub fn draw(g: &mut Gfx, app: &App) -> Result<()> {
    let notch = notch_rect(app.width.value(), app.height.value());
    let t = app.openness.value().clamp(0.0, 1.0);
    let radius = RADIUS_CLOSED + (RADIUS_OPEN - RADIUS_CLOSED) * t;
    let calm = app.reduce_motion();

    // Come la Dynamic Island: il contenuto chiuso sparisce subito; quello
    // aperto arriva quando la forma è quasi completa, crescendo un poco
    // dall'alto e mettendosi a fuoco.
    let closed_alpha = (1.0 - smooth(0.0, 0.3, t)) * app.swap.value().clamp(0.0, 1.0);
    let open_alpha = smooth(0.45, 1.0, t);
    let (scale, sigma) = if calm {
        (1.0, 0.0)
    } else {
        (
            0.93 + 0.07 * smooth(0.3, 1.0, t),
            7.0 * (1.0 - smooth(0.45, 1.0, t)),
        )
    };
    let cx = notch.center_x();
    let zoom = Matrix3x2::translation(-cx, 0.0)
        * Matrix3x2::scale(scale, scale)
        * Matrix3x2::translation(cx, 0.0);

    unsafe {
        g.dc.BeginDraw();
        g.dc.Clear(Some(&rgba(0.0, 0.0, 0.0, 0.0)));
    }
    let shape = fill_notch(g, notch, radius);
    // Il contenuto sfocato si registra prima di applicare la maschera della forma.
    let blurred = match (&shape, open_alpha > 0.005 && sigma > 0.3) {
        (Ok(_), true) => record(g, |g| pages::draw(g, app, open_alpha)),
        _ => None,
    };
    let mut result = shape.as_ref().map(|_| ()).map_err(Clone::clone);
    if let Ok(shape) = &shape {
        unsafe {
            // Il contenuto resta dentro la forma del notch, angoli arrotondati compresi,
            // anche quando è ancorato alla misura finale e il notch sta ancora crescendo.
            let mut layer = D2D1_LAYER_PARAMETERS1 {
                contentBounds: D2D_RECT_F {
                    left: -f32::MAX,
                    top: -f32::MAX,
                    right: f32::MAX,
                    bottom: f32::MAX,
                },
                geometricMask: ManuallyDrop::new(Some(shape.clone().into())),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: 1.0,
                opacityBrush: ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS1_NONE,
            };
            g.dc.PushLayer(&layer, None::<&ID2D1Layer>);
            ManuallyDrop::drop(&mut layer.geometricMask);
            // Anche un ritaglio rettangolare: costa poco e vale dove le maschere
            // non sono supportate (Wine, per le anteprime).
            let ear = EAR.min(notch.h * 0.6);
            let bounds = Rect::new(notch.x - ear, 0.0, notch.w + 2.0 * ear, notch.h);
            g.dc.PushAxisAlignedClip(&d2d_rect(bounds), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        if closed_alpha > 0.005 {
            result = result.and(draw_closed(g, app, notch, closed_alpha));
        }
        if open_alpha > 0.005 {
            unsafe { g.dc.SetTransform(&zoom) };
            result = result.and(match &blurred {
                Some(list) => draw_blurred(g, list, sigma),
                None => pages::draw(g, app, open_alpha),
            });
            unsafe { g.dc.SetTransform(&Matrix3x2::identity()) };
        }
        unsafe {
            g.dc.PopAxisAlignedClip();
            g.dc.PopLayer();
        }
    }
    unsafe {
        g.dc.EndDraw(None, None)?;
    }
    result?;
    g.present()
}

/// Registra un disegno in una command list, per poterlo poi sfocare.
/// Se qualcosa non va (driver vecchi, Wine) si rinuncia alla sfocatura per sempre.
fn record(g: &mut Gfx, draw: impl FnOnce(&mut Gfx) -> Result<()>) -> Option<ID2D1CommandList> {
    g.blur.as_ref()?;
    let recorded = unsafe {
        (|| -> Result<ID2D1CommandList> {
            let target = g.dc.GetTarget()?;
            let list = g.dc.CreateCommandList()?;
            g.dc.SetTarget(&list);
            let drawn = draw(g);
            g.dc.SetTarget(&target);
            list.Close()?;
            drawn.map(|_| list)
        })()
    };
    if recorded.is_err() {
        g.blur = None;
    }
    recorded.ok()
}

fn draw_blurred(g: &mut Gfx, list: &ID2D1CommandList, sigma: f32) -> Result<()> {
    let Some(blur) = g.blur.clone() else {
        return Ok(());
    };
    unsafe {
        blur.SetInput(0, list, true);
        blur.SetValue(
            D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32,
            D2D1_PROPERTY_TYPE_FLOAT,
            &sigma.to_le_bytes(),
        )?;
        let output = blur.GetOutput()?;
        g.dc.DrawImage(
            &output,
            None,
            None,
            D2D1_INTERPOLATION_MODE_LINEAR,
            D2D1_COMPOSITE_MODE_SOURCE_OVER,
        );
        blur.SetInput(0, None::<&ID2D1Image>, true);
    }
    Ok(())
}

/// Forma del notch: lati dritti, angoli inferiori arrotondati e piccoli
/// raccordi concavi in alto, dove si fonde con il bordo dello schermo.
fn fill_notch(g: &Gfx, r: Rect, radius: f32) -> Result<ID2D1PathGeometry1> {
    let (x0, x1, bottom) = (r.x, r.right(), r.bottom());
    let rad = radius.min(r.h / 2.0).min(r.w / 2.0).max(0.0);
    let k = rad * (1.0 - KAPPA);
    // Da ridotto (pochi DIP di altezza) anche i raccordi si rimpiccioliscono.
    let ear = EAR.min(r.h * 0.6);
    unsafe {
        let path = g.factory.CreatePathGeometry()?;
        let sink = path.Open()?;
        sink.BeginFigure(v2(x0 - ear, 0.0), D2D1_FIGURE_BEGIN_FILLED);
        sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
            point1: v2(x0, 0.0),
            point2: v2(x0, ear),
        });
        sink.AddLine(v2(x0, (bottom - rad).max(ear)));
        sink.AddBezier(&D2D1_BEZIER_SEGMENT {
            point1: v2(x0, bottom - k),
            point2: v2(x0 + k, bottom),
            point3: v2(x0 + rad, bottom),
        });
        sink.AddLine(v2(x1 - rad, bottom));
        sink.AddBezier(&D2D1_BEZIER_SEGMENT {
            point1: v2(x1 - k, bottom),
            point2: v2(x1, bottom - k),
            point3: v2(x1, (bottom - rad).max(ear)),
        });
        sink.AddLine(v2(x1, ear));
        sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
            point1: v2(x1, 0.0),
            point2: v2(x1 + ear, 0.0),
        });
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close()?;
        g.brush.SetColor(&NOTCH);
        g.dc.FillGeometry(&path, &g.brush, None);
        Ok(path)
    }
}

// ---- Notch chiuso ---------------------------------------------------------------------

fn draw_closed(g: &mut Gfx, app: &App, notch: Rect, alpha: f32) -> Result<()> {
    if app.mini {
        return Ok(());
    }
    if let Some(alert) = app.alerts.current() {
        return draw_alert(g, app, alert, alpha);
    }
    let clock = super::sys::clock_text();
    if !app.is_active() {
        // Anello della batteria all'estrema sinistra, ora (con la data, se scelta)
        // al centro, anello del download all'estrema destra.
        let y = (CLOSED_H - 15.0) / 2.0;
        if let Some(battery) = app.battery {
            battery_ring(g, Rect::new(notch.x + 14.0, y, 15.0, 15.0), battery, alpha)?;
        }
        if app.showing_download() {
            let r = Rect::new(notch.right() - 14.0 - 15.0, y, 15.0, 15.0);
            download_ring(g, r, app.download_turn(), alpha)?;
        }
        let band = Rect::new(notch.x, 0.0, notch.w, CLOSED_H);
        text(g, &app.idle_text(), &g.fonts.clock, band, TEXT, alpha);
        return Ok(());
    }

    let l = closed_layout(notch);
    let focus = app.pomodoro.is_active();
    let now = time::now_ms();
    let durations = Durations::from_settings(&app.settings);

    // Sinistra: copertina, oppure il cronometro del Focus.
    match app.media.as_ref().filter(|m| m.has_track()) {
        Some(media) => match &media.art {
            Some(art) => image(g, art, l.left, 5.0, alpha, false)?,
            None => {
                fill_round(g, l.left, 5.0, HOVER, alpha);
                glyph(g, GLYPH_MUSIC, &g.fonts.icon_small, l.left, TEXT_DIM, alpha);
            }
        },
        None => {
            let color = if app.pomodoro.is_running() {
                RED
            } else {
                TEXT_DIM
            };
            glyph(g, GLYPH_STOPWATCH, &g.fonts.icon, l.left, color, alpha);
        }
    }

    // Centro: riga del testo, conto alla rovescia oppure orologio.
    if let Some(line) = app.pill_line() {
        text(g, line, &g.fonts.pill, l.center, TEXT, alpha);
    } else if focus {
        let countdown = time::fmt_countdown(app.pomodoro.remaining(now, &durations));
        let color = if app.pomodoro.is_running() {
            TEXT
        } else {
            TEXT_DIM
        };
        text(g, &countdown, &g.fonts.clock, l.center, color, alpha);
    } else {
        text(g, &clock, &g.fonts.clock, l.center, TEXT, alpha);
    }

    // Destra: anello del Focus, del download oppure le barre della musica.
    if focus {
        let color = if app.pomodoro.is_running() {
            RED
        } else {
            TEXT_DIM
        };
        ring(
            g,
            l.right,
            1.0 - app.pomodoro.progress(now, &durations),
            color,
            alpha,
        )?;
    } else if app.showing_download() {
        download_ring(g, l.right.centered(15.0), app.download_turn(), alpha)?;
    } else if let Some(media) = &app.media {
        draw_bars(g, l.right, media.playing, alpha);
    }
    Ok(())
}

/// Anello verde della carica (giallo sotto il 20%, rosso sotto il 10%).
pub(super) fn battery_ring(g: &Gfx, r: Rect, battery: Battery, alpha: f32) -> Result<()> {
    let color = match battery.percent {
        _ if battery.charging => GREEN,
        0..10 => RED,
        10..20 => YELLOW,
        _ => GREEN,
    };
    let track = with_alpha(color, 0.25);
    arc(g, r, 2.5, 1.0, track, alpha)?;
    arc(g, r, 2.5, battery.percent as f32 / 100.0, color, alpha)
}

/// Download in corso: anello blu con un arco che gira (pieno e fermo se le
/// animazioni di Windows sono spente) e una freccia al centro. Senza percentuale:
/// i browser non dicono a nessun altro quanto manca.
pub(super) fn download_ring(g: &Gfx, r: Rect, turn: Option<f32>, alpha: f32) -> Result<()> {
    match turn {
        Some(turn) => {
            arc(g, r, 2.5, 1.0, with_alpha(BLUE, 0.25), alpha)?;
            sweep(g, r, 2.5, -90.0 + 360.0 * turn, 110.0, BLUE, alpha)?;
        }
        None => arc(g, r, 2.5, 1.0, BLUE, alpha)?,
    }
    // Freccia verso il basso: asta e punta.
    let (cx, cy) = (r.center_x(), r.center_y());
    let s = r.w / 15.0;
    polyline(
        g,
        &[(cx, cy - 3.0 * s), (cx, cy + 2.5 * s)],
        1.5,
        TEXT,
        alpha,
    )?;
    polyline(
        g,
        &[(cx - 2.5 * s, cy), (cx, cy + 2.5 * s), (cx + 2.5 * s, cy)],
        1.5,
        TEXT,
        alpha,
    )
}

/// Spezzata con estremi e angoli arrotondati.
fn polyline(
    g: &Gfx,
    points: &[(f32, f32)],
    width: f32,
    color: D2D1_COLOR_F,
    alpha: f32,
) -> Result<()> {
    let Some((&(x, y), rest)) = points.split_first() else {
        return Ok(());
    };
    unsafe {
        g.brush.SetColor(&with_alpha(color, alpha));
        let path = g.factory.CreatePathGeometry()?;
        let sink = path.Open()?;
        sink.BeginFigure(v2(x, y), D2D1_FIGURE_BEGIN_HOLLOW);
        for &(x, y) in rest {
            sink.AddLine(v2(x, y));
        }
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close()?;
        g.dc.DrawGeometry(&path, &g.brush, width, &g.round);
    }
    Ok(())
}

/// Tre barrette: in riproduzione sono alte e colorate, in pausa basse e grigie.
fn draw_bars(g: &Gfx, r: Rect, playing: bool, alpha: f32) {
    let heights: [f32; 3] = if playing { [7.0, 13.0, 9.0] } else { [4.0; 3] };
    let color = if playing { PINK } else { TEXT_DIM };
    let (bar_w, gap) = (3.0, 2.5);
    let x = r.center_x() - (3.0 * bar_w + 2.0 * gap) / 2.0;
    for (i, h) in heights.iter().enumerate() {
        let bar = Rect::new(
            x + i as f32 * (bar_w + gap),
            r.center_y() - h / 2.0,
            bar_w,
            *h,
        );
        fill_round(g, bar, bar_w / 2.0, color, alpha);
    }
}

/// Tinta dell'avviso, come i colori delle app di sistema.
fn alert_color(alert: &Alert) -> D2D1_COLOR_F {
    match alert.kind {
        AlertKind::Volume | AlertKind::Keyboard => TEXT,
        AlertKind::Brightness => YELLOW,
        AlertKind::Power if alert.title.contains("scarica") => RED,
        AlertKind::Power => GREEN,
        AlertKind::Network | AlertKind::Bluetooth => BLUE,
        AlertKind::Focus => RED,
        AlertKind::Water => LIGHT_BLUE,
        AlertKind::Todo => GREEN,
    }
}

fn draw_alert(g: &mut Gfx, app: &App, alert: &Alert, alpha: f32) -> Result<()> {
    let shape = AlertShape::of(alert);
    let l = alert_layout(shape);
    let color = alert_color(alert);
    fill_round(g, l.icon, l.icon.w / 2.0, with_alpha(color, 0.18), alpha);
    glyph(g, alert.glyph, &g.fonts.icon, l.icon, color, alpha);
    match shape {
        AlertShape::Level => {
            let level = alert.level.unwrap_or(0.0);
            text(g, &alert.title, &g.fonts.alert_title, l.title, TEXT, alpha);
            progress(g, l.bar, level, color, alpha);
            text(
                g,
                &format!("{:.0}%", level * 100.0),
                &g.fonts.value_right,
                l.value,
                TEXT_DIM,
                alpha,
            );
        }
        AlertShape::Text | AlertShape::Actions(_) => {
            text(g, &alert.title, &g.fonts.alert_title, l.title, TEXT, alpha);
            text(
                g,
                &alert.detail,
                &g.fonts.alert_detail,
                l.detail,
                TEXT_DIM,
                alpha,
            );
            for (i, (label, _)) in alert.actions.iter().enumerate() {
                if let Some(r) = l.buttons.get(i) {
                    let hot = app.hot == Some(Hit::AlertAction(i));
                    let fill = if i == 0 { Some(color) } else { None };
                    pill(g, *r, None, label, fill, hot, alpha);
                }
            }
        }
    }
    Ok(())
}

// ---- Primitive di disegno ---------------------------------------------------------------

pub(super) fn text(
    g: &Gfx,
    s: &str,
    font: &IDWriteTextFormat,
    r: Rect,
    color: D2D1_COLOR_F,
    alpha: f32,
) {
    if s.is_empty() || r.w <= 0.0 {
        return;
    }
    let wide: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        g.brush.SetColor(&with_alpha(color, alpha));
        g.dc.DrawText(
            &wide,
            font,
            &d2d_rect(r),
            &g.brush,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
}

pub(super) fn glyph(
    g: &Gfx,
    c: char,
    font: &IDWriteTextFormat,
    r: Rect,
    color: D2D1_COLOR_F,
    alpha: f32,
) {
    let c = g.icon_map.map_or(c, |map| map(c));
    text(g, c.encode_utf8(&mut [0; 4]), font, r, color, alpha);
}

pub(super) fn fill_round(g: &Gfx, r: Rect, radius: f32, color: D2D1_COLOR_F, alpha: f32) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    unsafe {
        g.brush.SetColor(&with_alpha(color, alpha));
        g.dc.FillRoundedRectangle(&rounded(r, radius), &g.brush);
    }
}

pub(super) fn stroke_round(
    g: &Gfx,
    r: Rect,
    radius: f32,
    color: D2D1_COLOR_F,
    alpha: f32,
    width: f32,
    dashed: bool,
) {
    unsafe {
        g.brush.SetColor(&with_alpha(color, alpha));
        let style = dashed.then_some(&g.dash);
        g.dc.DrawRoundedRectangle(
            &rounded(r.inset(width / 2.0), radius),
            &g.brush,
            width,
            style,
        );
    }
}

/// Pulsante rotondo con glifo; acceso quando il mouse ci passa sopra.
pub(super) fn icon_button(
    g: &Gfx,
    r: Rect,
    c: char,
    font: &IDWriteTextFormat,
    hot: bool,
    color: D2D1_COLOR_F,
    alpha: f32,
) {
    if hot {
        fill_round(g, r, r.w.min(r.h) / 2.0, HOVER, alpha);
    }
    glyph(g, c, font, r, color, alpha);
}

/// Pulsante a pillola nello stile di iOS: grigio, oppure pieno di `tint` con
/// testo bianco; con un glifo facoltativo prima del testo.
pub(super) fn pill(
    g: &Gfx,
    r: Rect,
    icon: Option<char>,
    label: &str,
    tint: Option<D2D1_COLOR_F>,
    hot: bool,
    alpha: f32,
) {
    let fill = match (tint, hot) {
        (Some(c), false) => c,
        (Some(c), true) => brighter(c),
        (None, false) => HOVER,
        (None, true) => PRESSED,
    };
    fill_round(g, r, r.h / 2.0, fill, alpha);
    let label_w = g.measure(label, &g.fonts.button);
    let icon_w = if icon.is_some() { 19.0 } else { 0.0 };
    let x = r.center_x() - (label_w + icon_w) / 2.0;
    if let Some(c) = icon {
        glyph(
            g,
            c,
            &g.fonts.icon_small,
            Rect::new(x, r.y, 14.0, r.h),
            TEXT,
            alpha,
        );
    }
    text(
        g,
        label,
        &g.fonts.button,
        Rect::new(x + icon_w - 10.0, r.y, label_w + 20.0, r.h),
        TEXT,
        alpha,
    );
}

/// Un colore un po' più chiaro, per lo stato "mouse sopra".
pub(super) fn brighter(c: D2D1_COLOR_F) -> D2D1_COLOR_F {
    rgba(
        c.r + (1.0 - c.r) * 0.18,
        c.g + (1.0 - c.g) * 0.18,
        c.b + (1.0 - c.b) * 0.18,
        c.a,
    )
}

/// Barra orizzontale arrotondata con riempimento proporzionale.
pub(super) fn progress(g: &Gfx, r: Rect, fraction: f32, color: D2D1_COLOR_F, alpha: f32) {
    fill_round(g, r, r.h / 2.0, TRACK, alpha);
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        fill_round(
            g,
            r.with_width((r.w * fraction).max(r.h)),
            r.h / 2.0,
            color,
            alpha,
        );
    }
}

/// Anello di avanzamento sottile (in senso orario dall'alto), con la traccia.
pub(super) fn ring(g: &Gfx, r: Rect, fraction: f32, color: D2D1_COLOR_F, alpha: f32) -> Result<()> {
    arc(g, r, 2.5, 1.0, TRACK, alpha)?;
    arc(g, r, 2.5, fraction, color, alpha)
}

/// Arco di cerchio inscritto in `r`, dall'alto in senso orario, per la frazione data.
pub(super) fn arc(
    g: &Gfx,
    r: Rect,
    width: f32,
    fraction: f32,
    color: D2D1_COLOR_F,
    alpha: f32,
) -> Result<()> {
    sweep(
        g,
        r,
        width,
        -90.0,
        360.0 * fraction.clamp(0.0, 1.0),
        color,
        alpha,
    )
}

/// Arco con estremi arrotondati da `start` gradi (0 = destra, -90 = alto) per `degrees` gradi.
pub(super) fn sweep(
    g: &Gfx,
    r: Rect,
    width: f32,
    start: f32,
    degrees: f32,
    color: D2D1_COLOR_F,
    alpha: f32,
) -> Result<()> {
    let radius = r.w.min(r.h) / 2.0 - width / 2.0;
    let (cx, cy) = (r.center_x(), r.center_y());
    if degrees <= 0.1 || radius <= 0.0 {
        return Ok(());
    }
    unsafe {
        g.brush.SetColor(&with_alpha(color, alpha));
        if degrees >= 359.9 {
            let ellipse = D2D1_ELLIPSE {
                point: v2(cx, cy),
                radiusX: radius,
                radiusY: radius,
            };
            g.dc.DrawEllipse(&ellipse, &g.brush, width, None);
            return Ok(());
        }
        let point = |deg: f32| {
            let a = deg.to_radians();
            v2(cx + radius * a.cos(), cy + radius * a.sin())
        };
        let path = g.factory.CreatePathGeometry()?;
        let sink = path.Open()?;
        sink.BeginFigure(point(start), D2D1_FIGURE_BEGIN_HOLLOW);
        // Archi come curve di Bézier, al massimo un quarto di cerchio per volta.
        let pieces = (degrees / 90.0).ceil().max(1.0);
        let step = degrees / pieces;
        let k = 4.0 / 3.0 * (step.to_radians() / 4.0).tan() * radius;
        let tangent = |deg: f32| {
            let a = deg.to_radians();
            (-a.sin() * k, a.cos() * k)
        };
        for i in 0..pieces as usize {
            let (a0, a1) = (start + i as f32 * step, start + (i + 1) as f32 * step);
            let (p0, p3) = (point(a0), point(a1));
            let (t0, t1) = (tangent(a0), tangent(a1));
            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: v2(p0.X + t0.0, p0.Y + t0.1),
                point2: v2(p3.X - t1.0, p3.Y - t1.1),
                point3: p3,
            });
        }
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close()?;
        g.dc.DrawGeometry(&path, &g.brush, width, &g.round);
    }
    Ok(())
}

/// Mini-grafico a linea dei valori, scalati sul massimo `max`.
pub(super) fn sparkline(
    g: &Gfx,
    r: Rect,
    values: &[f32],
    max: f32,
    color: D2D1_COLOR_F,
    alpha: f32,
) -> Result<()> {
    if values.len() < 2 || max <= 0.0 {
        return Ok(());
    }
    let step = r.w / (values.len() - 1) as f32;
    let point = |i: usize, v: f32| {
        v2(
            r.x + i as f32 * step,
            r.bottom() - (v / max).clamp(0.0, 1.0) * r.h,
        )
    };
    unsafe {
        let path = g.factory.CreatePathGeometry()?;
        let sink = path.Open()?;
        sink.BeginFigure(point(0, values[0]), D2D1_FIGURE_BEGIN_HOLLOW);
        for (i, &v) in values.iter().enumerate().skip(1) {
            sink.AddLine(point(i, v));
        }
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close()?;
        g.brush.SetColor(&with_alpha(color, alpha));
        g.dc.DrawGeometry(&path, &g.brush, 1.5, None);
    }
    Ok(())
}

/// Immagine con angoli arrotondati. Con `fit` mantiene le proporzioni dentro `r`.
pub(super) fn image(
    g: &mut Gfx,
    art: &Artwork,
    r: Rect,
    radius: f32,
    alpha: f32,
    fit: bool,
) -> Result<()> {
    let bitmap = g.image(art)?;
    let (w, h) = (art.width.max(1) as f32, art.height.max(1) as f32);
    let dest = if fit {
        let k = (r.w / w).min(r.h / h);
        Rect::new(
            r.center_x() - w * k / 2.0,
            r.center_y() - h * k / 2.0,
            w * k,
            h * k,
        )
    } else {
        r
    };
    unsafe {
        let props = D2D1_BITMAP_BRUSH_PROPERTIES1 {
            extendModeX: D2D1_EXTEND_MODE_CLAMP,
            extendModeY: D2D1_EXTEND_MODE_CLAMP,
            interpolationMode: D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
        };
        let brush = g.dc.CreateBitmapBrush(&bitmap, Some(&props), None)?;
        let scale = Matrix3x2::scale(dest.w / w, dest.h / h);
        brush.SetTransform(&(scale * Matrix3x2::translation(dest.x, dest.y)));
        brush.SetOpacity(alpha);
        g.dc.FillRoundedRectangle(&rounded(dest, radius), &brush);
    }
    Ok(())
}

pub(super) fn with_alpha(c: D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        a: c.a * alpha,
        ..c
    }
}

fn rounded(r: Rect, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: d2d_rect(r),
        radiusX: radius,
        radiusY: radius,
    }
}

pub(super) fn d2d_rect(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.x,
        top: r.y,
        right: r.right(),
        bottom: r.bottom(),
    }
}

pub(super) fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}
