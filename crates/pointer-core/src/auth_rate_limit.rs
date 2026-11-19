//! Login-entry rate limiting and in-flight capacity (process-local).
//!
//! Everything here is a fixed-window counter keyed by client IP plus a short
//! failure backoff. It is intentionally small: one process, one mutex, no
//! background task. The server asks [`LoginRateLimiter::check_start`] before it
//! does any work for a login, and reports `login/finish` failures back so a
//! captcha-guessing loop cannot run at full speed.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const WINDOW_MS: u64 = 60_000;
/// Drop stale keys once the maps grow past this, so a scan cannot pin memory.
const PRUNE_THRESHOLD: usize = 1024;

/// `[auth.rate_limit]` in `pointer-server.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RateLimitConfig {
    /// Master switch. Rate limiting is on by default.
    pub enabled: bool,
    /// `login/start` attempts allowed per client IP per minute. `0` = unlimited.
    pub per_ip_per_minute: u32,
    /// In-flight logins allowed at once. `0` = unlimited. Over-capacity starts
    /// are rejected — never evicted — so an in-progress login keeps its state.
    pub max_pending: usize,
    /// Cooldown after a failed `login/finish` for that IP. `0` = disabled.
    pub finish_fail_backoff_secs: u64,
    /// Proxy addresses whose `X-Forwarded-For` is trusted (IP or CIDR).
    pub trusted_proxies: Vec<String>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            // Conservative: a human logs in a handful of times per minute.
            per_ip_per_minute: 20,
            max_pending: 256,
            finish_fail_backoff_secs: 3,
            trusted_proxies: Vec::new(),
        }
    }
}

/// What `login/start` should do for this client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartVerdict {
    Allow,
    /// Too many starts from this IP in the current window.
    RateLimited { retry_after_secs: u64 },
    /// This IP is inside a `login/finish` failure cooldown.
    Backoff { retry_after_secs: u64 },
}

#[derive(Default)]
struct Window {
    minute: u64,
    count: u32,
}

#[derive(Default)]
struct LimiterState {
    windows: HashMap<String, Window>,
    backoff_until_ms: HashMap<String, u64>,
}

/// Fixed-window limiter plus failure backoff, shared by every login entry.
pub struct LoginRateLimiter {
    config: RateLimitConfig,
    state: Mutex<LimiterState>,
}

impl LoginRateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            state: Mutex::new(LimiterState::default()),
        }
    }

    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Count one `login/start` attempt for `ip` and decide whether it may run.
    pub fn check_start(&self, ip: &str, now_ms: u64) -> StartVerdict {
        if !self.config.enabled {
            return StartVerdict::Allow;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.windows.len() + state.backoff_until_ms.len() > PRUNE_THRESHOLD {
            prune(&mut state, now_ms);
        }
        if let Some(until) = state.backoff_until_ms.get(ip).copied() {
            if until > now_ms {
                return StartVerdict::Backoff {
                    retry_after_secs: secs_until(until, now_ms),
                };
            }
            state.backoff_until_ms.remove(ip);
        }
        let minute = now_ms / WINDOW_MS;
        let window = state.windows.entry(ip.to_string()).or_default();
        if window.minute != minute {
            window.minute = minute;
            window.count = 0;
        }
        window.count = window.count.saturating_add(1);
        if self.config.per_ip_per_minute > 0 && window.count > self.config.per_ip_per_minute {
            return StartVerdict::RateLimited {
                retry_after_secs: secs_until((minute + 1) * WINDOW_MS, now_ms),
            };
        }
        StartVerdict::Allow
    }

    /// True when this IP must wait out a failure cooldown before trying again.
    pub fn in_backoff(&self, ip: &str, now_ms: u64) -> Option<u64> {
        if !self.config.enabled || self.config.finish_fail_backoff_secs == 0 {
            return None;
        }
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let until = *state.backoff_until_ms.get(ip)?;
        (until > now_ms).then(|| secs_until(until, now_ms))
    }

    /// A `login/finish` attempt failed (bad captcha or bad password).
    pub fn record_finish_failure(&self, ip: &str, now_ms: u64) {
        if !self.config.enabled || self.config.finish_fail_backoff_secs == 0 {
            return;
        }
        let until = now_ms.saturating_add(self.config.finish_fail_backoff_secs * 1000);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.backoff_until_ms.insert(ip.to_string(), until);
    }

    /// A `login/finish` attempt succeeded: clear the cooldown for that IP.
    pub fn record_finish_success(&self, ip: &str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.backoff_until_ms.remove(ip);
    }
}

fn secs_until(until_ms: u64, now_ms: u64) -> u64 {
    let remaining = until_ms.saturating_sub(now_ms);
    remaining.div_ceil(1000).max(1)
}

