//! Promemoria per bere: registro dei bicchieri, totale del giorno, grafico per
//! ora e calcolo del prossimo promemoria dentro la fascia oraria scelta.

use serde::{Deserialize, Serialize};

use crate::settings::Settings;
use crate::time::{Clock, MINUTE};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drink {
    /// Secondi Unix.
    pub at: i64,
    pub ml: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaterLog {
    #[serde(default)]
    pub drinks: Vec<Drink>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaterPlan {
    pub enabled: bool,
    pub goal_ml: u32,
    pub glass_ml: u32,
    pub interval_min: u32,
    pub from_h: u32,
    pub to_h: u32,
}

impl WaterPlan {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            enabled: s.water_reminders,
            goal_ml: s.water_goal_ml,
            glass_ml: s.water_glass_ml,
            interval_min: s.water_interval_min.max(1),
            from_h: s.water_from_h.min(23),
            to_h: s.water_to_h.clamp(s.water_from_h + 1, 24),
        }
    }
}

impl WaterLog {
    pub fn drink(&mut self, at: i64, ml: u32) {
        self.drinks.push(Drink { at, ml });
    }

    /// Annulla l'ultimo bicchiere di oggi.
    pub fn undo_today(&mut self, clock: &Clock) -> bool {
        let today = clock.today();
        match self
            .drinks
            .iter()
            .rposition(|d| clock.day_of(d.at) == today)
        {
            Some(i) => {
                self.drinks.remove(i);
                true
            }
            None => false,
        }
    }

    pub fn total(&self, clock: &Clock, day: i64) -> u32 {
        self.drinks
            .iter()
            .filter(|d| clock.day_of(d.at) == day)
            .map(|d| d.ml)
            .sum()
    }

    pub fn last_today(&self, clock: &Clock) -> Option<i64> {
        let today = clock.today();
        self.drinks
            .iter()
            .filter(|d| clock.day_of(d.at) == today)
            .map(|d| d.at)
            .max()
    }

    /// Millilitri bevuti in ciascuna ora tra `from_h` (inclusa) e `to_h` (esclusa).
    pub fn hourly(&self, clock: &Clock, day: i64, from_h: u32, to_h: u32) -> Vec<u32> {
        let mut hours = vec![0; to_h.saturating_sub(from_h) as usize];
        for d in self.drinks.iter().filter(|d| clock.day_of(d.at) == day) {
            let hour = clock.minute_of_day(d.at) / 60;
            if let Some(slot) = hour
                .checked_sub(from_h)
                .and_then(|i| hours.get_mut(i as usize))
            {
                *slot += d.ml;
            }
        }
        hours
    }

    /// Tiene solo gli ultimi `days` giorni.
    pub fn prune(&mut self, clock: &Clock, days: i64) {
        let first = clock.today() - days + 1;
        self.drinks.retain(|d| clock.day_of(d.at) >= first);
    }
}

/// Quando ricordare di bere. Un intervallo dopo l'ultimo bicchiere (o dopo
/// l'inizio della fascia oraria), mai fuori fascia e mai a obiettivo raggiunto.
/// Se l'orario è già passato restituisce `now`.
pub fn next_reminder(
    log: &WaterLog,
    plan: &WaterPlan,
    clock: &Clock,
    snoozed_until: Option<i64>,
) -> Option<i64> {
    if !plan.enabled {
        return None;
    }
    let interval = plan.interval_min as i64 * MINUTE;
    let today = clock.today();
    let start = clock.at(today, plan.from_h * 60);
    let end = clock.at(today, plan.to_h * 60);

    if log.total(clock, today) < plan.goal_ml && clock.now < end {
        let anchor = log.last_today(clock).unwrap_or(start).max(start);
        let mut due = anchor + interval;
        if let Some(snooze) = snoozed_until {
            due = due.max(snooze);
        }
        if due < end {
            return Some(due.max(clock.now));
        }
    }
    Some(clock.at(today + 1, plan.from_h * 60) + interval)
}

