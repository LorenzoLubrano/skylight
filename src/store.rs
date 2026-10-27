//! Salvataggio dei dati (to-do, acqua, shelf) in file JSON nella cartella
//! dell'utente: `%APPDATA%\Skylight` su Windows.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::migrate::APP_NAME;

/// Cartella che contiene quelle dei dati delle app (`%APPDATA%` su Windows).
pub fn base_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

/// Cartella dei dati, creata se manca.
pub fn data_dir() -> Option<PathBuf> {
    let dir = base_dir()?.join(APP_NAME);
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Legge un file JSON; se manca si parte dal valore predefinito. Se c'è ma non
/// si riesce a leggere, lo si mette da parte come `<nome>.bad` invece di
/// lasciarlo sovrascrivere al primo salvataggio.
pub fn load<T: DeserializeOwned + Default>(dir: &Path, name: &str) -> T {
    let path = dir.join(name);
    let Ok(text) = fs::read_to_string(&path) else {
        return T::default();
    };
    match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => {
            let _ = fs::rename(&path, dir.join(format!("{name}.bad")));
            T::default()
        }
    }
}

/// Scrive prima un file temporaneo, lo forza su disco e poi lo rinomina:
/// un'interruzione a metà (anche un blackout) non lascia mai un file rovinato.
pub fn save<T: Serialize>(dir: &Path, name: &str, value: &T) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let tmp = dir.join(format!("{name}.tmp"));
    let mut file = fs::File::create(&tmp)?;
    file.write_all(&json)?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::todo::TodoList;

    #[test]
    fn round_trip_and_recovery() {
        let dir = std::env::temp_dir().join(format!("skylight-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let mut list = TodoList::default();
        list.add("prova".into(), Some(42), 1);
        save(&dir, "todo.json", &list).unwrap();
        let back: TodoList = load(&dir, "todo.json");
        assert_eq!(back.tasks, list.tasks);

        fs::write(dir.join("todo.json"), "{ rovinato").unwrap();
        let fresh: TodoList = load(&dir, "todo.json");
        assert!(fresh.tasks.is_empty());
        assert_eq!(
            fs::read_to_string(dir.join("todo.json.bad")).unwrap(),
            "{ rovinato",
            "il file illeggibile resta da parte"
        );
        let missing: TodoList = load(&dir, "non-esiste.json");
        assert!(missing.tasks.is_empty());

        fs::remove_dir_all(dir).unwrap();
    }
}
