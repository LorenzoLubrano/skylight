//! Lista di cose da fare con promemoria. L'orario si scrive nel testo:
//! "Chiama Dina alle 15:30", "Palestra domani alle 8", "Tè tra 10 min", "Post +1h".

use serde::{Deserialize, Serialize};

use crate::time::{Clock, HOUR, MINUTE};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub done_at: Option<i64>,
    /// Promemoria, in secondi Unix.
    #[serde(default)]
    pub due: Option<i64>,
    /// Il promemoria è già stato mostrato.
    #[serde(default)]
    pub notified: bool,
    pub created: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TodoList {
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    next_id: u64,
}

impl TodoList {
    pub fn add(&mut self, title: String, due: Option<i64>, now: i64) -> u64 {
        self.next_id = self
            .next_id
            .max(self.tasks.iter().map(|t| t.id).max().unwrap_or(0))
            + 1;
        let id = self.next_id;
        self.tasks.push(Task {
            id,
            title,
            done: false,
            done_at: None,
            due,
            notified: false,
            created: now,
        });
        id
    }

    pub fn get(&self, id: u64) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub fn toggle(&mut self, id: u64, now: i64) {
        if let Some(t) = self.get_mut(id) {
            t.done = !t.done;
            t.done_at = t.done.then_some(now);
        }
    }

    pub fn complete(&mut self, id: u64, now: i64) {
        if let Some(t) = self.get_mut(id).filter(|t| !t.done) {
            t.done = true;
            t.done_at = Some(now);
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.tasks.retain(|t| t.id != id);
    }

    pub fn snooze(&mut self, id: u64, until: i64) {
        if let Some(t) = self.get_mut(id) {
            t.due = Some(until);
            t.notified = false;
        }
    }

    pub fn mark_notified(&mut self, id: u64) {
        if let Some(t) = self.get_mut(id) {
            t.notified = true;
        }
    }

    /// Attività il cui promemoria è scattato e non è ancora stato mostrato.
    pub fn due_now(&self, now: i64) -> Vec<u64> {
        self.pending()
            .filter(|t| t.due.is_some_and(|d| d <= now))
            .map(|t| t.id)
            .collect()
    }

    pub fn next_due(&self) -> Option<i64> {
        self.pending().filter_map(|t| t.due).min()
    }

    fn pending(&self) -> impl Iterator<Item = &Task> {
        self.tasks.iter().filter(|t| !t.done && !t.notified)
    }

    /// Da fare prima (per orario, poi per creazione), quelle fatte in fondo.
    pub fn ordered(&self) -> Vec<&Task> {
        let mut todo: Vec<&Task> = self.tasks.iter().filter(|t| !t.done).collect();
        todo.sort_by_key(|t| (t.due.is_none(), t.due, t.created, t.id));
        let mut done: Vec<&Task> = self.tasks.iter().filter(|t| t.done).collect();
        done.sort_by_key(|t| std::cmp::Reverse(t.done_at));
        todo.extend(done);
        todo
    }

    pub fn done_today(&self, clock: &Clock) -> usize {
        let today = clock.today();
        self.tasks
            .iter()
            .filter(|t| t.done_at.is_some_and(|d| clock.day_of(d) == today))
            .count()
    }

    pub fn has_done(&self) -> bool {
        self.tasks.iter().any(|t| t.done)
    }

    pub fn clear_done(&mut self) {
        self.tasks.retain(|t| !t.done);
    }

    /// Le attività completate nei giorni precedenti spariscono da sole.
    pub fn prune(&mut self, clock: &Clock) {
        let today = clock.today();
        self.tasks
            .retain(|t| !t.done || t.done_at.is_some_and(|d| clock.day_of(d) >= today));
    }
}

/// Separa il titolo dall'eventuale orario scritto in fondo al testo.
pub fn parse_entry(input: &str, clock: &Clock) -> (String, Option<i64>) {
    let words: Vec<&str> = input.split_whitespace().collect();
    let (used, due) = if let Some((used, seconds)) = relative_suffix(&words) {
        (used, Some(clock.now + seconds))
    } else if let Some(time) = time_suffix(&words) {
        let today = clock.today();
        let due = match time.day {
            When::Next => clock.next_time_of_day(time.minute),
            When::In(days) => clock.at(today + days, time.minute),
            When::Weekday(w) => {
                let ahead = (w as i64 - Clock::weekday(today) as i64).rem_euclid(7);
                let t = clock.at(today + ahead, time.minute);
                if t > clock.now {
                    t
                } else {
                    clock.at(today + ahead + 7, time.minute)
                }
            }
        };
        (time.used, Some(due))
    } else {
        (0, None)
    };
    let title = words[..words.len() - used].join(" ");
    let title = if title.is_empty() {
        words.join(" ")
    } else {
        title
    };
    (title, due)
}

/// Per quale giorno vale l'orario scritto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum When {
    /// Nessun giorno indicato: la prossima volta che sarà quell'ora.
    Next,
    /// Tra N giorni (0 = oggi).
    In(i64),
    /// Il prossimo giorno della settimana indicato (0 = lunedì), oggi se l'ora non è passata.
    Weekday(u32),
}