/// Litri con una cifra decimale e virgola: "1,25 L" per 1250 ml.
pub fn fmt_liters(ml: u32) -> String {
    let cl = (ml + 5) / 10; // centilitri arrotondati
    let (whole, frac) = (cl / 100, cl % 100);
    if frac == 0 {
        format!("{whole} L")
    } else if frac % 10 == 0 {
        format!("{whole},{} L", frac / 10)
    } else {
        format!("{whole},{frac:02} L")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{DAY, HOUR};

    const MIDNIGHT: i64 = 1_792_972_800 - HOUR; // mezzanotte locale con offset +1h
    fn clock_at(h: i64, m: i64) -> Clock {
        Clock::new(MIDNIGHT + h * HOUR + m * MINUTE, HOUR)
    }
    fn plan() -> WaterPlan {
        WaterPlan {
            enabled: true,
            goal_ml: 1000,
            glass_ml: 250,
            interval_min: 60,
            from_h: 9,
            to_h: 21,
        }
    }

    #[test]
    fn totals_and_undo_work_per_day() {
        let c = clock_at(12, 0);
        let mut log = WaterLog::default();
        log.drink(c.now - DAY, 500); // ieri
        log.drink(c.now - HOUR, 250);
        log.drink(c.now, 250);
        assert_eq!(log.total(&c, c.today()), 500);
        assert!(log.undo_today(&c));
        assert_eq!(log.total(&c, c.today()), 250);
        assert!(log.undo_today(&c));
        assert!(!log.undo_today(&c), "non si annulla quello di ieri");
        assert_eq!(log.drinks.len(), 1);
    }

    #[test]
    fn hourly_chart_buckets_drinks() {
        let c = clock_at(15, 0);
        let mut log = WaterLog::default();
        log.drink(clock_at(9, 10).now, 250);
        log.drink(clock_at(9, 50).now, 250);
        log.drink(clock_at(14, 0).now, 300);
        log.drink(clock_at(7, 0).now, 100); // fuori fascia
        let h = log.hourly(&c, c.today(), 9, 21);
        assert_eq!(h.len(), 12);
        assert_eq!(h[0], 500);
        assert_eq!(h[5], 300);
        assert_eq!(h.iter().sum::<u32>(), 800);
    }

    #[test]
    fn reminder_after_interval_from_last_drink() {
        let mut log = WaterLog::default();
        log.drink(clock_at(10, 30).now, 250);
        let c = clock_at(11, 0);
        assert_eq!(
            next_reminder(&log, &plan(), &c, None),
            Some(clock_at(11, 30).now)
        );
        assert_eq!(
            next_reminder(&log, &plan(), &c, Some(clock_at(11, 45).now)),
            Some(clock_at(11, 45).now)
        );
    }

    #[test]
    fn overdue_reminder_fires_now() {
        let c = clock_at(15, 0);
        let due = next_reminder(&WaterLog::default(), &plan(), &c, None);
        assert_eq!(due, Some(c.now));
    }

    #[test]
    fn no_reminders_outside_hours_or_after_goal() {
        let tomorrow_first = clock_at(24 + 10, 0).now;
        let c = clock_at(20, 30);
        assert_eq!(
            next_reminder(&WaterLog::default(), &plan(), &c, None),
            Some(c.now)
        );
        let late = clock_at(21, 30);
        assert_eq!(
            next_reminder(&WaterLog::default(), &plan(), &late, None),
            Some(tomorrow_first)
        );
        let early = clock_at(7, 0);
        assert_eq!(
            next_reminder(&WaterLog::default(), &plan(), &early, None),
            Some(clock_at(10, 0).now)
        );

        let mut full = WaterLog::default();
        full.drink(clock_at(9, 0).now, 1000);
        assert_eq!(
            next_reminder(&full, &plan(), &clock_at(12, 0), None),
            Some(tomorrow_first)
        );
        let off = WaterPlan {
            enabled: false,
            ..plan()
        };
        assert_eq!(next_reminder(&WaterLog::default(), &off, &c, None), None);
    }

    #[test]
    fn formats_liters() {
        assert_eq!(fmt_liters(0), "0 L");
        assert_eq!(fmt_liters(1500), "1,5 L");
        assert_eq!(fmt_liters(1250), "1,25 L");
        assert_eq!(fmt_liters(2000), "2 L");
        assert_eq!(fmt_liters(333), "0,33 L");
    }

    #[test]
    fn prune_keeps_recent_days() {
        let c = clock_at(12, 0);
        let mut log = WaterLog::default();
        for days_ago in 0..10 {
            log.drink(c.now - days_ago * DAY, 100);
        }
        log.prune(&c, 7);
        assert_eq!(log.drinks.len(), 7);
    }
}
