//! Pagina Musica: copertina, titolo, avanzamento, controlli e testo sincronizzato.
//! Senza musica la pagina resta uguale, con "Nessuna riproduzione".

use super::{CONTENT_W, HEADER_H, Hit, Rect, content_x};

/// Altezza del contenuto (intestazione e piè di pagina esclusi).
pub const MUSIC_H: f32 = 112.0;
/// Spazio in più quando il testo sincronizzato è attivo.
pub const LYRICS_H: f32 = 78.0;

/// Il layout è ancorato alla dimensione finale: durante l'animazione il
/// contenuto resta fermo e il notch che si allarga lo "svela".
pub struct MusicLayout {
    pub art: Rect,
    pub title: Rect,
    pub artist: Rect,
    pub bar: Rect,
    pub bar_hit: Rect,
    pub time_left: Rect,
    pub time_right: Rect,
    pub prev: Rect,
    pub play: Rect,
    pub next: Rect,
    pub lyrics_btn: Rect,
    pub volume_btn: Rect,
    pub lyric_lines: [Rect; 3],
}

impl MusicLayout {
    pub fn new() -> Self {
        let (x, top) = (content_x(), HEADER_H);
        let art = Rect::new(x, top + 2.0, 54.0, 54.0);
        let tx = art.right() + 12.0;
        let tw = x + CONTENT_W - tx;

        let bar = Rect::new(x + 46.0, top + 70.0, CONTENT_W - 92.0, 4.0);
        let row = top + 94.0;
        let button = |cx: f32, size: f32| Rect::new(cx - size / 2.0, row - size / 2.0, size, size);
        let cx = x + CONTENT_W / 2.0;
        let lyrics = top + MUSIC_H + 2.0;

        Self {
            art,
            title: Rect::new(tx, top + 8.0, tw, 20.0),
            artist: Rect::new(tx, top + 29.0, tw, 18.0),
            bar,
            bar_hit: Rect::new(bar.x - 4.0, bar.y - 8.0, bar.w + 8.0, 20.0),
            time_left: Rect::new(x, top + 64.0, 42.0, 16.0),
            time_right: Rect::new(x + CONTENT_W - 42.0, top + 64.0, 42.0, 16.0),
            prev: button(cx - 46.0, 30.0),
            play: button(cx, 34.0),
            next: button(cx + 46.0, 30.0),
            lyrics_btn: button(x + 14.0, 28.0),
            volume_btn: button(x + CONTENT_W - 14.0, 28.0),
            lyric_lines: [
                Rect::new(x, lyrics, CONTENT_W, 20.0),
                Rect::new(x, lyrics + 22.0, CONTENT_W, 28.0),
                Rect::new(x, lyrics + 52.0, CONTENT_W, 20.0),
            ],
        }
    }

    /// `lines`: quali righe del testo (precedente, attuale, successiva) sono disegnate.
    /// `volume_mode`: la barra regola il volume invece della posizione nel brano.
    pub fn hit(&self, x: f32, y: f32, lines: [bool; 3], volume_mode: bool) -> Option<Hit> {
        let targets = [
            (self.prev, Hit::Prev),
            (self.play, Hit::PlayPause),
            (self.next, Hit::Next),
            (self.lyrics_btn, Hit::Lyrics),
            (self.volume_btn, Hit::Volume),
            (
                self.bar_hit,
                if volume_mode {
                    Hit::VolumeBar
                } else {
                    Hit::Seek
                },
            ),
        ];
        if let Some((_, hit)) = targets.iter().find(|(r, _)| r.contains(x, y)) {
            return Some(*hit);
        }
        self.lyric_lines
            .iter()
            .zip(lines)
            .enumerate()
            .find(|(_, (r, shown))| *shown && r.contains(x, y))
            .map(|(i, _)| Hit::LyricLine(i as i8 - 1))
    }

    /// Punto della barra di avanzamento sotto `x`, tra 0 e 1.
    pub fn seek_fraction(&self, x: f32) -> f32 {
        ((x - self.bar.x) / self.bar.w).clamp(0.0, 1.0)
    }
}

impl Default for MusicLayout {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_fits_inside_the_page() {
        let l = MusicLayout::new();
        let (left, right) = (content_x(), content_x() + CONTENT_W);
        for r in [
            l.art,
            l.title,
            l.artist,
            l.bar_hit,
            l.time_left,
            l.time_right,
            l.prev,
            l.play,
            l.next,
            l.lyrics_btn,
            l.volume_btn,
        ] {
            assert!(
                r.x >= left - 4.0 && r.right() <= right + 4.0,
                "{r:?} esce ai lati"
            );
            assert!(
                r.y >= HEADER_H && r.bottom() <= HEADER_H + MUSIC_H,
                "{r:?} esce in basso"
            );
        }
        for r in l.lyric_lines {
            assert!(r.y >= HEADER_H + MUSIC_H && r.bottom() <= HEADER_H + MUSIC_H + LYRICS_H);
        }
        let buttons = [l.prev, l.play, l.next, l.lyrics_btn, l.volume_btn];
        for (i, a) in buttons.iter().enumerate() {
            assert!(buttons[i + 1..].iter().all(|b| !a.overlaps(b)));
        }
        assert!(!l.time_left.overlaps(&l.bar) && !l.time_right.overlaps(&l.bar));
    }

    #[test]
    fn hit_testing_finds_controls() {
        let l = MusicLayout::new();
        assert_eq!(
            l.hit(l.play.center_x(), l.play.center_y(), [false; 3], false),
            Some(Hit::PlayPause)
        );
        assert_eq!(
            l.hit(l.next.center_x(), l.next.center_y(), [false; 3], false),
            Some(Hit::Next)
        );
        assert_eq!(
            l.hit(l.bar.center_x(), l.bar.center_y(), [false; 3], false),
            Some(Hit::Seek)
        );
        assert_eq!(
            l.hit(l.title.center_x(), l.title.center_y(), [false; 3], false),
            None
        );
        assert_eq!(
            l.hit(l.bar.center_x(), l.bar.center_y(), [false; 3], true),
            Some(Hit::VolumeBar)
        );
        assert_eq!(
            l.hit(
                l.volume_btn.center_x(),
                l.volume_btn.center_y(),
                [false; 3],
                false
            ),
            Some(Hit::Volume)
        );
        let line = l.lyric_lines[2];
        assert_eq!(
            l.hit(line.center_x(), line.center_y(), [false; 3], false),
            None
        );
        assert_eq!(
            l.hit(line.center_x(), line.center_y(), [true; 3], false),
            Some(Hit::LyricLine(1))
        );
    }

    #[test]
    fn seek_fraction_is_clamped() {
        let l = MusicLayout::new();
        assert_eq!(l.seek_fraction(l.bar.x - 50.0), 0.0);
        assert_eq!(l.seek_fraction(l.bar.right() + 50.0), 1.0);
        assert!((l.seek_fraction(l.bar.center_x()) - 0.5).abs() < 1e-4);
    }
}
