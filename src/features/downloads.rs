//! Download in corso. I browser scrivono il file in arrivo con un'estensione
//! provvisoria (.crdownload, .part, ...) e lo rinominano quando è finito: c'è un
//! download in corso se uno di questi file viene scritto. Uno fermo da qualche
//! secondo (in pausa, o rimasto dopo la chiusura del browser) non conta.

use std::collections::HashMap;

/// Estensioni dei file ancora in arrivo: Chrome, Edge, Brave e Vivaldi; Firefox;
/// Opera; il vecchio Edge; qBittorrent, µTorrent e BitTorrent.
const PARTIAL: [&str; 7] = [
    ".crdownload",
    ".part",
    ".opdownload",
    ".partial",
    ".!qb",
    ".!ut",
    ".!bt",
];

/// Un file che non cresce da così tanto non è più un download in corso.
pub const STALL_MS: u64 = 8_000;
/// Con soli file fermi si ricontrolla ogni tanto, nel caso il download riparta.
const STALE_CHECK_MS: u64 = 4_000;

/// Il nome è quello di un file ancora in download.
pub fn is_partial(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    PARTIAL
        .iter()
        .any(|ext| lower.len() > ext.len() && lower.ends_with(ext))
}

/// Impronta di un file provvisorio: cambia a ogni scrittura.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    /// Ultima modifica, in un'unità qualsiasi: conta solo che cambi. Serve coi
    /// programmi che riservano subito tutto lo spazio del file.
    pub modified: u64,
}

#[derive(Debug, Default)]
pub struct DownloadWatch {
    files: HashMap<String, Tracked>,
    scanned: bool,
}

#[derive(Debug)]
struct Tracked {
    stamp: Stamp,
    grew_at: Option<u64>,
}

impl DownloadWatch {
    /// Aggiorna con i file provvisori presenti adesso e dice se c'è un download
    /// in corso. Un file comparso dopo il primo controllo conta come appena
    /// partito, anche se è ancora vuoto; quelli già presenti al primo controllo
    /// contano solo quando cambiano.
    pub fn update(&mut self, now_ms: u64, seen: impl IntoIterator<Item = (String, Stamp)>) -> bool {
        let mut files = HashMap::new();
        for (name, stamp) in seen {
            let grew_at = match self.files.remove(&name) {
                Some(old) if old.stamp == stamp => old.grew_at,
                Some(_) => Some(now_ms),
                None if self.scanned => Some(now_ms),
                None => None,
            };
            files.insert(name, Tracked { stamp, grew_at });
        }
        self.files = files;
        self.scanned = true;
        self.active(now_ms)
    }

    pub fn active(&self, now_ms: u64) -> bool {
        self.files.values().any(|file| {
            file.grew_at
                .is_some_and(|at| now_ms.saturating_sub(at) < STALL_MS)
        })
    }

    /// Fra quanto ricontrollare anche se la cartella non cambia: subito durante
    /// un download (per accorgersi se si ferma), ogni tanto con file fermi,
    /// mai senza file provvisori (allora bastano gli eventi della cartella).
    pub fn next_check_ms(&self, now_ms: u64) -> Option<u64> {
        if self.active(now_ms) {
            Some(0)
        } else if self.files.is_empty() {
            None
        } else {
            Some(STALE_CHECK_MS)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, u64)]) -> Vec<(String, Stamp)> {
        list.iter()
            .map(|&(name, size)| (name.to_string(), Stamp { size, modified: 0 }))
            .collect()
    }

    #[test]
    fn recognises_browser_and_torrent_partials() {
        for name in [
            "setup.exe.crdownload",
            "Unconfirmed 812345.crdownload",
            "video.mp4.part",
            "foto.zip.opdownload",
            "file.iso.!qB",
            "FILM.MKV.!UT",
        ] {
            assert!(is_partial(name), "{name}");
        }
        for name in [
            "setup.exe",
            "part",
            ".part",
            "report.partners.pdf",
            "crdownload",
        ] {
            assert!(!is_partial(name), "{name}");
        }
    }

    #[test]
    fn a_growing_file_is_a_download_until_it_stalls() {
        let mut watch = DownloadWatch::default();
        assert!(!watch.update(0, files(&[("a.zip.crdownload", 100)])));
        assert!(watch.update(1_000, files(&[("a.zip.crdownload", 900)])));
        // In pausa: resta "in corso" solo per qualche secondo.
        assert!(watch.update(5_000, files(&[("a.zip.crdownload", 900)])));
        assert!(!watch.update(1_000 + STALL_MS, files(&[("a.zip.crdownload", 900)])));
        // Riprende.
        assert!(watch.update(20_000, files(&[("a.zip.crdownload", 1_500)])));
        // Finito: rinominato col nome vero.
        assert!(!watch.update(21_000, files(&[])));
    }

    #[test]
    fn writing_without_growing_counts_too() {
        let stamp = |modified| {
            vec![(
                "film.mkv.part".to_string(),
                Stamp {
                    size: 4_000_000,
                    modified,
                },
            )]
        };
        let mut watch = DownloadWatch::default();
        assert!(!watch.update(0, stamp(1)));
        assert!(watch.update(1_000, stamp(2)));
        assert!(!watch.update(1_000 + STALL_MS, stamp(2)));
    }

    #[test]
    fn leftovers_from_before_do_not_count() {
        let mut watch = DownloadWatch::default();
        assert!(!watch.update(0, files(&[("vecchio.part", 50)])));
        assert!(!watch.update(1_000, files(&[("vecchio.part", 50)])));
        assert_eq!(watch.next_check_ms(1_000), Some(STALE_CHECK_MS));
    }

    #[test]
    fn a_new_file_counts_at_once_even_if_empty() {
        let mut watch = DownloadWatch::default();
        assert!(!watch.update(0, files(&[])));
        assert_eq!(watch.next_check_ms(0), None);
        assert!(watch.update(500, files(&[("Unconfirmed 1.crdownload", 0)])));
        assert_eq!(watch.next_check_ms(500), Some(0));
        // Chrome lo rinomina col nome del file appena lo conosce: è lo stesso download.
        assert!(watch.update(1_500, files(&[("libro.pdf.crdownload", 0)])));
    }
}
