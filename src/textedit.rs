//! Campo di testo su una riga: il minimo per scrivere un'attività
//! (inserimento, cancellazione, frecce, Ctrl per le parole).

#[derive(Clone, Debug, Default)]
pub struct TextEdit {
    text: String,
    /// Posizione del cursore in byte, sempre su un confine di carattere.
    caret: usize,
    max_chars: usize,
}

impl TextEdit {
    pub fn new(max_chars: usize) -> Self {
        Self {
            text: String::new(),
            caret: 0,
            max_chars,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// Posizione del cursore in unità UTF-16, come la vuole DirectWrite.
    pub fn caret_utf16(&self) -> u32 {
        self.text[..self.caret].encode_utf16().count() as u32
    }

    pub fn insert(&mut self, s: &str) {
        let room = self.max_chars.saturating_sub(self.text.chars().count());
        let clean: String = s
            .chars()
            .map(|c| {
                if c == '\n' || c == '\r' || c == '\t' {
                    ' '
                } else {
                    c
                }
            })
            .filter(|c| !c.is_control())
            .take(room)
            .collect();
        self.text.insert_str(self.caret, &clean);
        self.caret += clean.len();
    }

    pub fn backspace(&mut self, word: bool) {
        let from = if word {
            self.word_start()
        } else {
            self.prev_boundary()
        };
        self.text.replace_range(from..self.caret, "");
        self.caret = from;
    }

    pub fn delete(&mut self, word: bool) {
        let to = if word {
            self.word_end()
        } else {
            self.next_boundary()
        };
        self.text.replace_range(self.caret..to, "");
    }

    pub fn left(&mut self, word: bool) {
        self.caret = if word {
            self.word_start()
        } else {
            self.prev_boundary()
        };
    }

    pub fn right(&mut self, word: bool) {
        self.caret = if word {
            self.word_end()
        } else {
            self.next_boundary()
        };
    }

    pub fn home(&mut self) {
        self.caret = 0;
    }

    pub fn end(&mut self) {
        self.caret = self.text.len();
    }

    fn prev_boundary(&self) -> usize {
        self.text[..self.caret]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self) -> usize {
        self.text[self.caret..]
            .chars()
            .next()
            .map_or(self.caret, |c| self.caret + c.len_utf8())
    }

    /// Inizio della parola a sinistra (saltando prima gli spazi).
    fn word_start(&self) -> usize {
        let before = &self.text[..self.caret];
        let trimmed = before.trim_end();
        trimmed.rfind(char::is_whitespace).map_or(0, |i| {
            i + trimmed[i..].chars().next().map_or(1, char::len_utf8)
        })
    }

    /// Fine della parola a destra (saltando prima gli spazi).
    fn word_end(&self) -> usize {
        let after = &self.text[self.caret..];
        let skipped = after.len() - after.trim_start().len();
        let rest = &after[skipped..];
        self.caret + skipped + rest.find(char::is_whitespace).unwrap_or(rest.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(s: &str) -> TextEdit {
        let mut e = TextEdit::new(100);
        e.insert(s);
        e
    }

    #[test]
    fn inserts_at_the_caret() {
        let mut e = edit("Ciao mondo");
        e.left(true);
        e.insert("bel ");
        assert_eq!(e.text(), "Ciao bel mondo");
        e.home();
        e.insert("» ");
        assert_eq!(e.text(), "» Ciao bel mondo");
        assert_eq!(e.caret_utf16(), 2);
    }

    #[test]
    fn deletes_characters_and_words() {
        let mut e = edit("perché sì");
        e.backspace(false);
        assert_eq!(e.text(), "perché s");
        e.backspace(true);
        assert_eq!(e.text(), "perché ");
        e.backspace(true);
        assert_eq!(e.text(), "");
        e.backspace(false);
        assert_eq!(e.text(), "");

        let mut e = edit("uno due tre");
        e.home();
        e.delete(true);
        assert_eq!(e.text(), " due tre");
        e.delete(false);
        assert_eq!(e.text(), "due tre");
    }

    #[test]
    fn moves_by_words_and_handles_emoji() {
        let mut e = edit("tè 🍵 ok");
        e.left(true);
        assert_eq!(&e.text()[e.caret..], "ok");
        e.left(false);
        e.left(false);
        assert_eq!(e.caret_utf16(), 3);
        e.right(false);
        assert_eq!(e.caret_utf16(), 5, "l'emoji vale due unità UTF-16");
        e.right(true);
        assert_eq!(e.caret, e.text().len());
        e.left(true);
        e.left(true);
        e.left(true);
        assert_eq!(e.caret, 0);
    }

    #[test]
    fn respects_the_limit_and_strips_control_chars() {
        let mut e = TextEdit::new(5);
        e.insert("ab\ncd\u{7}ef");
        assert_eq!(e.text(), "ab cd");
        e.insert("x");
        assert_eq!(e.text(), "ab cd");
    }
}
