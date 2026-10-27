//! Geometria del notch: dimensioni, posizione degli elementi e hit-testing.
//!
//! Tutte le misure sono in DIP (pixel indipendenti dalla scala dello schermo):
//! a 150% di scala 1 DIP = 1,5 pixel. La finestra è una "tela" trasparente
//! larga `CANVAS_W`, centrata in cima al monitor; il notch è disegnato al centro.

pub mod alert;
pub mod music;
pub mod pages;
pub mod settings;

// Fuori da Windows alcune parti servono solo ai test.
#[cfg_attr(not(windows), allow(unused_imports))]
pub use alert::*;
#[cfg_attr(not(windows), allow(unused_imports))]
pub use music::*;
pub use pages::*;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center_x(&self) -> f32 {
        self.x + self.w / 2.0
    }

    pub fn center_y(&self) -> f32 {
        self.y + self.h / 2.0
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn offset_y(&self, dy: f32) -> Self {
        Self {
            y: self.y + dy,
            ..*self
        }
    }

    pub fn with_width(&self, w: f32) -> Self {
        Self { w, ..*self }
    }

    pub fn inset(&self, d: f32) -> Self {
        Self::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }

    /// Quadrato di lato `size` centrato nel rettangolo.
    pub fn centered(&self, size: f32) -> Self {
        Self::new(
            self.center_x() - size / 2.0,
            self.center_y() - size / 2.0,
            size,
            size,
        )
    }

    #[cfg(test)]
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
}

pub const CANVAS_W: f32 = 620.0;
/// Più alta della pagina più alta: la molla dell'altezza rimbalza un po' oltre.
pub const CANVAS_H: f32 = 264.0;
/// Margine da lasciare sotto la pagina più alta per quel rimbalzo.
#[cfg(test)]
pub const BOUNCE_ROOM: f32 = 12.0;

/// Raccordo concavo agli angoli superiori, dove il notch si unisce al bordo.
pub const EAR: f32 = 7.0;
pub const RADIUS_CLOSED: f32 = 11.0;
pub const RADIUS_OPEN: f32 = 24.0;

pub const CLOSED_H: f32 = 32.0;
pub const CLOSED_IDLE_W: f32 = 168.0;
pub const CLOSED_PLAYING_W: f32 = 236.0;
/// Spazio ai lati del testo nella pillola chiusa: icona a sinistra, indicatore a destra.
pub const PILL_SIDE: f32 = 44.0;
pub const PILL_MAX_W: f32 = 580.0;

/// Larghezza del notch aperto (uguale per tutte le pagine) e margine laterale
/// del contenuto: ai lati ci sono le frecce per cambiare pagina.
pub const OPEN_W: f32 = 456.0;
pub const PAD: f32 = 38.0;
/// Larghezza utile del contenuto delle pagine.
pub const CONTENT_W: f32 = OPEN_W - 2.0 * PAD;
/// Intestazione del notch aperto (titolo, batteria, data e ora) e piè di pagina
/// (pallini delle pagine, che diventano la barra delle icone col mouse sopra).
pub const HEADER_H: f32 = 32.0;
pub const FOOTER_H: f32 = 26.0;

/// Notch ridotto mentre un'app è a schermo intero: una linguetta appena visibile.
pub const MINI_W: f32 = 112.0;
pub const MINI_H: f32 = 7.0;

/// Larghezza della pillola chiusa che mostra una riga di testo larga `text_w`.
pub fn pill_width(text_w: f32) -> f32 {
    (text_w + 2.0 * PILL_SIDE + 8.0).clamp(CLOSED_PLAYING_W, PILL_MAX_W)
}

/// Il notch è centrato orizzontalmente nella tela e attaccato al bordo superiore.
pub fn notch_rect(w: f32, h: f32) -> Rect {
    Rect::new((CANVAS_W - w) / 2.0, 0.0, w, h.min(CANVAS_H))
}

/// Bordo sinistro del notch aperto.
pub fn open_x() -> f32 {
    (CANVAS_W - OPEN_W) / 2.0
}

/// Bordo sinistro del contenuto delle pagine.
pub fn content_x() -> f32 {
    open_x() + PAD
}

/// Altezza totale del notch aperto con una pagina alta `content_h`.
pub fn open_height(content_h: f32) -> f32 {
    HEADER_H + content_h + FOOTER_H
}

/// Elementi della vista chiusa, ancorati al notch attuale (che cambia larghezza).
pub struct ClosedLayout {
    pub left: Rect,
    pub center: Rect,
    pub right: Rect,
}

pub fn closed_layout(notch: Rect) -> ClosedLayout {
    ClosedLayout {
        left: Rect::new(notch.x + 14.0, (CLOSED_H - 20.0) / 2.0, 20.0, 20.0),
        center: Rect::new(
            notch.x + PILL_SIDE,
            0.0,
            (notch.w - 2.0 * PILL_SIDE).max(0.0),
            CLOSED_H,
        ),
        right: Rect::new(
            notch.right() - 14.0 - 18.0,
            (CLOSED_H - 18.0) / 2.0,
            18.0,
            18.0,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pill_width_is_bounded_and_fits_the_canvas() {
        assert_eq!(pill_width(0.0), CLOSED_PLAYING_W);
        assert_eq!(pill_width(10_000.0), PILL_MAX_W);
        const { assert!(PILL_MAX_W + 2.0 * EAR <= CANVAS_W) };
        const { assert!(OPEN_W + 2.0 * EAR <= CANVAS_W) };
    }

    #[test]
    fn rect_helpers() {
        let r = Rect::new(10.0, 10.0, 100.0, 50.0);
        assert_eq!(r.inset(5.0), Rect::new(15.0, 15.0, 90.0, 40.0));
        assert_eq!(r.centered(20.0), Rect::new(50.0, 25.0, 20.0, 20.0));
        assert!(r.overlaps(&Rect::new(100.0, 50.0, 10.0, 10.0)));
        assert!(!r.overlaps(&Rect::new(110.0, 10.0, 10.0, 10.0)));
    }
}
