//! Download in corso nella cartella Download. Senza file provvisori il thread
//! dorme sugli eventi della cartella (niente polling); finché ce n'è uno ne
//! controlla la dimensione circa una volta al secondo.
//!
//! La dimensione si legge aprendo il file: quella nell'elenco della cartella
//! resta indietro finché il browser lo tiene aperto.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::Storage::FileSystem::{
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
    FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};
use windows::Win32::UI::Shell::{FOLDERID_Downloads, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::core::{Error, HSTRING, Result};

use super::{Events, SystemEvent, emit};
use crate::features::downloads::{DownloadWatch, Stamp, is_partial};
use crate::win::Notifier;

pub fn run(inbox: Arc<Events>, notifier: Notifier) -> Result<()> {
    let dir = downloads_dir()?;
    let filter =
        FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_SIZE | FILE_NOTIFY_CHANGE_LAST_WRITE;
    let change =
        unsafe { FindFirstChangeNotificationW(&HSTRING::from(dir.as_path()), false, filter)? };
    let start = Instant::now();
    let now = || start.elapsed().as_millis() as u64;
    let mut watch = DownloadWatch::default();
    let mut active = false;
    let result = loop {
        if watch.update(now(), partial_files(&dir)) != active {
            active = !active;
            emit(&inbox, &notifier, SystemEvent::Downloading(active));
        }
        // Al massimo un controllo al secondo, anche se la cartella cambia di continuo.
        std::thread::sleep(Duration::from_secs(1));
        let timeout = watch.next_check_ms(now()).map_or(INFINITE, |ms| ms as u32);
        if unsafe { WaitForSingleObject(change, timeout) } == WAIT_OBJECT_0
            && let Err(e) = unsafe { FindNextChangeNotification(change) }
        {
            break Err(e);
        }
    };
    let _ = unsafe { FindCloseChangeNotification(change) };
    result
}

/// La cartella Download dell'utente, anche se spostata su un altro disco.
fn downloads_dir() -> Result<PathBuf> {
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None)?;
        let dir = path.to_string().map_err(|_| Error::empty());
        CoTaskMemFree(Some(path.0 as *const _));
        Ok(PathBuf::from(dir?))
    }
}

/// File provvisori dei download, con dimensione e ultima modifica attuali.
fn partial_files(dir: &Path) -> Vec<(String, Stamp)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_partial(&name) {
                return None;
            }
            let meta = std::fs::metadata(entry.path()).ok()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_micros() as u64);
            let stamp = Stamp {
                size: meta.len(),
                modified,
            };
            Some((name, stamp))
        })
        .collect()
}
