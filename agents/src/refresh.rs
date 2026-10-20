//! Fixed-window filesystem debounce and periodic liveness refreshes.

use crate::sessions::RefreshRequest;
use std::time::{Duration, Instant};

pub const FALLBACK_INTERVAL: Duration = Duration::from_secs(5);
pub const POLLING_INTERVAL: Duration = Duration::from_secs(1);
const DEBOUNCE: Duration = Duration::from_millis(50);

pub struct RefreshSchedule {
    periodic: Instant,
    changed: Option<Instant>,
    interval: Duration,
    request: Option<RefreshRequest>,
}

impl RefreshSchedule {
    pub fn new(now: Instant, interval: Duration) -> Self {
        Self {
            periodic: now + interval,
            changed: None,
            interval,
            request: None,
        }
    }

    pub fn changed(&mut self, now: Instant) {
        self.paths_changed(now, RefreshRequest::Full);
    }

    pub fn paths_changed(&mut self, now: Instant, request: RefreshRequest) {
        self.changed.get_or_insert(now + DEBOUNCE);
        if let Some(pending) = &mut self.request {
            pending.merge(request);
        } else {
            self.request = Some(request);
        }
    }

    pub fn take_due(&mut self, now: Instant) -> Option<RefreshRequest> {
        if !self.due(now) {
            return None;
        }
        let request = if now >= self.periodic {
            RefreshRequest::Full
        } else {
            self.request
                .take()
                .expect("filesystem deadline has a refresh request")
        };
        if matches!(request, RefreshRequest::Full) {
            self.refreshed(now);
        } else {
            self.changed = None;
            self.request = None;
        }
        Some(request)
    }

    pub fn timeout(&self, now: Instant) -> Duration {
        self.changed
            .map_or(self.periodic, |changed| changed.min(self.periodic))
            .saturating_duration_since(now)
    }

    pub fn redraw_timeout(&self, now: Instant, background: bool) -> Duration {
        let timeout = self.timeout(now);
        if background {
            timeout.min(Duration::from_secs(1))
        } else {
            timeout
        }
    }

    pub fn due(&self, now: Instant) -> bool {
        self.timeout(now).is_zero()
    }

    pub fn refreshed(&mut self, now: Instant) {
        self.periodic = now + self.interval;
        self.changed = None;
        self.request = None;
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

    #[test]
    fn targeted_refreshes_keep_the_full_liveness_deadline() {
        let start = Instant::now();
        let mut schedule = RefreshSchedule::new(start, FALLBACK_INTERVAL);
        for second in 1..5 {
            let now = start + Duration::from_secs(second);
            let request = RefreshRequest::paths(vec!["session.jsonl".into()]);
            schedule.paths_changed(now, request);
            assert!(matches!(
                schedule.take_due(now + DEBOUNCE),
                Some(RefreshRequest::Paths(_))
            ));
        }
        assert_eq!(
            schedule.take_due(start + FALLBACK_INTERVAL),
            Some(RefreshRequest::Full)
        );
    }
}

#[cfg(test)]
mod redraw_tests {
    use super::*;

    #[test]
    fn background_redraw_does_not_refresh_sessions_or_wake_idle_view() {
        let now = Instant::now();
        let schedule = RefreshSchedule::new(now, FALLBACK_INTERVAL);
        assert_eq!(schedule.redraw_timeout(now, true), Duration::from_secs(1));
        assert!(!schedule.due(now + Duration::from_secs(1)));
        assert_eq!(schedule.redraw_timeout(now, false), FALLBACK_INTERVAL);
        assert_eq!(
            schedule.redraw_timeout(now + Duration::from_millis(4500), true),
            Duration::from_millis(500)
        );
    }
}
