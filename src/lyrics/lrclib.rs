//! Parte "pura" del client LRCLIB (https://lrclib.net): costruzione degli URL,
//! pulizia dei titoli e scelta del risultato migliore. La rete è in `fetch.rs`.
//!
//! API (verificata sul sorgente del server):
//! - `GET /api/get`: `track_name` e `artist_name` obbligatori, `album_name` e
//!   `duration` (secondi) facoltativi. Risponde 404 se non trova il brano.
//! - `GET /api/search`: `track_name`, `artist_name` (o `q`); restituisce un array.

use std::sync::Arc;

use serde::Deserialize;

use super::{LyricsState, lrc::Lyrics};
use crate::util::percent_encode;

pub const BASE_URL: &str = "https://lrclib.net/api";

/// Tolleranza sulla durata per accettare un risultato della ricerca.
const DURATION_TOLERANCE_S: f64 = 3.0;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub instrumental: bool,
    #[serde(default)]
    pub plain_lyrics: Option<String>,
    #[serde(default)]
    pub synced_lyrics: Option<String>,
}

/// Il brano come lo descrive il player.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_s: Option<f64>,
}

impl Query {
    /// Ricerca "esatta" con i dati grezzi del player (funziona bene con Spotify & co.).
    pub fn get_url(&self) -> Option<String> {
        let (title, artist) = (self.title.trim(), self.artist.trim());
        if title.is_empty() || artist.is_empty() {
            return None;
        }
        let mut url = format!(
            "{BASE_URL}/get?track_name={}&artist_name={}",
            percent_encode(title),
            percent_encode(artist)
        );
        if !self.album.trim().is_empty() {
            url += &format!("&album_name={}", percent_encode(self.album.trim()));
        }
        if let Some(d) = self.duration_s.filter(|d| (1.0..=3600.0).contains(d)) {
            url += &format!("&duration={}", d.round() as u64);
        }
        Some(url)
    }

    /// Ricerche più tolleranti, con titoli ripuliti (utile per YouTube nel browser).
    pub fn search_urls(&self) -> Vec<String> {
        candidates(&self.title, &self.artist)
            .into_iter()
            .map(|(t, a)| {
                format!(
                    "{BASE_URL}/search?track_name={}&artist_name={}",
                    percent_encode(&t),
                    percent_encode(&a)
                )
            })
            .collect()
    }
}

/// Coppie (titolo, artista) da provare, in ordine di priorità.
pub fn candidates(title: &str, artist: &str) -> Vec<(String, String)> {
    let title = strip_noise(title);
    let artist = clean_artist(artist);
    let mut out = Vec::new();
    if !title.is_empty() && !artist.is_empty() {
        out.push((title.clone(), artist));
    }
    // Su YouTube il titolo è spesso "Artista - Brano" e l'artista è il canale.
    if let Some((left, right)) = title.split_once(" - ") {
        let (left, right) = (left.trim(), strip_noise(right));
        if !left.is_empty() && !right.is_empty() {
            let pair = (right, left.to_string());
            if !out.contains(&pair) {
                out.push(pair);
            }
        }
    }
    out
}

