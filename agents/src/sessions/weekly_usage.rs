use crate::providers::{ProviderId, WeeklyUsage};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
pub(super) struct WeeklyUsageCache {
    requests: HashMap<ProviderId, (Instant, bool)>,
    pub values: HashMap<ProviderId, WeeklyUsage>,
}

impl WeeklyUsageCache {
    pub fn start(&mut self, provider: ProviderId, now: Instant) -> bool {
        if self
            .requests
            .get(&provider)
            .is_some_and(|(last, pending)| *pending || now.duration_since(*last) < INTERVAL)
        {
            return false;
        }
        self.requests.insert(provider, (now, true));
        true
    }

    pub fn complete(&mut self, provider: ProviderId, result: std::io::Result<Option<WeeklyUsage>>) {
        if let Some((_, pending)) = self.requests.get_mut(&provider) {
            *pending = false;
        }
        if let Ok(Some(usage)) = result {
            self.values.insert(provider, usage);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekly_usage_throttles_each_provider_and_preserves_good_values() {
        let mut cache = WeeklyUsageCache::default();
        let now = Instant::now();
        let provider = ProviderId::Claude;
        let usage = WeeklyUsage::from_used_percent(18.4, None).unwrap();
        assert!(cache.start(provider, now));
        assert!(cache.start(ProviderId::Codex, now));
        assert!(!cache.start(provider, now + INTERVAL * 2));
        cache.complete(provider, Ok(Some(usage.clone())));
        assert!(!cache.start(provider, now + INTERVAL - Duration::from_nanos(1)));
        assert!(cache.start(provider, now + INTERVAL));
        cache.complete(provider, Err(std::io::Error::other("request failed")));
        assert_eq!(cache.values.get(&provider), Some(&usage));
        assert!(!cache.start(provider, now + INTERVAL + Duration::from_secs(1)));
        assert!(cache.start(provider, now + INTERVAL * 2));
        cache.complete(provider, Ok(None));
        assert_eq!(cache.values.get(&provider), Some(&usage));
        cache.complete(
            ProviderId::Codex,
            Err(std::io::Error::other("daemon absent")),
        );
        assert!(!cache.values.contains_key(&ProviderId::Codex));
    }
}