fn prune(state: &mut LimiterState, now_ms: u64) {
    let minute = now_ms / WINDOW_MS;
    state.windows.retain(|_, window| minute.saturating_sub(window.minute) <= 1);
    state.backoff_until_ms.retain(|_, until| *until > now_ms);
}

/// Milliseconds since the Unix epoch, saturating at 0 before 1970.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter(config: RateLimitConfig) -> LoginRateLimiter {
        LoginRateLimiter::new(config)
    }

    #[test]
    fn allows_up_to_the_limit_then_reports_retry_after() {
        let limiter = limiter(RateLimitConfig {
            per_ip_per_minute: 3,
            ..RateLimitConfig::default()
        });
        let now = 1_000_000;
        for _ in 0..3 {
            assert_eq!(limiter.check_start("203.0.113.7", now), StartVerdict::Allow);
        }
        match limiter.check_start("203.0.113.7", now) {
            StartVerdict::RateLimited { retry_after_secs } => {
                assert!(retry_after_secs >= 1 && retry_after_secs <= 60);
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
        // Another client is unaffected.
        assert_eq!(limiter.check_start("203.0.113.8", now), StartVerdict::Allow);
    }

    #[test]
    fn the_window_resets_on_the_next_minute() {
        let limiter = limiter(RateLimitConfig {
            per_ip_per_minute: 1,
            ..RateLimitConfig::default()
        });
        assert_eq!(limiter.check_start("ip", 0), StartVerdict::Allow);
        assert!(matches!(
            limiter.check_start("ip", 30_000),
            StartVerdict::RateLimited { .. }
        ));
        assert_eq!(limiter.check_start("ip", 60_000), StartVerdict::Allow);
    }

    #[test]
    fn disabled_limiter_never_blocks_and_never_backs_off() {
        let limiter = limiter(RateLimitConfig {
            enabled: false,
            per_ip_per_minute: 1,
            ..RateLimitConfig::default()
        });
        for _ in 0..50 {
            assert_eq!(limiter.check_start("ip", 0), StartVerdict::Allow);
        }
        limiter.record_finish_failure("ip", 0);
        assert_eq!(limiter.in_backoff("ip", 0), None);
    }

    #[test]
    fn zero_per_ip_per_minute_means_unlimited() {
        let limiter = limiter(RateLimitConfig {
            per_ip_per_minute: 0,
            ..RateLimitConfig::default()
        });
        for _ in 0..500 {
            assert_eq!(limiter.check_start("ip", 0), StartVerdict::Allow);
        }
    }

    #[test]
    fn finish_failures_back_off_then_expire() {
        let limiter = limiter(RateLimitConfig {
            finish_fail_backoff_secs: 5,
            ..RateLimitConfig::default()
        });
        limiter.record_finish_failure("198.51.100.4", 10_000);
        assert_eq!(limiter.in_backoff("198.51.100.4", 10_500), Some(5));
        assert!(matches!(
            limiter.check_start("198.51.100.4", 10_500),
            StartVerdict::Backoff { retry_after_secs: 5 }
        ));
        // Expired: allowed again.
        assert_eq!(limiter.in_backoff("198.51.100.4", 15_000), None);
        assert_eq!(limiter.check_start("198.51.100.4", 15_000), StartVerdict::Allow);
    }

    #[test]
    fn a_successful_finish_clears_the_backoff() {
        let limiter = limiter(RateLimitConfig::default());
        limiter.record_finish_failure("ip", 0);
        assert!(limiter.in_backoff("ip", 0).is_some());
        limiter.record_finish_success("ip");
        assert_eq!(limiter.in_backoff("ip", 0), None);
    }

    #[test]
    fn zero_backoff_seconds_disables_the_cooldown() {
        let limiter = limiter(RateLimitConfig {
            finish_fail_backoff_secs: 0,
            ..RateLimitConfig::default()
        });
        limiter.record_finish_failure("ip", 0);
        assert_eq!(limiter.in_backoff("ip", 0), None);
    }

    #[test]
    fn stale_keys_are_pruned() {
        let limiter = limiter(RateLimitConfig::default());
        for i in 0..PRUNE_THRESHOLD + 10 {
            limiter.check_start(&format!("ip-{i}"), 0);
        }
        // A later minute triggers the prune and drops the old window map.
        limiter.check_start("fresh", WINDOW_MS * 5);
        let state = limiter.state.lock().unwrap();
        assert!(
            state.windows.len() < PRUNE_THRESHOLD,
            "windows={}",
            state.windows.len()
        );
    }

    #[test]
    fn config_defaults_are_conservative_and_on() {
        let config = RateLimitConfig::default();
        assert!(config.enabled);
        assert!(config.per_ip_per_minute > 0 && config.per_ip_per_minute <= 60);
        assert!(config.max_pending > 0);
        assert!(config.trusted_proxies.is_empty());
    }
}