/// Toglie le parentesi tipo "(Official Video)", "[Lyrics]", "(Remastered 2011)".
fn strip_noise(title: &str) -> String {
    const NOISE: &[&str] = &[
        "official",
        "video",
        "audio",
        "lyric",
        "lyrics",
        "visualizer",
        "visualiser",
        "mv",
        "hd",
        "4k",
        "remaster",
        "remastered",
        "explicit",
        "testo",
    ];
    let mut out = String::with_capacity(title.len());
    let mut rest = title;
    while let Some(open) = rest.find(['(', '[']) {
        let close_ch = if rest.as_bytes()[open] == b'(' {
            ')'
        } else {
            ']'
        };
        let Some(len) = rest[open..].find(close_ch) else {
            break;
        };
        let inner = rest[open + 1..open + len].to_lowercase();
        let noisy = inner.contains("m/v")
            || inner
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| NOISE.contains(&word));
        out.push_str(&rest[..open]);
        if !noisy {
            out.push_str(&rest[open..=open + len]);
        }
        rest = &rest[open + len + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_artist(artist: &str) -> String {
    let a = artist.trim();
    a.strip_suffix(" - Topic").unwrap_or(a).trim().to_string()
}

pub fn parse_get(body: &str) -> Option<Track> {
    serde_json::from_str(body).ok()
}

pub fn parse_search(body: &str) -> Vec<Track> {
    serde_json::from_str(body).unwrap_or_default()
}

/// Cosa si può mostrare per questa traccia; `None` se non contiene nulla di utile.
pub fn track_state(t: &Track) -> Option<LyricsState> {
    if t.instrumental {
        return Some(LyricsState::Instrumental);
    }
    if let Some(synced) = t.synced_lyrics.as_deref().filter(|s| !s.trim().is_empty()) {
        let lyrics = Lyrics::parse(synced);
        if !lyrics.is_empty() {
            return Some(LyricsState::Synced(Arc::new(lyrics)));
        }
    }
    t.plain_lyrics
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|_| LyricsState::Unsynced)
}

/// Sceglie tra i risultati della ricerca: prima i testi sincronizzati con la
/// durata più vicina, poi gli strumentali, poi quelli solo testuali.
pub fn pick_best(tracks: &[Track], duration_s: Option<f64>) -> Option<LyricsState> {
    let distance = |t: &Track| match (duration_s, t.duration) {
        (Some(want), Some(have)) => (want - have).abs(),
        _ => 0.0,
    };
    let mut candidates: Vec<&Track> = tracks
        .iter()
        .filter(|t| distance(t) <= DURATION_TOLERANCE_S)
        .collect();
    candidates.sort_by(|a, b| distance(a).total_cmp(&distance(b)));

    let states: Vec<LyricsState> = candidates.iter().filter_map(|t| track_state(t)).collect();
    let rank = |s: &LyricsState| match s {
        LyricsState::Synced(_) => 0,
        LyricsState::Instrumental => 1,
        _ => 2,
    };
    states.into_iter().min_by_key(rank)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(
        duration: f64,
        synced: Option<&str>,
        plain: Option<&str>,
        instrumental: bool,
    ) -> Track {
        Track {
            duration: Some(duration),
            instrumental,
            plain_lyrics: plain.map(Into::into),
            synced_lyrics: synced.map(Into::into),
        }
    }

    #[test]
    fn builds_get_url_with_optional_fields() {
        let q = Query {
            title: "Paper Sky".into(),
            artist: "Lumen Coast".into(),
            album: "".into(),
            duration_s: Some(193.4),
        };
        assert_eq!(
            q.get_url().unwrap(),
            "https://lrclib.net/api/get?track_name=Paper%20Sky&artist_name=Lumen%20Coast&duration=193"
        );
        let q = Query {
            album: "Summer Flows".into(),
            duration_s: Some(0.0),
            ..q
        };
        assert_eq!(
            q.get_url().unwrap(),
            "https://lrclib.net/api/get?track_name=Paper%20Sky&artist_name=Lumen%20Coast&album_name=Summer%20Flows"
        );
        let q = Query {
            artist: " ".into(),
            ..q
        };
        assert_eq!(q.get_url(), None);
    }

    #[test]
    fn cleans_youtube_style_titles() {
        assert_eq!(strip_noise("Paper Sky (Official Video)"), "Paper Sky");
        assert_eq!(
            strip_noise("Paper Sky [Official Music Video] (Lyrics)"),
            "Paper Sky"
        );
        assert_eq!(strip_noise("Paper Sky (Remastered 2011)"), "Paper Sky");
        assert_eq!(
            strip_noise("Paper Sky (feat. Dina)"),
            "Paper Sky (feat. Dina)"
        );
        assert_eq!(strip_noise("Paper Sky (M/V)"), "Paper Sky");
        assert_eq!(strip_noise("Paper (Sky"), "Paper (Sky");
    }

    #[test]
    fn generates_candidates_from_artist_dash_title() {
        let c = candidates(
            "Lumen Coast - Paper Sky (Official Video)",
            "Lumen Coast VEVO",
        );
        assert_eq!(
            c,
            vec![
                (
                    "Lumen Coast - Paper Sky".to_string(),
                    "Lumen Coast VEVO".to_string()
                ),
                ("Paper Sky".to_string(), "Lumen Coast".to_string()),
            ]
        );
        let c = candidates("Paper Sky", "Lumen Coast - Topic");
        assert_eq!(
            c,
            vec![("Paper Sky".to_string(), "Lumen Coast".to_string())]
        );
        assert!(candidates("", "").is_empty());
    }

    #[test]
    fn parses_api_responses() {
        let body = r#"{"id":1,"name":"Paper Sky","trackName":"Paper Sky","artistName":"Lumen Coast",
            "albumName":"Summer Flows","duration":193.0,"instrumental":false,"hasWordSync":false,
            "plainLyrics":"we fold the summer light","syncedLyrics":"[00:12.00] we fold the summer light",
            "lyricsfile":null}"#;
        let t = parse_get(body).unwrap();
        assert!(
            matches!(track_state(&t), Some(LyricsState::Synced(l)) if l.lines[0].time_ms == 12_000)
        );

        let list = parse_search(&format!("[{body}]"));
        assert_eq!(list.len(), 1);
        assert!(parse_search("not json").is_empty());
        assert!(
            parse_get(r#"{"message":"Failed to find specified track","name":"TrackNotFound"}"#)
                .and_then(|t| track_state(&t))
                .is_none()
        );
    }

    #[test]
    fn picks_synced_lyrics_with_the_closest_duration() {
        let results = [
            track(200.0, Some("[00:01.00]troppo lunga"), None, false),
            track(194.0, None, Some("solo testo"), false),
            track(193.0, Some("[00:02.00]giusta"), None, false),
        ];
        match pick_best(&results, Some(193.2)) {
            Some(LyricsState::Synced(l)) => assert_eq!(l.lines[0].text, "giusta"),
            other => panic!("risultato inatteso: {other:?}"),
        }
        assert!(matches!(
            pick_best(&results[1..2], Some(193.0)),
            Some(LyricsState::Unsynced)
        ));
        assert!(pick_best(&results[..1], Some(193.0)).is_none());
        assert!(matches!(
            pick_best(&results[..1], None),
            Some(LyricsState::Synced(_))
        ));
    }

    #[test]
    fn instrumental_beats_plain_text() {
        let results = [
            track(100.0, None, Some("x"), false),
            track(100.0, None, None, true),
        ];
        assert!(matches!(
            pick_best(&results, Some(100.0)),
            Some(LyricsState::Instrumental)
        ));
    }
}
