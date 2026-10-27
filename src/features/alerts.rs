//! Avvisi mostrati nel notch: volume, batteria, Wi-Fi, promemoria...
//! Quelli informativi spariscono da soli; quelli con pulsanti restano finché
//! l'utente risponde (o per un po'), e intanto gli altri aspettano in coda.

use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertKind {
    Volume,
    Brightness,
    Keyboard,
    Power,
    Network,
    Bluetooth,
    Focus,
    Water,
    Todo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertAction {
    CompleteTask(u64),
    SnoozeTask(u64),
    Drink,
    SnoozeWater,
    StartFocusPhase,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub kind: AlertKind,
    pub glyph: char,
    pub title: String,
    pub detail: String,
    /// Barra di livello (volume, luminosità, batteria) tra 0 e 1.
    pub level: Option<f32>,
    pub actions: Vec<(String, AlertAction)>,
}

impl Alert {
    pub fn new(
        kind: AlertKind,
        glyph: char,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            glyph,
            title: title.into(),
            detail: detail.into(),
            level: None,
            actions: Vec::new(),
        }
    }

    pub fn with_level(mut self, level: f32) -> Self {
        self.level = Some(level.clamp(0.0, 1.0));
        self
    }

    pub fn with_action(mut self, label: impl Into<String>, action: AlertAction) -> Self {
        self.actions.push((label.into(), action));
        self
    }

    pub fn is_actionable(&self) -> bool {
        !self.actions.is_empty()
    }

    /// Quanto resta visibile, in millisecondi.
    pub fn lifetime_ms(&self) -> u64 {
        if self.is_actionable() {
            20_000
        } else if self.level.is_some() && self.detail.is_empty() {
            1_600
        } else {
            3_200
        }
    }
}

/// Scadenza di un avviso fermo in attesa di essere visto.
const FROZEN: u64 = u64::MAX;

#[derive(Debug)]
struct Shown {
    alert: Alert,
    expires: u64,
}

#[derive(Debug, Default)]
pub struct Alerts {
    current: Option<Shown>,
    queue: VecDeque<Alert>,
}

impl Alerts {
    pub fn push(&mut self, alert: Alert, now: u64) {
        let expires = now + alert.lifetime_ms();
        match &mut self.current {
            // Lo stesso tipo si aggiorna sul posto: il volume che sale non crea dieci avvisi.
            Some(shown)
                if shown.alert.kind == alert.kind
                    && !shown.alert.is_actionable()
                    && !alert.is_actionable() =>
            {
                *shown = Shown { alert, expires };
            }
            // Mentre si aspetta una risposta, gli avvisi informativi si perdono, gli altri aspettano.
            Some(shown) if shown.alert.is_actionable() => {
                if alert.is_actionable() {
                    self.queue.push_back(alert);
                }
            }
            _ => self.current = Some(Shown { alert, expires }),
        }
    }

    pub fn current(&self) -> Option<&Alert> {
        self.current.as_ref().map(|s| &s.alert)
    }

    pub fn deadline(&self) -> Option<u64> {
        self.current
            .as_ref()
            .map(|s| s.expires)
            .filter(|&t| t != FROZEN)
    }

    /// Fa scadere l'avviso corrente; restituisce `true` se è cambiato qualcosa.
    pub fn tick(&mut self, now: u64) -> bool {
        match &self.current {
            Some(shown) if now >= shown.expires => {
                self.dismiss(now);
                true
            }
            _ => false,
        }
    }

    pub fn dismiss(&mut self, now: u64) {
        self.current = self.queue.pop_front().map(|alert| Shown {
            expires: now + alert.lifetime_ms(),
            alert,
        });
    }

    /// Il mouse ci è sopra: non farlo sparire sotto il cursore.
    pub fn hold(&mut self, now: u64) {
        if let Some(shown) = &mut self.current {
            shown.expires = shown.expires.max(now + 1_500);
        }
    }

