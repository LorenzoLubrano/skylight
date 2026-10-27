//! Tempo "da calendario": secondi Unix (UTC) più lo scarto del fuso orario.
//! Promemoria, acqua e to-do lavorano su questo, così sono testabili ovunque.

use std::time::{SystemTime, UNIX_EPOCH};

pub const MINUTE: i64 = 60;
pub const HOUR: i64 = 3600;
pub const DAY: i64 = 86_400;

/// Adesso, in millisecondi Unix.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[derive(Clone, Copy, Debug)]
pub struct Clock {
    /// Adesso, in secondi Unix (UTC).
    pub now: i64,
    /// Ora locale meno ora UTC adesso, in secondi (es. +7200 in Italia d'estate).
    pub offset: i64,
    /// Scarto in un altro istante, per i giorni dall'altra parte del cambio dell'ora.
    /// Senza, vale sempre `offset`.
    zone: Option<fn(i64) -> i64>,
}

impl Clock {
    pub fn new(now: i64, offset: i64) -> Self {
        Self {
            now,
            offset,
            zone: None,
        }
    }

    pub fn with_zone(mut self, zone: fn(i64) -> i64) -> Self {
        self.zone = Some(zone);
        self
    }

    fn offset_at(&self, t: i64) -> i64 {
        self.zone.map_or(self.offset, |zone| zone(t))
    }

    /// Giorno locale (giorni dal 1° gennaio 1970) dell'istante `t`.
    pub fn day_of(&self, t: i64) -> i64 {
        (t + self.offset_at(t)).div_euclid(DAY)
    }

    pub fn today(&self) -> i64 {
        self.day_of(self.now)
    }

    /// Minuti dalla mezzanotte locale.
    pub fn minute_of_day(&self, t: i64) -> u32 {
        ((t + self.offset_at(t)).rem_euclid(DAY) / MINUTE) as u32
    }

    /// Istante UTC che corrisponde al minuto `minute` del giorno locale `day`,
    /// con lo scarto in vigore quel giorno (ora legale o solare).
    pub fn at(&self, day: i64, minute: u32) -> i64 {
        let local = day * DAY + minute as i64 * MINUTE;
        let guess = local - self.offset;
        local - self.offset_at(guess)
    }

    /// Prossima volta in cui sarà hh:mm: oggi se non è ancora passato, altrimenti domani.
    pub fn next_time_of_day(&self, minute: u32) -> i64 {
        let t = self.at(self.today(), minute);
        if t > self.now {
            t
        } else {
            self.at(self.today() + 1, minute)
        }
    }

    /// Giorno della settimana di un giorno locale: 0 = lunedì … 6 = domenica.
    pub fn weekday(day: i64) -> u32 {
        // Il 1° gennaio 1970 era un giovedì.
        (day + 3).rem_euclid(7) as u32
    }
}

/// Data del calendario (anno, mese, giorno) per un giorno contato dal 1970.
/// Algoritmo "civil from days" di Howard Hinnant.
pub fn civil(day: i64) -> (i64, u32, u32) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// "09:05"
pub fn fmt_hm(minute_of_day: u32) -> String {
    format!("{:02}:{:02}", minute_of_day / 60 % 24, minute_of_day % 60)
}

/// Conto alla rovescia "mm:ss", arrotondato per eccesso (25:00 appena partito).
pub fn fmt_countdown(ms: u64) -> String {
    let s = ms.div_ceil(1000);
    format!("{:02}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Lunedì 26 ottobre 2026, 10:00 in Italia (UTC+1 dopo il cambio d'ora).
    const NOW: i64 = 1_792_972_800 + 9 * HOUR;

    #[test]
    fn local_day_and_minute() {
        let c = Clock::new(NOW, HOUR);
        assert_eq!(c.minute_of_day(NOW), 10 * 60);
        let midnight = c.at(c.today(), 0);
        assert_eq!(c.minute_of_day(midnight), 0);
        assert_eq!(c.day_of(midnight - 1), c.today() - 1);
    }

    #[test]
    fn negative_offsets_work() {
        let c = Clock::new(NOW, -5 * HOUR);
        assert_eq!(c.minute_of_day(NOW), 4 * 60);
        assert_eq!(c.at(c.today(), 4 * 60), NOW);
    }

    #[test]
    fn next_time_of_day_rolls_to_tomorrow() {
        let c = Clock::new(NOW, HOUR);
        assert_eq!(c.next_time_of_day(15 * 60), NOW + 5 * HOUR);
        assert_eq!(c.next_time_of_day(9 * 60), NOW - HOUR + DAY);
        assert_eq!(c.next_time_of_day(10 * 60), NOW + DAY);
    }

    #[test]
    fn days_across_the_dst_change_use_their_own_offset() {
        // Fine dell'ora legale in Italia: domenica 25 ottobre 2026, 01:00 UTC.
        const SWITCH: i64 = 1_792_886_400 + HOUR;
        fn rome(t: i64) -> i64 {
            if t < SWITCH { 2 * HOUR } else { HOUR }
        }
        // Sabato 24 alle 22:00 (UTC+2)
        let c = Clock::new(SWITCH - 5 * HOUR, 2 * HOUR).with_zone(rome);
        assert_eq!(c.minute_of_day(c.now), 22 * 60);
        // "domani alle 8:15" = 08:15 di ora solare = 07:15 UTC
        let t = c.at(c.today() + 1, 8 * 60 + 15);
        assert_eq!(t, SWITCH - HOUR + 7 * HOUR + 15 * MINUTE);
        assert_eq!(c.minute_of_day(t), 8 * 60 + 15);
        assert_eq!(c.day_of(t), c.today() + 1);
        assert_eq!(c.next_time_of_day(9 * 60), c.at(c.today() + 1, 9 * 60));
    }

    #[test]
    fn weekdays() {
        let c = Clock::new(NOW, HOUR);
        assert_eq!(Clock::weekday(c.today()), 0, "26 ottobre 2026 è lunedì");
        assert_eq!(Clock::weekday(0), 3, "1° gennaio 1970 era giovedì");
    }

    #[test]
    fn calendar_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(Clock::new(NOW, HOUR).today()), (2026, 10, 26));
        assert_eq!(civil(-1), (1969, 12, 31));
        assert_eq!(civil(19_782), (2024, 2, 29));
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_hm(9 * 60 + 5), "09:05");
        assert_eq!(fmt_countdown(1_500_000), "25:00");
        assert_eq!(fmt_countdown(1_499_001), "25:00");
        assert_eq!(fmt_countdown(59_000), "00:59");
        assert_eq!(fmt_countdown(0), "00:00");
    }
}
