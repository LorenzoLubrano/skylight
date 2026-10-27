//! Scarica i testi da LRCLIB su un thread dedicato, con l'HttpClient di Windows
//! (TLS del sistema, nessuna dipendenza in più nell'eseguibile).

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use windows::Foundation::Uri;
use windows::Web::Http::{HttpClient, HttpStatusCode};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::core::{HSTRING, Result};

use super::LyricsState;
use super::lrclib::{self, Query};
use crate::util::Inbox;
use crate::win::Notifier;

/// LRCLIB chiede di identificare l'app nello User-Agent.
const USER_AGENT: &str = concat!("Skylight/", env!("CARGO_PKG_VERSION"));

/// Risposte per l'interfaccia: `None` significa "richiesta saltata perché
/// superata da una più recente", così l'interfaccia può riprovare più tardi.
pub type Replies = Inbox<(u64, Option<LyricsState>)>;

pub fn spawn(inbox: Arc<Replies>, notifier: Notifier) -> Sender<(u64, Query)> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("lyrics".into())
        .spawn(move || run(rx, &inbox, notifier))
        .expect("impossibile avviare il thread dei testi");
    tx
}

fn run(rx: Receiver<(u64, Query)>, inbox: &Replies, notifier: Notifier) {
    let client = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.and_then(|()| make_client());
    while let Ok(mut request) = rx.recv() {
        // Se l'utente ha già cambiato brano, serve solo l'ultima richiesta.
        while let Ok(newer) = rx.try_recv() {
            inbox.push((request.0, None));
            request = newer;
        }
        let (key, query) = request;
        let state = match &client {
            Ok(client) => fetch(client, &query).unwrap_or(LyricsState::Failed),
            Err(_) => LyricsState::Failed,
        };
        inbox.push((key, Some(state)));
        notifier.notify();
    }
}

fn make_client() -> Result<HttpClient> {
    let client = HttpClient::new()?;
    client
        .DefaultRequestHeaders()?
        .UserAgent()?
        .TryParseAdd(&HSTRING::from(USER_AGENT))?;
    Ok(client)
}

fn fetch(client: &HttpClient, query: &Query) -> Result<LyricsState> {
    let mut fallback = None;

    if let Some(url) = query.get_url()
        && let Some(track) = get(client, &url)?.as_deref().and_then(lrclib::parse_get)
    {
        match lrclib::track_state(&track) {
            Some(found @ (LyricsState::Synced(_) | LyricsState::Instrumental)) => return Ok(found),
            other => fallback = other,
        }
    }

    for url in query.search_urls() {
        if let Some(body) = get(client, &url)? {
            match lrclib::pick_best(&lrclib::parse_search(&body), query.duration_s) {
                Some(found @ (LyricsState::Synced(_) | LyricsState::Instrumental)) => {
                    return Ok(found);
                }
                Some(other) => {
                    fallback.get_or_insert(other);
                }
                None => {}
            }
        }
    }
    Ok(fallback.unwrap_or(LyricsState::Missing))
}

/// GET che restituisce `None` per un 404 (brano non presente su LRCLIB).
fn get(client: &HttpClient, url: &str) -> Result<Option<String>> {
    let uri = Uri::CreateUri(&HSTRING::from(url))?;
    let response = client.GetAsync(&uri)?.join()?;
    if response.StatusCode()? == HttpStatusCode::NotFound {
        return Ok(None);
    }
    response.EnsureSuccessStatusCode()?;
    Ok(Some(
        response.Content()?.ReadAsStringAsync()?.join()?.to_string(),
    ))
}