struct TimeSuffix {
    /// Quante parole finali descrivono l'orario.
    used: usize,
    minute: u32,
    day: When,
}

fn is_at_word(w: &str) -> bool {
    matches!(w.to_lowercase().as_str(), "alle" | "ore" | "at" | "@" | "h")
}

fn day_word(w: &str) -> Option<When> {
    let w = w.to_lowercase();
    let day = match w.trim_end_matches(['ì', 'i']) {
        "luned" => When::Weekday(0),
        "marted" => When::Weekday(1),
        "mercoled" => When::Weekday(2),
        "gioved" => When::Weekday(3),
        "venerd" => When::Weekday(4),
        _ => match w.as_str() {
            "sabato" => When::Weekday(5),
            "domenica" => When::Weekday(6),
            "oggi" | "today" | "stasera" | "stamattina" | "tonight" => When::In(0),
            "domani" | "tomorrow" => When::In(1),
            "dopodomani" => When::In(2),
            _ => return None,
        },
    };
    Some(day)
}

/// "pomeriggio", "sera", "stasera": "alle 9" diventa le 21.
fn is_afternoon_word(w: &str) -> bool {
    matches!(
        w.to_lowercase().as_str(),
        "pomeriggio" | "sera" | "stasera" | "tonight"
    )
}

fn is_part_of_day(w: &str) -> bool {
    is_afternoon_word(w) || matches!(w.to_lowercase().as_str(), "mattina" | "stamattina")
}

fn is_in_word(w: &str) -> bool {
    matches!(w.to_lowercase().as_str(), "tra" | "fra" | "in")
}

