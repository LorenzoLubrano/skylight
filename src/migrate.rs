//! Cambio di nome da Cresta a Skylight (0.4). Al primo avvio col nome nuovo si
//! portano con sé dati, impostazioni e avvio con Windows del nome vecchio.
//! I dati e le impostazioni vecchi restano dove sono; se una copia fallisce si
//! parte da zero e al prossimo avvio si riprova.
//!
//! Qui c'è solo la logica, con file e registro simulabili nei test; il registro
//! vero sta in `win::migration`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "Skylight";
pub const OLD_NAME: &str = "Cresta";

/// Esito di una parte della migrazione.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Non c'era niente da portare, o era già stato fatto.
    Nothing,
    Done,
    /// Non riuscita: si parte da zero, i vecchi restano intatti.
    Failed,
}

// ---- Cartella dei dati --------------------------------------------------------------------

/// Copia i file della cartella dati vecchia (`todo.json`, `water.json`,
/// `shelf.json`, eventuali `.bad`) in quella nuova, se questa manca o è vuota.
pub fn copy_data(old: &Path, new: &Path) -> Outcome {
    copy_data_with(old, new, |from, to| fs::copy(from, to).map(drop))
}

fn copy_data_with(
    old: &Path,
    new: &Path,
    mut copy: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> Outcome {
    if !old.is_dir() || has_entries(new) {
        return Outcome::Nothing;
    }
    let mut copied: Vec<PathBuf> = Vec::new();
    let result = (|| {
        fs::create_dir_all(new)?;
        for entry in fs::read_dir(old)? {
            let entry = entry?;
            let name = entry.file_name();
            // I `.tmp` sono salvataggi interrotti a metà: non servono.
            let temporary = Path::new(&name).extension().is_some_and(|e| e == "tmp");
            if !entry.file_type()?.is_file() || temporary {
                continue;
            }
            let target = new.join(&name);
            copy(&entry.path(), &target)?;
            copied.push(target);
        }
        io::Result::Ok(())
    })();
    match result {
        Ok(()) if copied.is_empty() => Outcome::Nothing,
        Ok(()) => Outcome::Done,
        Err(_) => {
            // Niente copie a metà: la cartella nuova resta vuota e la prossima volta si riprova.
            for path in copied {
                let _ = fs::remove_file(path);
            }
            Outcome::Failed
        }
    }
}

fn has_entries(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

// ---- Registro -----------------------------------------------------------------------------

/// Quanto serve del registro dell'utente. `app` è il nome dell'app: le
/// impostazioni stanno in `HKCU\Software\<app>`, l'avvio con Windows nel valore
/// `<app>` di `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
pub trait Registry {
    fn has_settings(&self, app: &str) -> bool;
    /// Copia tutta la chiave delle impostazioni.
    fn copy_settings(&mut self, from: &str, to: &str) -> bool;
    fn delete_settings(&mut self, app: &str);
    fn has_autostart(&self, app: &str) -> bool;
    /// La voce di avvio è stata disattivata da Gestione attività.
    fn autostart_disabled(&self, app: &str) -> bool;
    fn set_autostart(&mut self, app: &str, command: &str) -> bool;
    /// Toglie la voce di avvio (e il suo stato in Gestione attività).
    fn delete_autostart(&mut self, app: &str);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryOutcome {
    pub settings: Outcome,
    pub autostart: Outcome,
}

/// Impostazioni e avvio con Windows. `command` è la riga di comando del nuovo
/// eseguibile, come va scritta nella voce di avvio.
pub fn migrate_registry(reg: &mut impl Registry, command: &str) -> RegistryOutcome {
    let settings = if reg.has_settings(APP_NAME) || !reg.has_settings(OLD_NAME) {
        Outcome::Nothing
    } else if reg.copy_settings(OLD_NAME, APP_NAME) {
        Outcome::Done
    } else {
        // Una copia a metà non deve sembrare fatta: si riprova al prossimo avvio.
        reg.delete_settings(APP_NAME);
        Outcome::Failed
    };

    // La voce vecchia farebbe partire il vecchio exe (o niente, se è stato cancellato).
    let autostart = if !reg.has_autostart(OLD_NAME) {
        Outcome::Nothing
    } else {
        let enabled = !reg.autostart_disabled(OLD_NAME);
        let written =
            !enabled || reg.has_autostart(APP_NAME) || reg.set_autostart(APP_NAME, command);
        if written {
            // Si toglie solo quando la nuova c'è: altrimenti si riprova al prossimo avvio.
            reg.delete_autostart(OLD_NAME);
            Outcome::Done
        } else {
            Outcome::Failed
        }
    };
    RegistryOutcome {
        settings,
        autostart,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;

    // ---- Cartella simulata in una cartella temporanea ----

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("skylight-migrate-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn old_data(base: &Path) -> PathBuf {
        let old = base.join(OLD_NAME);
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("todo.json"), "[todo]").unwrap();
        fs::write(old.join("water.json"), "[acqua]").unwrap();
        fs::write(old.join("shelf.json.bad"), "{ rovinato").unwrap();
        fs::write(old.join("shelf.json.tmp"), "{ a metà").unwrap();
        old
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn data_is_copied_into_a_missing_folder_and_the_old_one_stays() {
        let base = TempDir::new("copy");
        let old = old_data(&base.0);
        let new = base.0.join(APP_NAME);
        assert_eq!(copy_data(&old, &new), Outcome::Done);
        assert_eq!(names(&new), ["shelf.json.bad", "todo.json", "water.json"]);
        assert_eq!(fs::read_to_string(new.join("todo.json")).unwrap(), "[todo]");
        assert_eq!(names(&old).len(), 4, "la cartella vecchia non si tocca");
        // Già fatto: la seconda volta non copia di nuovo.
        fs::write(new.join("todo.json"), "[nuovo]").unwrap();
        assert_eq!(copy_data(&old, &new), Outcome::Nothing);
        assert_eq!(
            fs::read_to_string(new.join("todo.json")).unwrap(),
            "[nuovo]"
        );
    }

    #[test]
    fn an_empty_new_folder_still_gets_the_data() {
        let base = TempDir::new("empty");
        let old = old_data(&base.0);
        let new = base.0.join(APP_NAME);
        fs::create_dir_all(&new).unwrap();
        assert_eq!(copy_data(&old, &new), Outcome::Done);
        assert_eq!(names(&new).len(), 3);
    }

    #[test]
    fn nothing_to_do_without_the_old_folder() {
        let base = TempDir::new("none");
        let new = base.0.join(APP_NAME);
        assert_eq!(copy_data(&base.0.join(OLD_NAME), &new), Outcome::Nothing);
        assert!(!new.exists());
    }

    #[test]
    fn a_failed_copy_leaves_the_new_folder_empty_and_the_old_intact() {
        let base = TempDir::new("fail");
        let old = old_data(&base.0);
        let new = base.0.join(APP_NAME);
        let mut calls = 0;
        let outcome = copy_data_with(&old, &new, |from, to| {
            calls += 1;
            if calls == 2 {
                return Err(io::Error::other("disco pieno"));
            }
            fs::copy(from, to).map(drop)
        });
        assert_eq!(outcome, Outcome::Failed);
        assert!(names(&new).is_empty(), "nessun file a metà");
        assert_eq!(names(&old).len(), 4);
        // Al prossimo avvio si riprova.
        assert_eq!(copy_data(&old, &new), Outcome::Done);
    }

    // ---- Registro simulato ----

    #[derive(Default)]
    struct FakeRegistry {
        settings: HashMap<String, HashMap<String, u32>>,
        run: HashMap<String, String>,
        disabled: HashSet<String>,
        fail_copy: bool,
        fail_run: bool,
    }

    impl Registry for FakeRegistry {
        fn has_settings(&self, app: &str) -> bool {
            self.settings.contains_key(app)
        }
        fn copy_settings(&mut self, from: &str, to: &str) -> bool {
            let mut values = self.settings[from].clone();
            if self.fail_copy {
                // A metà strada.
                values.retain(|name, _| name.starts_with('H'));
            }
            self.settings.insert(to.into(), values);
            !self.fail_copy
        }
        fn delete_settings(&mut self, app: &str) {
            self.settings.remove(app);
        }
        fn has_autostart(&self, app: &str) -> bool {
            self.run.contains_key(app)
        }
        fn autostart_disabled(&self, app: &str) -> bool {
            self.disabled.contains(app)
        }
        fn set_autostart(&mut self, app: &str, command: &str) -> bool {
            if !self.fail_run {
                self.run.insert(app.into(), command.into());
            }
            !self.fail_run
        }
        fn delete_autostart(&mut self, app: &str) {
            self.run.remove(app);
            self.disabled.remove(app);
        }
    }

    const EXE: &str = r#""C:\Programmi\Skylight\skylight.exe""#;

    fn with_old_settings() -> FakeRegistry {
        let mut reg = FakeRegistry::default();
        let values = HashMap::from([("HudVolume".into(), 0), ("NotchScale".into(), 120)]);
        reg.settings.insert(OLD_NAME.into(), values);
        reg
    }

    #[test]
    fn settings_are_copied_once_and_the_old_key_stays() {
        let mut reg = with_old_settings();
        let outcome = migrate_registry(&mut reg, EXE);
        assert_eq!(outcome.settings, Outcome::Done);
        assert_eq!(reg.settings[APP_NAME]["NotchScale"], 120);
        assert!(reg.has_settings(OLD_NAME));
        // Le impostazioni nuove non si sovrascrivono più.
        reg.settings
            .get_mut(APP_NAME)
            .unwrap()
            .insert("NotchScale".into(), 90);
        assert_eq!(migrate_registry(&mut reg, EXE).settings, Outcome::Nothing);
        assert_eq!(reg.settings[APP_NAME]["NotchScale"], 90);
    }

    #[test]
    fn a_half_copied_key_is_removed_so_it_is_tried_again() {
        let mut reg = with_old_settings();
        reg.fail_copy = true;
        assert_eq!(migrate_registry(&mut reg, EXE).settings, Outcome::Failed);
        assert!(
            !reg.has_settings(APP_NAME),
            "si parte dalle impostazioni predefinite"
        );
        assert_eq!(reg.settings[OLD_NAME].len(), 2);
        reg.fail_copy = false;
        assert_eq!(migrate_registry(&mut reg, EXE).settings, Outcome::Done);
    }

    #[test]
    fn autostart_moves_to_the_new_exe() {
        let mut reg = FakeRegistry::default();
        reg.run
            .insert(OLD_NAME.into(), r#""C:\Vecchio\cresta.exe""#.into());
        let outcome = migrate_registry(&mut reg, EXE);
        assert_eq!(outcome.autostart, Outcome::Done);
        assert_eq!(reg.run.get(APP_NAME).map(String::as_str), Some(EXE));
        assert!(!reg.has_autostart(OLD_NAME));
        assert_eq!(outcome.settings, Outcome::Nothing);
    }

    #[test]
    fn autostart_disabled_in_task_manager_stays_off() {
        let mut reg = FakeRegistry::default();
        reg.run.insert(OLD_NAME.into(), "vecchio".into());
        reg.disabled.insert(OLD_NAME.into());
        assert_eq!(migrate_registry(&mut reg, EXE).autostart, Outcome::Done);
        assert!(reg.run.is_empty());
    }

    #[test]
    fn the_old_autostart_stays_until_the_new_one_is_written() {
        let mut reg = FakeRegistry::default();
        reg.run.insert(OLD_NAME.into(), "vecchio".into());
        reg.fail_run = true;
        assert_eq!(migrate_registry(&mut reg, EXE).autostart, Outcome::Failed);
        assert!(reg.has_autostart(OLD_NAME));
        reg.fail_run = false;
        assert_eq!(migrate_registry(&mut reg, EXE).autostart, Outcome::Done);
        assert!(reg.has_autostart(APP_NAME) && !reg.has_autostart(OLD_NAME));
    }

    #[test]
    fn nothing_happens_without_the_old_name() {
        let mut reg = FakeRegistry::default();
        let outcome = migrate_registry(&mut reg, EXE);
        assert_eq!(outcome.settings, Outcome::Nothing);
        assert_eq!(outcome.autostart, Outcome::Nothing);
        assert!(reg.settings.is_empty() && reg.run.is_empty());
    }
}
