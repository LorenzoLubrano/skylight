//! Stato della musica in riproduzione, indipendente dalla piattaforma.

#[cfg(windows)]
pub mod gsmtc;

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

/// Copertina già decodificata: pixel BGRA premoltiplicati, pronti per la GPU.
pub struct Artwork {
    /// Cambia a ogni nuova immagine: serve all'interfaccia per sapere quando ricaricarla.
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct MediaSnapshot {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// Posizione (ms) valida all'istante `updated_at`.
    pub position_ms: u64,
    pub duration_ms: u64,
    /// Istante dell'ultima posizione nota, in unità FILETIME (100 ns dal 1601).
    pub updated_at: i64,
    pub rate: f64,
    pub art: Option<Arc<Artwork>>,
}

impl MediaSnapshot {
    pub fn has_track(&self) -> bool {
        !self.title.is_empty()
    }

    /// Posizione stimata all'istante `now`: i player non aggiornano la posizione
    /// di continuo, quindi la si calcola dal tempo trascorso.
    pub fn position_at(&self, now: i64) -> u64 {
        let mut pos = self.position_ms as f64;
        if self.playing && self.updated_at > 0 && now > self.updated_at {
            pos += (now - self.updated_at) as f64 / 10_000.0 * self.rate.max(0.0);
        }
        let pos = pos.max(0.0) as u64;
        if self.duration_ms > 0 {
            pos.min(self.duration_ms)
        } else {
            pos
        }
    }

    /// Fissa la posizione stimata a `now`: va fatto prima di cambiare stato
    /// (pausa/riproduzione) o dopo un salto, per non accumulare errori.
    pub fn rebase(&mut self, now: i64) {
        self.position_ms = self.position_at(now);
        self.updated_at = now;
    }

    /// Identità del brano, usata come chiave per la cache dei testi.
    pub fn track_key(&self) -> u64 {
        let mut h = DefaultHasher::new();
        (self.title.as_str(), self.artist.as_str()).hash(&mut h);
        h.finish()
    }
}

#[derive(Clone, Copy, Debug)]
pub enum MediaCommand {
    TogglePlayPause,
    Next,
    Previous,
    /// Posizione in millisecondi dall'inizio del brano.
    Seek(u64),
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: i64 = 10_000_000; // in unità FILETIME

    fn snapshot(playing: bool) -> MediaSnapshot {
        MediaSnapshot {
            title: "Paper Sky".into(),
            playing,
            position_ms: 10_000,
            duration_ms: 60_000,
            updated_at: 100 * SECOND,
            rate: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn extrapolates_only_while_playing() {
        assert_eq!(snapshot(true).position_at(105 * SECOND), 15_000);
        assert_eq!(snapshot(false).position_at(105 * SECOND), 10_000);
    }

    #[test]
    fn respects_rate_and_duration() {
        let s = MediaSnapshot {
            rate: 2.0,
            ..snapshot(true)
        };
        assert_eq!(s.position_at(105 * SECOND), 20_000);
        assert_eq!(snapshot(true).position_at(1_000 * SECOND), 60_000);
    }

    #[test]
    fn clock_going_backwards_does_not_rewind() {
        assert_eq!(snapshot(true).position_at(50 * SECOND), 10_000);
    }

    #[test]
    fn rebase_freezes_the_estimate() {
        let mut s = snapshot(true);
        s.rebase(103 * SECOND);
        s.playing = false;
        assert_eq!(s.position_at(200 * SECOND), 13_000);
    }

    #[test]
    fn track_key_depends_on_title_and_artist() {
        let a = snapshot(true);
        let b = MediaSnapshot {
            position_ms: 0,
            ..snapshot(false)
        };
        let c = MediaSnapshot {
            artist: "Altro".into(),
            ..snapshot(true)
        };
        assert_eq!(a.track_key(), b.track_key());
        assert_ne!(a.track_key(), c.track_key());
    }
}
