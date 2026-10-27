//! Timer Pomodoro: concentrazione, pausa breve e, ogni N sessioni, pausa lunga.
//! I tempi sono millisecondi Unix, così il timer resta giusto anche dopo lo standby.

use crate::settings::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    #[cfg(test)]
    pub const ALL: [Phase; 3] = [Phase::Focus, Phase::ShortBreak, Phase::LongBreak];

    pub fn label(self) -> &'static str {
        match self {
            Phase::Focus => "Concentrazione",
            Phase::ShortBreak => "Pausa breve",
            Phase::LongBreak => "Pausa lunga",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Durations {
    pub focus_ms: u64,
    pub short_ms: u64,
    pub long_ms: u64,
    pub long_every: u32,
}

impl Durations {
    pub fn from_settings(s: &Settings) -> Self {
        let min = |m: u32| m as u64 * 60_000;
        Self {
            focus_ms: min(s.focus_min),
            short_ms: min(s.short_break_min),
            long_ms: min(s.long_break_min),
            long_every: s.long_break_every.max(1),
        }
    }

    pub fn of(&self, phase: Phase) -> u64 {
        match phase {
            Phase::Focus => self.focus_ms,
            Phase::ShortBreak => self.short_ms,
            Phase::LongBreak => self.long_ms,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Run {
    Idle,
    Running { ends_at: u64 },
    Paused { remaining: u64 },
}

#[derive(Clone, Debug)]
pub struct Pomodoro {
    phase: Phase,
    run: Run,
    /// Sessioni di concentrazione completate nel ciclo attuale.
    done_in_cycle: u32,
}

impl Default for Pomodoro {
    fn default() -> Self {
        Self {
            phase: Phase::Focus,
            run: Run::Idle,
            done_in_cycle: 0,
        }
    }
}

impl Pomodoro {
    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn is_running(&self) -> bool {
        matches!(self.run, Run::Running { .. })
    }

    /// In corso o in pausa: c'è qualcosa da mostrare nel notch chiuso.
    pub fn is_active(&self) -> bool {
        !matches!(self.run, Run::Idle)
    }

    pub fn remaining(&self, now: u64, d: &Durations) -> u64 {
        match self.run {
            Run::Idle => d.of(self.phase),
            Run::Running { ends_at } => ends_at.saturating_sub(now),
            Run::Paused { remaining } => remaining,
        }
    }

    /// Frazione già trascorsa della fase, tra 0 e 1.
    pub fn progress(&self, now: u64, d: &Durations) -> f32 {
        let total = d.of(self.phase).max(1);
        1.0 - (self.remaining(now, d).min(total) as f32 / total as f32)
    }

    /// Numero della sessione di concentrazione (da 1) e quante prima della pausa lunga.
    pub fn session(&self, d: &Durations) -> (u32, u32) {
        let n = match self.phase {
            Phase::Focus => self.done_in_cycle + 1,
            Phase::ShortBreak => self.done_in_cycle,
            Phase::LongBreak => d.long_every,
        };
        (n.clamp(1, d.long_every), d.long_every)
    }

    /// Avvia, mette in pausa o riprende.
    pub fn toggle(&mut self, now: u64, d: &Durations) {
        self.run = match self.run {
            Run::Idle => Run::Running {
                ends_at: now + d.of(self.phase),
            },
            Run::Paused { remaining } => Run::Running {
                ends_at: now + remaining,
            },
            Run::Running { ends_at } => Run::Paused {
                remaining: ends_at.saturating_sub(now),
            },
        };
    }

    pub fn reset(&mut self) {
        self.run = Run::Idle;
    }

    pub fn select(&mut self, phase: Phase) {
        self.phase = phase;
        self.run = Run::Idle;
    }

    pub fn skip(&mut self, now: u64, d: &Durations, auto_start: bool) {
        self.advance(now, d, auto_start);
    }

    /// Da chiamare a ogni risveglio: se la fase è finita passa alla successiva
    /// e restituisce quella appena conclusa.
    pub fn tick(&mut self, now: u64, d: &Durations, auto_start: bool) -> Option<Phase> {
        match self.run {
            Run::Running { ends_at } if now >= ends_at => {
                let finished = self.phase;
                self.advance(now, d, auto_start);
                Some(finished)
            }
            _ => None,
        }
    }

    pub fn deadline(&self) -> Option<u64> {
        match self.run {
            Run::Running { ends_at } => Some(ends_at),
            _ => None,
        }
    }

    fn advance(&mut self, now: u64, d: &Durations, auto_start: bool) {
        self.phase = match self.phase {
            Phase::Focus => {
                self.done_in_cycle += 1;
                if self.done_in_cycle >= d.long_every {
                    // Il ciclo riparte entrando nella pausa lunga, anche se poi la si salta.
                    self.done_in_cycle = 0;
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            Phase::LongBreak => {
                self.done_in_cycle = 0;
                Phase::Focus
            }
            Phase::ShortBreak => Phase::Focus,
        };
        self.run = if auto_start {
            Run::Running {
                ends_at: now + d.of(self.phase),
            }
        } else {
            Run::Idle
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;

    fn durations() -> Durations {
        Durations {
            focus_ms: 25 * MIN,
            short_ms: 5 * MIN,
            long_ms: 15 * MIN,
            long_every: 2,
        }
    }

    #[test]
    fn counts_down_and_pauses() {
        let d = durations();
        let mut p = Pomodoro::default();
        assert_eq!(p.remaining(0, &d), 25 * MIN);
        p.toggle(1_000, &d);
        assert!(p.is_running());
        assert_eq!(p.remaining(1_000 + 10 * MIN, &d), 15 * MIN);
        p.toggle(1_000 + 10 * MIN, &d);
        assert!(!p.is_running() && p.is_active());
        assert_eq!(
            p.remaining(1_000 + 60 * MIN, &d),
            15 * MIN,
            "in pausa il tempo è fermo"
        );
        p.toggle(100 * MIN, &d);
        assert_eq!(p.deadline(), Some(115 * MIN));
        assert!((p.progress(100 * MIN, &d) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn cycles_through_phases() {
        let d = durations();
        let mut p = Pomodoro::default();
        p.toggle(0, &d);
        assert_eq!(p.tick(25 * MIN - 1, &d, false), None);
        assert_eq!(p.tick(25 * MIN, &d, false), Some(Phase::Focus));
        assert_eq!(p.phase(), Phase::ShortBreak);
        assert!(!p.is_active(), "senza avvio automatico aspetta l'utente");

        p.toggle(30 * MIN, &d);
        assert_eq!(p.tick(35 * MIN, &d, false), Some(Phase::ShortBreak));
        assert_eq!(p.phase(), Phase::Focus);
        assert_eq!(p.session(&d), (2, 2));

        p.skip(40 * MIN, &d, true);
        assert_eq!(
            p.phase(),
            Phase::LongBreak,
            "dopo 2 sessioni arriva la pausa lunga"
        );
        assert!(p.is_running());
        assert_eq!(p.tick(55 * MIN, &d, false), Some(Phase::LongBreak));
        assert_eq!(p.phase(), Phase::Focus);
        assert_eq!(p.session(&d), (1, 2));
    }

    #[test]
    fn leaving_the_long_break_early_starts_a_new_cycle() {
        let d = durations();
        let mut p = Pomodoro::default();
        p.skip(0, &d, false);
        p.skip(0, &d, false);
        p.skip(0, &d, false);
        assert_eq!(p.phase(), Phase::LongBreak);
        p.select(Phase::Focus);
        assert_eq!(p.session(&d), (1, 2));
        p.toggle(0, &d);
        assert_eq!(p.tick(25 * MIN, &d, false), Some(Phase::Focus));
        assert_eq!(p.phase(), Phase::ShortBreak, "non un'altra pausa lunga");
    }

    #[test]
    fn select_and_reset_stop_the_timer() {
        let d = durations();
        let mut p = Pomodoro::default();
        p.toggle(0, &d);
        p.select(Phase::ShortBreak);
        assert!(!p.is_active());
        assert_eq!(p.remaining(0, &d), 5 * MIN);
        p.toggle(0, &d);
        p.reset();
        assert_eq!(p.deadline(), None);
    }
}
