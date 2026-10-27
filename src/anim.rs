//! Molle (spring) per animazioni fluide e indipendenti dal frame rate.
//!
//! Una molla insegue il suo `target`: quando lo raggiunge si ferma da sola
//! e smette di chiedere frame, così l'app torna a 0% di CPU.

/// Passo massimo di integrazione: piccolo abbastanza da restare stabile.
const SUBSTEP: f32 = 1.0 / 240.0;
/// Se un frame arriva molto in ritardo l'animazione rallenta invece di saltare.
const MAX_DT: f32 = 1.0 / 20.0;

/// Transizione morbida da 0 (sotto `from`) a 1 (sopra `to`).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn smooth(from: f32, to: f32, t: f32) -> f32 {
    let x = ((t - from) / (to - from)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    value: f32,
    velocity: f32,
    target: f32,
    stiffness: f32,
    damping: f32,
    epsilon: f32,
}

impl Spring {
    /// Molla per dimensioni in DIP: rapida, con un leggero rimbalzo.
    pub fn size(value: f32) -> Self {
        Self::new(value, 420.0, 32.0, 0.05)
    }

    /// Molla per valori tra 0 e 1 (dissolvenze, scorrimenti): nessun rimbalzo.
    pub fn unit(value: f32) -> Self {
        Self::new(value, 260.0, 32.0, 0.001)
    }

    /// Regola la molla come le animazioni di Apple: `response` è la durata di
    /// un'oscillazione in secondi, `damping` la frazione di smorzamento
    /// (1 = si ferma senza rimbalzare, meno di 1 = un piccolo rimbalzo).
    /// Si può cambiare a metà animazione: la molla riparte dalla velocità che ha.
    pub fn tune(&mut self, response: f32, damping: f32) {
        let omega = std::f32::consts::TAU / response.max(0.05);
        self.stiffness = omega * omega;
        self.damping = 2.0 * damping.max(0.0) * omega;
    }

    fn new(value: f32, stiffness: f32, damping: f32, epsilon: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            stiffness,
            damping,
            epsilon,
        }
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Porta la molla a `value` all'istante, senza animazione.
    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }

    /// Salta alla fine dell'animazione in corso.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn settle(&mut self) {
        self.snap(self.target);
    }

    pub fn is_settled(&self) -> bool {
        self.value == self.target && self.velocity == 0.0
    }

    /// Avanza la simulazione di `dt` secondi.
    pub fn step(&mut self, dt: f32) {
        if self.is_settled() {
            return;
        }
        let mut remaining = dt.clamp(0.0, MAX_DT);
        while remaining > 0.0 {
            let h = remaining.min(SUBSTEP);
            let force = -self.stiffness * (self.value - self.target) - self.damping * self.velocity;
            self.velocity += force * h;
            self.value += self.velocity * h;
            remaining -= h;
        }
        if (self.value - self.target).abs() < self.epsilon
            && self.velocity.abs() < self.epsilon * 10.0
        {
            self.snap(self.target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Valore massimo raggiunto andando da 0 a 1.
    fn peak(mut s: Spring) -> f32 {
        s.set_target(1.0);
        let mut peak: f32 = 0.0;
        for _ in 0..240 {
            s.step(1.0 / 120.0);
            peak = peak.max(s.value());
        }
        peak
    }

    #[test]
    fn tuning_controls_the_bounce() {
        let mut bouncy = Spring::unit(0.0);
        bouncy.tune(0.45, 0.7);
        let mut critical = Spring::unit(0.0);
        critical.tune(0.3, 1.0);
        assert!(peak(bouncy) > 1.03, "con smorzamento 0,7 rimbalza");
        assert!(peak(critical) <= 1.0 + 1e-3, "con smorzamento 1 non supera");
    }

    fn run(s: &mut Spring, seconds: f32) -> usize {
        let mut frames = 0;
        while !s.is_settled() && frames < (seconds * 60.0) as usize {
            s.step(1.0 / 60.0);
            frames += 1;
        }
        frames
    }

    #[test]
    fn size_spring_reaches_target_and_stops() {
        let mut s = Spring::size(32.0);
        s.set_target(166.0);
        let frames = run(&mut s, 3.0);
        assert!(s.is_settled(), "non si è fermata dopo {frames} frame");
        assert_eq!(s.value(), 166.0);
        assert!(frames < 90, "troppo lenta: {frames} frame");
    }

    #[test]
    fn unit_spring_does_not_overshoot() {
        let mut s = Spring::unit(0.0);
        s.set_target(1.0);
        let mut max: f32 = 0.0;
        while !s.is_settled() {
            s.step(1.0 / 60.0);
            max = max.max(s.value());
        }
        assert!(max <= 1.0 + 1e-3, "rimbalzo inatteso: {max}");
    }

    #[test]
    fn settled_spring_ignores_steps() {
        let mut s = Spring::unit(0.5);
        s.step(1.0);
        assert!(s.is_settled());
        assert_eq!(s.value(), 0.5);
    }

    #[test]
    fn huge_dt_stays_stable() {
        let mut s = Spring::size(0.0);
        s.set_target(500.0);
        for _ in 0..200 {
            s.step(10.0);
            assert!(s.value().is_finite());
        }
        assert!(s.is_settled());
    }

    #[test]
    fn retargeting_mid_flight_wakes_the_spring() {
        let mut s = Spring::size(0.0);
        s.set_target(100.0);
        s.step(0.05);
        s.set_target(0.0);
        run(&mut s, 3.0);
        assert_eq!(s.value(), 0.0);
    }
}
