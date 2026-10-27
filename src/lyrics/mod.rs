//! Testi sincronizzati: parsing LRC, ricerca su LRCLIB e stato per brano.

#[cfg(windows)]
pub mod fetch;
pub mod lrc;
pub mod lrclib;

use std::sync::Arc;

pub use lrc::Lyrics;

#[derive(Debug, Clone)]
pub enum LyricsState {
    Loading,
    Synced(Arc<Lyrics>),
    Instrumental,
    /// Esiste solo il testo senza tempi: per ora non lo mostriamo.
    Unsynced,
    Missing,
    /// Errore di rete: si riprova la prossima volta che il brano parte.
    Failed,
}