    /// Il notch è aperto o nascosto: un avviso con pulsanti non si vede, quindi
    /// non deve scadere. Riparte con `resume` quando torna visibile.
    pub fn freeze(&mut self) {
        if let Some(shown) = self.current.as_mut().filter(|s| s.alert.is_actionable()) {
            shown.expires = FROZEN;
        }
    }

    /// L'avviso è di nuovo visibile: se era fermo, ha di nuovo tutto il suo tempo.
    pub fn resume(&mut self, now: u64) {
        if let Some(shown) = self.current.as_mut().filter(|s| s.expires == FROZEN) {
            shown.expires = now + shown.alert.lifetime_ms();
        }
    }

    /// Toglie gli avvisi di un certo tipo (es. il promemoria di un'attività già fatta).
    pub fn retain(&mut self, now: u64, keep: impl Fn(&Alert) -> bool) {
        self.queue.retain(&keep);
        if self.current.as_ref().is_some_and(|s| !keep(&s.alert)) {
            self.dismiss(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(level: f32) -> Alert {
        Alert::new(AlertKind::Volume, 'v', "Volume", "").with_level(level)
    }
    fn reminder(id: u64) -> Alert {
        Alert::new(AlertKind::Todo, 't', "È ora", "x")
            .with_action("Fatto", AlertAction::CompleteTask(id))
    }

    #[test]
    fn same_kind_updates_in_place() {
        let mut a = Alerts::default();
        a.push(volume(0.2), 0);
        a.push(volume(0.3), 1_000);
        assert_eq!(a.current().unwrap().level, Some(0.3));
        assert_eq!(a.deadline(), Some(2_600));
        assert!(!a.tick(2_599));
        assert!(a.tick(2_600));
        assert!(a.current().is_none());
    }

    #[test]
    fn actionable_alerts_wait_in_line() {
        let mut a = Alerts::default();
        a.push(reminder(1), 0);
        a.push(volume(0.5), 10);
        a.push(reminder(2), 20);
        assert_eq!(
            a.current().unwrap().actions[0].1,
            AlertAction::CompleteTask(1)
        );
        a.dismiss(30);
        assert_eq!(
            a.current().unwrap().actions[0].1,
            AlertAction::CompleteTask(2)
        );
        a.dismiss(40);
        assert!(a.current().is_none(), "il volume intanto si è perso");
    }

    #[test]
    fn new_alert_replaces_informative_one() {
        let mut a = Alerts::default();
        a.push(volume(0.5), 0);
        a.push(reminder(7), 10);
        assert!(a.current().unwrap().is_actionable());
    }

    #[test]
    fn frozen_alerts_wait_to_be_seen() {
        let mut a = Alerts::default();
        a.push(volume(0.5), 0);
        a.freeze();
        assert_eq!(
            a.deadline(),
            Some(1_600),
            "gli avvisi informativi scadono comunque"
        );
        a.push(reminder(1), 100);
        a.freeze();
        a.hold(5_000);
        assert_eq!(a.deadline(), None);
        assert!(!a.tick(10_000_000));
        a.resume(60_000);
        assert_eq!(
            a.deadline(),
            Some(80_000),
            "tutto il suo tempo da quando si vede"
        );
        a.resume(70_000);
        assert_eq!(a.deadline(), Some(80_000));
    }

    #[test]
    fn hold_and_retain() {
        let mut a = Alerts::default();
        a.push(volume(0.5), 0);
        a.hold(1_500);
        assert_eq!(a.deadline(), Some(3_000));
        a.push(reminder(1), 2_000);
        a.push(reminder(2), 2_000);
        a.retain(2_100, |al| {
            al.actions.first().map(|x| x.1) != Some(AlertAction::CompleteTask(1))
        });
        assert_eq!(
            a.current().unwrap().actions[0].1,
            AlertAction::CompleteTask(2)
        );
    }
}
