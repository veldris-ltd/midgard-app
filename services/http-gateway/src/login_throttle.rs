//! Failed-login throttling: after a few wrong credentials a username is locked for a
//! while, and each further failure doubles the wait. [RC-37] [NFR-SEC-1]
//!
//! The count is kept per username exactly as submitted — known or not, so a lockout is
//! no oracle for which names exist. While a name is locked its logins are refused
//! before the password is checked, so even the right password waits. A success, or a
//! quiet spell of [`FAILURE_WINDOW`], clears the count.
//!
//! The edge binds loopback and sees no client address, so this is per name only.
//! Spreading guesses across many names (password spraying) is the front end's to rate
//! limit by source, where the address is known. State is in memory: a restart clears
//! it, as it clears every issued token.

use std::collections::BTreeMap;

/// Failures a name may have before it is locked.
pub const FREE_FAILURES: u32 = 5;
/// The first lockout, in seconds; each later failure doubles it, up to [`MAX_LOCKOUT`].
pub const FIRST_LOCKOUT: u64 = 30;
/// The longest lockout, in seconds.
pub const MAX_LOCKOUT: u64 = 900;
/// Seconds without a failure after which a name's count is forgotten.
pub const FAILURE_WINDOW: u64 = 900;
/// Most names tracked at once. A flood of made-up names evicts the oldest entries
/// rather than growing without bound.
const MAX_TRACKED: usize = 10_000;

#[derive(Debug, Clone, Copy)]
struct Failures {
    count: u32,
    last: u64,
    locked_until: u64,
}

/// Per-username failure counts and lockouts.
#[derive(Debug, Default)]
pub struct LoginThrottle {
    names: BTreeMap<String, Failures>,
}

impl LoginThrottle {
    /// An empty throttle.
    pub fn new() -> Self {
        Self::default()
    }

    /// Seconds until `username` may try again, or `None` if it may try now.
    pub fn retry_after(&self, username: &str, now: u64) -> Option<u64> {
        self.names
            .get(username)
            .filter(|f| now < f.locked_until)
            .map(|f| f.locked_until - now)
    }

    /// Record a failed login for `username`; returns the lockout it now carries, in
    /// seconds, if this failure locked it.
    pub fn record_failure(&mut self, username: &str, now: u64) -> Option<u64> {
        if !self.names.contains_key(username) && self.names.len() >= MAX_TRACKED {
            self.evict(now);
        }
        let f = self.names.entry(username.to_string()).or_insert(Failures {
            count: 0,
            last: now,
            locked_until: 0,
        });
        if now.saturating_sub(f.last) >= FAILURE_WINDOW {
            f.count = 0;
        }
        f.count = f.count.saturating_add(1);
        f.last = now;
        let over = f.count.checked_sub(FREE_FAILURES + 1)?;
        let lockout = FIRST_LOCKOUT
            .checked_shl(over)
            .unwrap_or(MAX_LOCKOUT)
            .min(MAX_LOCKOUT);
        f.locked_until = now.saturating_add(lockout);
        Some(lockout)
    }

    /// Record a successful login: `username` starts again from no failures.
    pub fn record_success(&mut self, username: &str) {
        self.names.remove(username);
    }

    /// Drop every name whose count has lapsed; if none has, the one idle longest.
    fn evict(&mut self, now: u64) {
        let before = self.names.len();
        self.names
            .retain(|_, f| now < f.locked_until || now.saturating_sub(f.last) < FAILURE_WINDOW);
        if self.names.len() == before {
            if let Some(oldest) = self
                .names
                .iter()
                .min_by_key(|(_, f)| f.last)
                .map(|(name, _)| name.clone())
            {
                self.names.remove(&oldest);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 1_800_000_000;

    #[test]
    fn a_name_is_locked_after_its_free_failures_and_the_wait_doubles() {
        let mut t = LoginThrottle::new();
        for i in 0..FREE_FAILURES - 1 {
            assert_eq!(t.record_failure("olivia", T + u64::from(i)), None);
            assert_eq!(t.retry_after("olivia", T + u64::from(i)), None);
        }
        assert_eq!(t.record_failure("olivia", T), None, "the last free failure");
        assert_eq!(t.record_failure("olivia", T), Some(FIRST_LOCKOUT));
        assert_eq!(t.retry_after("olivia", T + 10), Some(FIRST_LOCKOUT - 10));
        assert_eq!(t.retry_after("olivia", T + FIRST_LOCKOUT), None);
        assert_eq!(t.record_failure("olivia", T + 40), Some(2 * FIRST_LOCKOUT));
    }

    #[test]
    fn the_lockout_is_capped() {
        let mut t = LoginThrottle::new();
        let mut last = None;
        for _ in 0..80 {
            last = t.record_failure("olivia", T);
        }
        assert_eq!(last, Some(MAX_LOCKOUT));
    }

    #[test]
    fn a_success_or_a_quiet_window_clears_the_count() {
        let mut t = LoginThrottle::new();
        for _ in 0..FREE_FAILURES {
            t.record_failure("olivia", T);
        }
        t.record_success("olivia");
        assert_eq!(t.record_failure("olivia", T), None);

        let mut t = LoginThrottle::new();
        for _ in 0..FREE_FAILURES {
            t.record_failure("victor", T);
        }
        let later = T + FAILURE_WINDOW;
        assert_eq!(t.record_failure("victor", later), None, "the count lapsed");
    }

    #[test]
    fn names_are_counted_apart() {
        let mut t = LoginThrottle::new();
        for _ in 0..=FREE_FAILURES {
            t.record_failure("olivia", T);
        }
        assert!(t.retry_after("olivia", T).is_some());
        assert_eq!(t.retry_after("victor", T), None);
    }

    #[test]
    fn a_flood_of_names_stays_bounded() {
        let mut t = LoginThrottle::new();
        for i in 0..MAX_TRACKED + 50 {
            t.record_failure(&format!("guess-{i}"), T);
        }
        assert!(t.names.len() <= MAX_TRACKED);
    }
}
