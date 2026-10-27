//! Contenuto della finestra Impostazioni: sezioni, interruttori e pulsanti − / +.

use windows::core::Result;
use windows_numerics::Matrix3x2;

use super::*;
use crate::layout::settings::{Row, SettingsHit, SettingsLayout, field_controls};
use crate::settings::{Field, Kind, Settings};

const BACKGROUND: D2D1_COLOR_F = rgba(0.09, 0.09, 0.1, 1.0);

pub struct View<'a> {
    pub layout: &'a SettingsLayout,
    pub scroll: f32,
    pub hot: Option<SettingsHit>,
    pub settings: &'a Settings,
    pub monitors: u32,
    pub view_h: f32,
}

pub fn draw(g: &mut Gfx, v: &View) -> Result<()> {
    unsafe {
        g.dc.BeginDraw();
        g.dc.Clear(Some(&BACKGROUND));
        g.dc.SetTransform(&Matrix3x2::translation(0.0, -v.scroll));
    }
    draw_content(g, v);
    unsafe {
        g.dc.SetTransform(&Matrix3x2::identity());
        g.dc.EndDraw(None, None)?;
    }
    g.present()
}

fn draw_content(g: &Gfx, v: &View) {
    let a = 1.0;
    text(
        g,
        "Impostazioni",
        &g.fonts.settings_title,
        v.layout.title,
        TEXT,
        a,
    );
    let visible = |r: &Rect| r.bottom() >= v.scroll && r.y <= v.scroll + v.view_h;

    for (row, r) in v.layout.rows.iter().filter(|(_, r)| visible(r)) {
        match *row {
            Row::Header(section) => {
                let label = Rect::new(r.x + 4.0, r.y + 14.0, r.w, r.h - 14.0);
                text(
                    g,
                    &section.title().to_uppercase(),
                    &g.fonts.section,
                    label,
                    TEXT_DIM,
                    a,
                );
            }
            Row::Field(field) => draw_field(g, v, field, *r),
        }
    }

    let footer = format!(
        "Skylight {} · i tuoi dati restano su questo PC",
        env!("CARGO_PKG_VERSION")
    );
    text(
        g,
        &footer,
        &g.fonts.small_center,
        v.layout.footer,
        TEXT_FAINT,
        a,
    );
}

fn draw_field(g: &Gfx, v: &View, field: Field, r: Rect) {
    let a = 1.0;
    let row_hot = matches!(v.hot, Some(SettingsHit::Toggle(f) | SettingsHit::Minus(f) | SettingsHit::Plus(f)) if f == field);
    if row_hot {
        fill_round(g, r, 10.0, SURFACE, a);
    }
    let controls = field_controls(r, field.kind());
    let label_right = controls
        .toggle
        .or(controls.minus)
        .map_or(r.right(), |c| c.x)
        - 12.0;
    text(
        g,
        field.label(),
        &g.fonts.body,
        Rect::new(r.x + 12.0, r.y, label_right - r.x - 12.0, r.h),
        TEXT,
        a,
    );

    match field.kind() {
        Kind::Toggle => {
            let Some(track) = controls.toggle else { return };
            let on = field.get(v.settings) != 0;
            fill_round(
                g,
                track,
                track.h / 2.0,
                // Interruttori verdi come in iOS e macOS.
                if on { GREEN } else { PRESSED },
                a,
            );
            let knob_size = track.h - 6.0;
            let knob_x = if on {
                track.right() - 3.0 - knob_size
            } else {
                track.x + 3.0
            };
            let knob = Rect::new(knob_x, track.y + 3.0, knob_size, knob_size);
            fill_round(g, knob, knob_size / 2.0, TEXT, a);
        }
        Kind::Number { .. } | Kind::Monitor | Kind::Choice(_) => {
            let (Some(minus), Some(value), Some(plus)) =
                (controls.minus, controls.value, controls.plus)
            else {
                return;
            };
            let hot_minus = v.hot == Some(SettingsHit::Minus(field));
            let hot_plus = v.hot == Some(SettingsHit::Plus(field));
            fill_round(
                g,
                minus,
                minus.w / 2.0,
                if hot_minus { PRESSED } else { HOVER },
                a,
            );
            fill_round(
                g,
                plus,
                plus.w / 2.0,
                if hot_plus { PRESSED } else { HOVER },
                a,
            );
            glyph(g, '\u{E738}', &g.fonts.icon_small, minus, TEXT, a);
            glyph(g, '\u{E710}', &g.fonts.icon_small, plus, TEXT, a);
            let label = match field.kind() {
                Kind::Monitor if v.monitors <= 1 && field.get(v.settings) == 0 => {
                    "Principale".to_string()
                }
                _ => field.format(v.settings),
            };
            text(g, &label, &g.fonts.body_center, value, TEXT, a);
        }
    }
}
