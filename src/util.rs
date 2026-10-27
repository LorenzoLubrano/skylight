//! Piccole utilità senza dipendenze dal sistema operativo.

use std::fmt::Write;
use std::sync::Mutex;

/// Formatta millisecondi come `m:ss`, oppure `h:mm:ss` oltre l'ora.
pub fn fmt_time(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Codifica percentuale per un parametro di query string (RFC 3986).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// Coda tra un thread di lavoro (che fa `push`) e il thread dell'interfaccia
/// (che fa `drain`). Il lucchetto è tenuto per pochi nanosecondi.
pub struct Inbox<T>(Mutex<Vec<T>>);

impl<T> Inbox<T> {
    pub const fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    pub fn push(&self, value: T) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(value);
    }

    pub fn drain(&self) -> Vec<T> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl<T> Default for Inbox<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_times() {
        assert_eq!(fmt_time(0), "0:00");
        assert_eq!(fmt_time(9_999), "0:09");
        assert_eq!(fmt_time(193_000), "3:13");
        assert_eq!(fmt_time(3_723_000), "1:02:03");
    }

    #[test]
    fn encodes_query_values() {
        assert_eq!(percent_encode("Bohemian Rhapsody"), "Bohemian%20Rhapsody");
        assert_eq!(percent_encode("AC/DC & co"), "AC%2FDC%20%26%20co");
        assert_eq!(percent_encode("Perché"), "Perch%C3%A9");
        assert_eq!(percent_encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn inbox_drains_in_order() {
        let inbox = Inbox::new();
        inbox.push(1);
        inbox.push(2);
        assert_eq!(inbox.drain(), vec![1, 2]);
        assert!(inbox.drain().is_empty());
    }
}
