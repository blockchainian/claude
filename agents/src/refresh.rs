//! Fixed-window filesystem debounce and periodic liveness refreshes.

use std::time::{Duration, Instant};

pub const FALLBACK_INTERVAL: Duration = Duration::from_secs(5);
pub const POLLING_INTERVAL: Duration = Duration::from_secs(1);
const DEBOUNCE: Duration = Duration::from_millis(50);

pub struct RefreshSchedule {
    periodic: Instant,
    changed: Option<Instant>,
    interval: Duration,
}

impl RefreshSchedule {
    pub fn new(now: Instant, interval: Duration) -> Self {
        Self {
            periodic: now + interval,
            changed: None,
            interval,
        }
    }

    pub fn changed(&mut self, now: Instant) {
        self.changed.get_or_insert(now + DEBOUNCE);
    }

    pub fn timeout(&self, now: Instant) -> Duration {
        self.changed
            .map_or(self.periodic, |changed| changed.min(self.periodic))
            .saturating_duration_since(now)
    }

    pub fn due(&self, now: Instant) -> bool {
        self.timeout(now).is_zero()
    }

    pub fn refreshed(&mut self, now: Instant) {
        self.periodic = now + self.interval;
        self.changed = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_refreshes_once_after_fifty_ms_without_extending_the_window() {
        let start = Instant::now();
        let mut schedule = RefreshSchedule::new(start, FALLBACK_INTERVAL);
        for ms in [0, 10, 30, 49] {
            schedule.changed(start + Duration::from_millis(ms));
        }
        assert!(!schedule.due(start + Duration::from_millis(49)));
        assert!(schedule.due(start + DEBOUNCE));
        schedule.refreshed(start + DEBOUNCE);
        assert!(!schedule.due(start + Duration::from_millis(100)));
        schedule.changed(start + Duration::from_millis(100));
        assert!(schedule.due(start + Duration::from_millis(150)));
    }

    #[test]
    fn continuous_events_cannot_delay_liveness_deadline() {
        let start = Instant::now();
        let mut schedule = RefreshSchedule::new(start, FALLBACK_INTERVAL);
        schedule.changed(start + FALLBACK_INTERVAL - Duration::from_millis(10));
        assert!(schedule.due(start + FALLBACK_INTERVAL));
    }

    #[test]
    fn polling_refreshes_every_second_without_events() {
        let start = Instant::now();
        let mut schedule = RefreshSchedule::new(start, POLLING_INTERVAL);
        assert!(!schedule.due(start + Duration::from_millis(999)));
        assert!(schedule.due(start + POLLING_INTERVAL));
        schedule.refreshed(start + POLLING_INTERVAL);
        assert_eq!(schedule.timeout(start + POLLING_INTERVAL), POLLING_INTERVAL);
    }
}
