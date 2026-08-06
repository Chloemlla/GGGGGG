use std::{
    collections::HashMap,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub struct RateLimitStatus {
    pub allowed: bool,
    pub retry_after_secs: u64,
}

pub struct FixedWindow {
    limit: Arc<AtomicU64>,
    window: Duration,
    buckets: Mutex<HashMap<String, (Instant, u64)>>,
}

impl FixedWindow {
    pub fn new(limit: u64, window_secs: u64) -> Self {
        Self {
            limit: Arc::new(AtomicU64::new(limit)),
            window: Duration::from_secs(window_secs),
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub fn set_limit(&self, limit: u64) {
        self.limit.store(limit.max(1), Ordering::Relaxed);
    }

    pub fn check(&self, key: &str) -> RateLimitStatus {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        buckets.retain(|_, (started, _)| now.duration_since(*started) < self.window);

        let entry = buckets.entry(key.to_string()).or_insert((now, 0));
        if entry.1 >= self.limit.load(Ordering::Relaxed) {
            let elapsed = now.duration_since(entry.0).as_secs();
            let retry_after_secs = self.window.as_secs().saturating_sub(elapsed).max(1);
            return RateLimitStatus {
                allowed: false,
                retry_after_secs,
            };
        }

        entry.1 += 1;
        RateLimitStatus {
            allowed: true,
            retry_after_secs: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FixedWindow;

    #[test]
    fn allows_requests_up_to_limit() {
        let limiter = FixedWindow::new(3, 60);
        assert!(limiter.check("key").allowed);
        assert!(limiter.check("key").allowed);
        assert!(limiter.check("key").allowed);
        let blocked = limiter.check("key");
        assert!(!blocked.allowed);
        assert_eq!(blocked.retry_after_secs, 60);
    }

    #[test]
    fn tracks_separate_keys_independently() {
        let limiter = FixedWindow::new(2, 60);
        assert!(limiter.check("a").allowed);
        assert!(limiter.check("b").allowed);
        assert!(limiter.check("a").allowed);
        assert!(!limiter.check("a").allowed);
        assert!(limiter.check("b").allowed);
    }
}
