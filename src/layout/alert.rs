//! Avvisi nel notch chiuso: una riga con barra di livello (volume, luminosità,
//! batteria), oppure titolo e dettaglio, con eventuali pulsanti sotto.

use super::{Hit, Rect, notch_rect};
use crate::features::alerts::Alert;

pub const ALERT_W: f32 = 360.0;
const LEVEL_H: f32 = 48.0;
const TEXT_H: f32 = 62.0;
const ACTIONS_H: f32 = 106.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertShape {
    /// Icona, titolo breve, barra e percentuale.
    Level,
    /// Icona, titolo e dettaglio.
    Text,
    /// Come `Text`, con una fila di pulsanti.
    Actions(usize),
}

impl AlertShape {
    pub fn of(alert: &Alert) -> Self {
        if alert.is_actionable() {
            AlertShape::Actions(alert.actions.len())
        } else if alert.level.is_some() && alert.detail.is_empty() {
            AlertShape::Level
        } else {
            AlertShape::Text
        }
    }

    pub fn height(self) -> f32 {
        match self {
            AlertShape::Level => LEVEL_H,
            AlertShape::Text => TEXT_H,
            AlertShape::Actions(_) => ACTIONS_H,
        }
    }
}

pub struct AlertLayout {
    pub icon: Rect,
    pub title: Rect,
    pub detail: Rect,
    pub bar: Rect,
    pub value: Rect,
    pub buttons: Vec<Rect>,
}

/// Ancorato al riquadro finale dell'avviso (centrato in alto).
pub fn alert_layout(shape: AlertShape) -> AlertLayout {
    let r = notch_rect(ALERT_W, shape.height());
    match shape {
        AlertShape::Level => {
            let cy = r.h / 2.0;
            AlertLayout {
                icon: Rect::new(r.x + 16.0, cy - 14.0, 28.0, 28.0),
                title: Rect::new(r.x + 52.0, 0.0, 96.0, r.h),
                detail: Rect::default(),
                bar: Rect::new(r.x + 152.0, cy - 3.0, r.w - 152.0 - 62.0, 6.0),
                value: Rect::new(r.right() - 56.0, 0.0, 42.0, r.h),
                buttons: Vec::new(),
            }
        }
        AlertShape::Text | AlertShape::Actions(_) => {
            let n = if let AlertShape::Actions(n) = shape {
                n.min(2)
            } else {
                0
            };
            let (bw, gap) = (140.0, 8.0);
            let buttons = (0..n)
                .map(|i| {
                    let from_right = (n - i) as f32;
                    Rect::new(
                        r.right() - 16.0 - from_right * bw - (from_right - 1.0) * gap,
                        64.0,
                        bw,
                        30.0,
                    )
                })
                .collect();
            AlertLayout {
                icon: Rect::new(r.x + 16.0, 15.0, 32.0, 32.0),
                title: Rect::new(r.x + 60.0, 11.0, r.w - 76.0, 20.0),
                detail: Rect::new(r.x + 60.0, 32.0, r.w - 76.0, 18.0),
                bar: Rect::default(),
                value: Rect::default(),
                buttons,
            }
        }
    }
}

pub fn alert_hit(shape: AlertShape, x: f32, y: f32) -> Option<Hit> {
    let layout = alert_layout(shape);
    if let Some(i) = layout.buttons.iter().position(|b| b.contains(x, y)) {
        return Some(Hit::AlertAction(i));
    }
    notch_rect(ALERT_W, shape.height())
        .contains(x, y)
        .then_some(Hit::AlertBody)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_are_right_aligned_and_inside() {
        let shape = AlertShape::Actions(2);
        let l = alert_layout(shape);
        let r = notch_rect(ALERT_W, shape.height());
        assert_eq!(l.buttons.len(), 2);
        assert!(l.buttons[0].right() < l.buttons[1].x);
        assert!((l.buttons[1].right() - (r.right() - 16.0)).abs() < 1e-3);
        assert!(
            l.buttons
                .iter()
                .all(|b| b.x > r.x && b.bottom() < r.bottom())
        );
        let b = l.buttons[1];
        assert_eq!(
            alert_hit(shape, b.center_x(), b.center_y()),
            Some(Hit::AlertAction(1))
        );
        assert_eq!(
            alert_hit(shape, l.icon.center_x(), l.icon.center_y()),
            Some(Hit::AlertBody)
        );
    }

    #[test]
    fn level_bar_fits() {
        let l = alert_layout(AlertShape::Level);
        assert!(l.bar.w > 100.0);
        assert!(l.bar.right() < l.value.x);
        assert!(l.title.right() <= l.bar.x);
    }
}