/// "15:30" sempre; "9.30" e "9" solo dopo "alle"/"ore"/"at".
fn parse_clock(token: &str, after_at: bool) -> Option<u32> {
    let (token, after_at) = match token.strip_prefix('@') {
        Some(t) => (t, true),
        None => (token, after_at),
    };
    let (h, m) = match token.split_once([':', '.']) {
        Some((h, m)) if token.contains(':') || after_at => (h, m),
        Some(_) => return None,
        None if after_at => (token, "00"),
        None => return None,
    };
    let digits = |s: &str| !s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(h) || m.len() != 2 || !digits(m) {
        return None;
    }
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Orario scritto in fondo: "alle 15", "domani alle ore 8:15", "venerdì alle 12",
/// "stasera alle 9", "dopodomani mattina alle 7".
fn time_suffix(words: &[&str]) -> Option<TimeSuffix> {
    let n = words.len();
    let last = *words.last()?;
    let after_at = n >= 2 && is_at_word(words[n - 2]);
    let mut minute = parse_clock(last, after_at)?;
    let mut used = 1;
    // "alle", "alle ore", "ore"
    while n > used && is_at_word(words[n - used - 1]) {
        used += 1;
    }
    let before = |used: usize| (n > used).then(|| words[n - used - 1]);
    let mut day = When::Next;
    if let Some(w) = before(used)
        && is_part_of_day(w)
    {
        if is_afternoon_word(w) && minute < 12 * 60 {
            minute += 12 * 60;
        }
        if let Some(d) = day_word(w) {
            day = d;
        }
        used += 1;
    }
    if day == When::Next
        && let Some(d) = before(used).and_then(day_word)
    {
        day = d;
        used += 1;
    }
    Some(TimeSuffix { used, minute, day })
}

fn unit_seconds(unit: &str) -> Option<i64> {
    match unit.to_lowercase().as_str() {
        "m" | "min" | "mins" | "minuto" | "minuti" | "minute" | "minutes" => Some(MINUTE),
        "h" | "ora" | "ore" | "hour" | "hours" => Some(HOUR),
        _ => None,
    }
}

/// "tra 20 min", "in 2 ore", "fra 5m", "+1h", "+10 min".
fn relative_suffix(words: &[&str]) -> Option<(usize, i64)> {
    let n = words.len();
    let number = |s: &str| -> Option<(i64, bool)> {
        let (s, plus) = match s.strip_prefix('+') {
            Some(rest) => (rest, true),
            None => (s, false),
        };
        let v: i64 = s.parse().ok()?;
        (v > 0 && v <= 10_000).then_some((v, plus))
    };
    let preceded_by_in = |i: usize| i > 0 && is_in_word(words[i - 1]);

    // Numero e unità attaccati: "20min", "+1h"
    let last = *words.last()?;
    let split = last
        .trim_start_matches('+')
        .find(|c: char| !c.is_ascii_digit());
    if let Some(at) = split.map(|i| i + (last.len() - last.trim_start_matches('+').len()))
        && let (Some((v, plus)), Some(unit)) = (number(&last[..at]), unit_seconds(&last[at..]))
    {
        if plus {
            return Some((1, v * unit));
        }
        if preceded_by_in(n - 1) {
            return Some((2, v * unit));
        }
    }
    // Numero e unità separati: "20 min", "+10 min"
    if n >= 2
        && let (Some((v, plus)), Some(unit)) = (number(words[n - 2]), unit_seconds(last))
    {
        if plus {
            return Some((2, v * unit));
        }
        if preceded_by_in(n - 2) {
            return Some((3, v * unit));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::DAY;

    // 10:00 locali (UTC+1)
    const MIDNIGHT: i64 = 1_792_972_800 - HOUR;
    fn clock() -> Clock {
        Clock::new(MIDNIGHT + 10 * HOUR, HOUR)
    }
    fn at(h: i64, m: i64) -> i64 {
        MIDNIGHT + h * HOUR + m * MINUTE
    }

    #[test]
    fn parses_times_and_relative_delays() {
        let c = clock();
        let cases: &[(&str, &str, Option<i64>)] = &[
            ("Chiama Dina alle 15:30", "Chiama Dina", Some(at(15, 30))),
            ("Report 9:00", "Report", Some(at(9, 0) + DAY)),
            (
                "Palestra domani alle 8:15",
                "Palestra",
                Some(at(8, 15) + DAY),
            ),
            ("Leggere alle 9", "Leggere", Some(at(9, 0) + DAY)),
            ("Leggere alle 11.45", "Leggere", Some(at(11, 45))),
            ("Pausa @14:05", "Pausa", Some(at(14, 5))),
            ("Pausa tra 20 min", "Pausa", Some(c.now + 20 * MINUTE)),
            ("Forno fra 5m", "Forno", Some(c.now + 5 * MINUTE)),
            ("Post +1h", "Post", Some(c.now + HOUR)),
            ("Tè +10 min", "Tè", Some(c.now + 10 * MINUTE)),
            ("Riunione in 2 ore", "Riunione", Some(c.now + 2 * HOUR)),
            ("Comprare latte", "Comprare latte", None),
            ("Rilascio 2.10", "Rilascio 2.10", None),
            ("Capitolo 20", "Capitolo 20", None),
            ("15:30", "15:30", Some(at(15, 30))),
            ("Sveglia 25:00", "Sveglia 25:00", None),
            ("Riunione alle ore 15", "Riunione", Some(at(15, 0))),
            (
                "Riunione domani alle ore 15",
                "Riunione",
                Some(at(15, 0) + DAY),
            ),
            ("Dentista ore 16:30", "Dentista", Some(at(16, 30))),
            // Oggi è lunedì 26 ottobre, ore 10:00.
            (
                "Consegna venerdì alle 12",
                "Consegna",
                Some(at(12, 0) + 4 * DAY),
            ),
            (
                "Consegna venerdi alle 12",
                "Consegna",
                Some(at(12, 0) + 4 * DAY),
            ),
            ("Calcetto lunedì alle 21", "Calcetto", Some(at(21, 0))),
            ("Spesa lunedì alle 9", "Spesa", Some(at(9, 0) + 7 * DAY)),
            ("Messa domenica alle 11", "Messa", Some(at(11, 0) + 6 * DAY)),
            (
                "Palestra dopodomani alle 8",
                "Palestra",
                Some(at(8, 0) + 2 * DAY),
            ),
            ("Cena stasera alle 9", "Cena", Some(at(21, 0))),
            ("Film sera alle 9:30", "Film", Some(at(21, 30))),
            ("Corsa domani mattina alle 7", "Corsa", Some(at(7, 0) + DAY)),
            ("Chiamata oggi alle 9", "Chiamata", Some(at(9, 0))),
            ("domani alle 8", "domani alle 8", Some(at(8, 0) + DAY)),
        ];
        for (input, title, due) in cases {
            assert_eq!(
                parse_entry(input, &c),
                (title.to_string(), *due),
                "input: {input}"
            );
        }
    }

    #[test]
    fn reminders_fire_once_and_can_be_snoozed() {
        let mut list = TodoList::default();
        let a = list.add("A".into(), Some(100), 0);
        let b = list.add("B".into(), Some(200), 0);
        let _c = list.add("C".into(), None, 0);
        assert_eq!(list.next_due(), Some(100));
        assert_eq!(list.due_now(150), vec![a]);
        list.mark_notified(a);
        assert_eq!(list.due_now(150), Vec::<u64>::new());
        assert_eq!(list.next_due(), Some(200));
        list.snooze(a, 300);
        assert_eq!(list.due_now(250), vec![b]);
        list.complete(b, 250);
        assert_eq!(list.next_due(), Some(300));
    }

    #[test]
    fn ordering_and_counts() {
        let c = clock();
        let mut list = TodoList::default();
        let late = list.add("tardi".into(), Some(c.now + 2 * HOUR), 1);
        let soon = list.add("presto".into(), Some(c.now + HOUR), 2);
        let none = list.add("senza orario".into(), None, 3);
        let done = list.add("fatta".into(), None, 4);
        list.toggle(done, c.now);
        let ids: Vec<u64> = list.ordered().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![soon, late, none, done]);
        assert_eq!(list.done_today(&c), 1);

        list.toggle(done, c.now);
        assert_eq!(list.done_today(&c), 0);
        list.complete(done, c.now - DAY);
        list.prune(&c);
        assert!(
            list.get(done).is_none(),
            "le attività fatte ieri spariscono"
        );
        assert_eq!(list.tasks.len(), 3);
    }

    #[test]
    fn ids_stay_unique_after_removal() {
        let mut list = TodoList::default();
        let a = list.add("a".into(), None, 0);
        list.remove(a);
        let b = list.add("b".into(), None, 0);
        assert_ne!(a, b);
    }
}
