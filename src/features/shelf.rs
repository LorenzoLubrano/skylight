//! Shelf: un ripiano per file e frammenti di testo. Dei file si ricorda solo il
//! percorso (niente copie): resta tutto dov'è.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ShelfItem {
    File { path: String },
    Text { text: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    #[serde(flatten)]
    pub item: ShelfItem,
    pub added: i64,
}

impl Entry {
    /// Nome da mostrare sotto l'anteprima.
    pub fn name(&self) -> String {
        match &self.item {
            ShelfItem::File { path } => {
                let trimmed = path.trim_end_matches(['\\', '/']);
                trimmed
                    .rsplit(['\\', '/'])
                    .next()
                    .unwrap_or(trimmed)
                    .to_string()
            }
            ShelfItem::Text { text } => {
                let line = text
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .unwrap_or("");
                let short: String = line.chars().take(48).collect();
                if short.len() < line.len() {
                    format!("{short}…")
                } else {
                    short
                }
            }
        }
    }

    pub fn path(&self) -> Option<&str> {
        match &self.item {
            ShelfItem::File { path } => Some(path),
            ShelfItem::Text { .. } => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match &self.item {
            ShelfItem::Text { text } => Some(text),
            ShelfItem::File { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Shelf {
    #[serde(default)]
    pub entries: Vec<Entry>,
    #[serde(default)]
    next_id: u64,
}

impl Shelf {
    fn new_id(&mut self) -> u64 {
        self.next_id = self
            .next_id
            .max(self.entries.iter().map(|e| e.id).max().unwrap_or(0))
            + 1;
        self.next_id
    }

    /// Aggiunge in cima; un file già presente torna in cima invece di duplicarsi.
    pub fn add_files(&mut self, paths: Vec<String>, now: i64) -> usize {
        let mut added = 0;
        for path in paths.into_iter().rev() {
            if path.trim().is_empty() {
                continue;
            }
            self.entries
                .retain(|e| !e.path().is_some_and(|p| p.eq_ignore_ascii_case(&path)));
            let id = self.new_id();
            self.entries.insert(
                0,
                Entry {
                    id,
                    item: ShelfItem::File { path },
                    added: now,
                },
            );
            added += 1;
        }
        added
    }

    pub fn add_text(&mut self, text: String, now: i64) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        self.entries.retain(|e| e.text() != Some(text.as_str()));
        let id = self.new_id();
        self.entries.insert(
            0,
            Entry {
                id,
                item: ShelfItem::Text { text },
                added: now,
            },
        );
        true
    }

    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn remove(&mut self, ids: &[u64]) {
        self.entries.retain(|e| !ids.contains(&e.id));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_without_duplicates() {
        let mut s = Shelf::default();
        assert_eq!(
            s.add_files(vec![r"C:\a\uno.pdf".into(), r"C:\a\due.png".into()], 1),
            2
        );
        let names: Vec<String> = s.entries.iter().map(Entry::name).collect();
        assert_eq!(names, ["uno.pdf", "due.png"]);

        s.add_files(vec![r"c:\A\DUE.png".into()], 2);
        assert_eq!(s.len(), 2, "lo stesso file non si duplica");
        assert_eq!(s.entries[0].name(), "DUE.png");
    }

    #[test]
    fn names_for_folders_and_text() {
        let mut s = Shelf::default();
        s.add_files(vec![r"C:\Progetti\Skylight\".into()], 0);
        assert_eq!(s.entries[0].name(), "Skylight");
        assert!(s.add_text("\n  prima riga\nseconda".into(), 0));
        assert_eq!(s.entries[0].name(), "prima riga");
        assert!(!s.add_text("   ".into(), 0));
        let long = "x".repeat(60);
        s.add_text(long, 0);
        assert_eq!(s.entries[0].name().chars().count(), 49);
    }

    #[test]
    fn remove_and_clear() {
        let mut s = Shelf::default();
        s.add_files(vec!["a".into(), "b".into(), "c".into()], 0);
        let ids: Vec<u64> = s.entries.iter().map(|e| e.id).take(2).collect();
        s.remove(&ids);
        assert_eq!(s.len(), 1);
        s.clear();
        assert!(s.is_empty());
    }

    #[test]
    fn serializes_with_a_type_tag() {
        let mut s = Shelf::default();
        s.add_text("ciao".into(), 5);
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""type":"text""#), "{json}");
        let back: Shelf = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entries, s.entries);
    }
}
