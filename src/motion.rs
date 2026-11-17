//! Interruptible, time-based reveal; no sleeps or document mutations.
use std::time::{Duration, Instant};
#[derive(Clone, Debug)]
pub struct Reveal {
    from: f32,
    target: f32,
    started: Instant,
}
impl Reveal {
    pub fn new(visible: bool) -> Self {
        Self {
            from: visible as u8 as f32,
            target: visible as u8 as f32,
            started: Instant::now(),
        }
    }
    pub fn value_at(&self, now: Instant) -> f32 {
        let t = (now.saturating_duration_since(self.started).as_secs_f32() / 0.16).clamp(0., 1.);
        self.from + (self.target - self.from) * (1. - (1. - t).powi(3))
    }
    pub fn value(&self) -> f32 {
        self.value_at(Instant::now())
    }
    pub fn target(&self) -> f32 {
        self.target
    }
    pub fn active(&self) -> bool {
        (self.from - self.target).abs() > 0.0001
            && self.started.elapsed() < Duration::from_millis(160)
    }
    pub fn set_at(&mut self, visible: bool, now: Instant) {
        let target = visible as u8 as f32;
        if target != self.target {
            self.from = self.value_at(now);
            self.target = target;
            self.started = now;
        }
    }
    pub fn set(&mut self, visible: bool) {
        self.set_at(visible, Instant::now());
    }
    pub fn snap(&mut self, visible: bool) {
        self.from = visible as u8 as f32;
        self.target = self.from;
        self.started = Instant::now();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reveal_finishes_and_reversal_is_continuous() {
        let now = Instant::now();
        let mut r = Reveal::new(false);
        r.set_at(true, now);
        let middle = now + Duration::from_millis(60);
        let v = r.value_at(middle);
        assert!(v > 0. && v < 1.);
        r.set_at(false, middle);
        assert!((r.value_at(middle) - v).abs() < 1e-6);
        assert_eq!(r.value_at(middle + Duration::from_millis(160)), 0.);
    }
    #[test]
    fn reduced_motion_snap_is_final() {
        let mut r = Reveal::new(false);
        r.set(true);
        r.snap(true);
        assert_eq!(r.value(), 1.);
        assert!(!r.active());
    }
}
