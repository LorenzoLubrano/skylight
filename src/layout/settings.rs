//! Finestra Impostazioni: titolo, sezioni e una riga per impostazione, con
//! interruttore oppure pulsanti − / + ai lati del valore.

use super::Rect;
use crate::settings::{Field, Kind, Section};

pub const SETTINGS_W: f32 = 480.0;
pub const SETTINGS_H: f32 = 640.0;
const MARGIN: f32 = 24.0;
const TITLE_H: f32 = 76.0;
const HEADER_H: f32 = 40.0;
const ROW_H: f32 = 44.0;
const FOOTER_H: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Header(Section),
    Field(Field),
}

pub struct SettingsLayout {
    pub title: Rect,
    pub rows: Vec<(Row, Rect)>,
    pub footer: Rect,
    /// Altezza totale del contenuto (si scorre se supera la finestra).
    pub content_h: f32,
}

pub fn settings_layout() -> SettingsLayout {
    let inner = SETTINGS_W - 2.0 * MARGIN;
    let mut y = TITLE_H;
    let mut rows = Vec::new();
    let mut section = None;
    for field in Field::ALL {
        if section != Some(field.section()) {
            if section.is_some() {
                y += 10.0;
            }
            section = Some(field.section());
            rows.push((
                Row::Header(field.section()),
                Rect::new(MARGIN, y, inner, HEADER_H),
            ));
            y += HEADER_H;
        }
        rows.push((Row::Field(field), Rect::new(MARGIN, y, inner, ROW_H)));
        y += ROW_H;
    }
    SettingsLayout {
        title: Rect::new(MARGIN, 20.0, inner, 40.0),
        rows,
        footer: Rect::new(MARGIN, y + 16.0, inner, 28.0),
        content_h: y + FOOTER_H,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FieldControls {
    pub toggle: Option<Rect>,
    pub minus: Option<Rect>,
    pub value: Option<Rect>,
    pub plus: Option<Rect>,
}

pub fn field_controls(row: Rect, kind: Kind) -> FieldControls {
    let cy = row.center_y();
    match kind {
        Kind::Toggle => FieldControls {
            toggle: Some(Rect::new(row.right() - 46.0, cy - 12.0, 46.0, 24.0)),
            ..Default::default()
        },
        Kind::Number { .. } | Kind::Monitor | Kind::Choice(_) => {
            let plus = Rect::new(row.right() - 30.0, cy - 15.0, 30.0, 30.0);
            let value = Rect::new(plus.x - 108.0, row.y, 108.0, row.h);
            let minus = Rect::new(value.x - 30.0, cy - 15.0, 30.0, 30.0);
            FieldControls {
                toggle: None,
                minus: Some(minus),
                value: Some(value),
                plus: Some(plus),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsHit {
    Toggle(Field),
    Minus(Field),
    Plus(Field),
}

/// `y` è già nelle coordinate del contenuto (scorrimento compreso).
pub fn settings_hit(layout: &SettingsLayout, x: f32, y: f32) -> Option<SettingsHit> {
    layout.rows.iter().find_map(|(row, rect)| {
        let Row::Field(field) = *row else { return None };
        if !rect.contains(x, y) {
            return None;
        }
        let c = field_controls(*rect, field.kind());
        if c.toggle.is_some() {
            // Tutta la riga è cliccabile: bersaglio più comodo.
            return Some(SettingsHit::Toggle(field));
        }
        if c.minus.is_some_and(|r| r.contains(x, y)) {
            Some(SettingsHit::Minus(field))
        } else if c.plus.is_some_and(|r| r.contains(x, y)) {
            Some(SettingsHit::Plus(field))
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_stacked_without_overlap() {
        let l = settings_layout();
        assert_eq!(
            l.rows
                .iter()
                .filter(|(r, _)| matches!(r, Row::Field(_)))
                .count(),
            Field::ALL.len()
        );
        assert_eq!(
            l.rows
                .iter()
                .filter(|(r, _)| matches!(r, Row::Header(_)))
                .count(),
            4
        );
        for pair in l.rows.windows(2) {
            assert!(pair[0].1.bottom() <= pair[1].1.y);
        }
        assert!(l.footer.bottom() <= l.content_h);
    }

    #[test]
    fn controls_and_hits() {
        let l = settings_layout();
        let (_, toggle_row) = l
            .rows
            .iter()
            .find(|(r, _)| *r == Row::Field(Field::Lyrics))
            .unwrap();
        assert_eq!(
            settings_hit(&l, toggle_row.x + 5.0, toggle_row.center_y()),
            Some(SettingsHit::Toggle(Field::Lyrics))
        );

        let (_, number_row) = l
            .rows
            .iter()
            .find(|(r, _)| *r == Row::Field(Field::FocusMin))
            .unwrap();
        let c = field_controls(*number_row, Field::FocusMin.kind());
        let (minus, value, plus) = (c.minus.unwrap(), c.value.unwrap(), c.plus.unwrap());
        assert!(
            minus.right() <= value.x
                && value.right() <= plus.x
                && plus.right() <= number_row.right()
        );
        assert_eq!(
            settings_hit(&l, plus.center_x(), plus.center_y()),
            Some(SettingsHit::Plus(Field::FocusMin))
        );
        assert_eq!(
            settings_hit(&l, minus.center_x(), minus.center_y()),
            Some(SettingsHit::Minus(Field::FocusMin))
        );
        assert_eq!(
            settings_hit(&l, number_row.x + 5.0, number_row.center_y()),
            None
        );
    }
}
