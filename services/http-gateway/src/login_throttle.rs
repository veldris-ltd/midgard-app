//! Failed-login throttling: after a few wrong credentials a username is locked for a
//! while, and each further failure doubles the wait. [RC-37] [NFR-SEC-1]
//!
//! The count is kept per username exactly as submitted — known or not, so a lockout is
//! no oracle for which names exist. While a name is locked its logins are refused
//! before the password is checked, so even the right password waits. A success, or a
//! quiet spell of [`FAILURE_WINDOW`], clears the count.
//!
//! Every login field is bounded before any of this is touched ([`MAX_USERNAME_BYTES`],
//! [`MAX_PASSWORD_BYTES`], [`MAX_TOTP_CHARS`]), so the names kept here are at most
//! [`MAX_TRACKED`] of at most 128 bytes each. Spreading guesses across many names
//! (password spraying) is limited by source instead: the edge gives each client address
//! a token bucket of [`LOGIN_SOURCE_BURST`] attempts refilling at
//! [`LOGIN_SOURCE_PER_SEC`] a second, shared by the login and administration routes,
//! and refuses an attempt past it before any password is checked. Under compose every
//! client reaches the edge from the proxy's one address, so there that bucket is one
//! for the whole site.
//!
//! Throttled attempts are coalesced on the ledger rather than written one per probe: a
//! locked name's first throttled attempt is recorded and later ones are counted onto
//! its next recorded event, and a throttled source is recorded at most once per
//! [`SOURCE_RECORD_INTERVAL`] with the count suppressed since. State is in memory: a
//! restart clears it, as it clears every issued token. [VLD-ANB-MIDGARD-0005]

use std::collections::BTreeMap;

/// Failures a name may have before it is locked.
pub const FREE_FAILURES: u32 = 5;
/// The first lockout, in seconds; each later failure doubles it, up to [`MAX_LOCKOUT`].
pub const FIRST_LOCKOUT: u64 = 30;
/// The longest lockout, in seconds.
pub const MAX_LOCKOUT: u64 = 900;
/// Seconds without a failure after which a name's count is forgotten.
pub const FAILURE_WINDOW: u64 = 900;
/// Most names, and most sources, tracked at once. A flood of made-up names evicts the
/// oldest entries rather than growing without bound.
pub const MAX_TRACKED: usize = 10_000;

/// The longest username a login may carry, in bytes; with [`MAX_TRACKED`] it bounds
/// the throttle's keys at about 1.3 MB. [VLD-ANB-MIDGARD-0005]
pub const MAX_USERNAME_BYTES: usize = 128;
/// The longest password a login may carry, in bytes. [VLD-ANB-MIDGARD-0005]
pub const MAX_PASSWORD_BYTES: usize = 1024;
/// The longest one-time code a login may carry, in characters. [VLD-ANB-MIDGARD-0005]
pub const MAX_TOTP_CHARS: usize = 16;

/// Login attempts one source address may make in a burst. [VLD-ANB-MIDGARD-0005]
pub const LOGIN_SOURCE_BURST: f64 = 20.0;
/// Login attempts a second one source address's allowance refills at.
/// [VLD-ANB-MIDGARD-0005]
pub const LOGIN_SOURCE_PER_SEC: f64 = 0.5;
/// Seconds between ledger rows for one throttled source; the attempts in between are
/// counted onto the next row. [VLD-ANB-MIDGARD-0005]
pub const SOURCE_RECORD_INTERVAL: u64 = 60;

#[derive(Debug, Clone, Copy)]
struct Failures {
    count: u32,
    last: u64,
    locked_until: u64,
    /// The lock (its `locked_until`) whose first throttled attempt is on the ledger.
    recorded_lock: u64,
    /// Throttled attempts kept off the ledger since that one.
    suppressed: u32,
}

#[derive(Debug, Clone, Copy)]
struct SourceRecord {
    /// When this source's last ledger row was written.
    recorded_at: u64,
    /// Throttled attempts kept off the ledger since then.
    suppressed: u64,
}

/// Per-username failure counts and lockouts, and when each throttled source was last
/// recorded.
#[derive(Debug, Default)]
pub struct LoginThrottle {
    names: BTreeMap<String, Failures>,
    sources: BTreeMap<String, SourceRecord>,
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
            recorded_lock: 0,
            suppressed: 0,
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

    /// Note a login refused because `username` is locked: `true` if it is the lock's
    /// first such attempt, to be recorded on the ledger; `false` if one already is, and
    /// this one is counted for [`Self::take_suppressed`] instead.
    pub fn note_throttled(&mut self, username: &str) -> bool {
        let Some(f) = self.names.get_mut(username) else {
            return true;
        };
        if f.recorded_lock == f.locked_until {
            f.suppressed = f.suppressed.saturating_add(1);
            false
        } else {
            f.recorded_lock = f.locked_until;
            true
        }
    }

    /// The throttled attempts on `username` kept off the ledger since its last recorded
    /// one, for its next recorded event to name; the count starts again from zero.
    pub fn take_suppressed(&mut self, username: &str) -> u32 {
        self.names
            .get_mut(username)
            .map_or(0, |f| std::mem::take(&mut f.suppressed))
    }

    /// Note a login refused because `source` is over its allowance: `Some(n)` if it is
    /// to be recorded on the ledger now — the first for this source, or the first
    /// [`SOURCE_RECORD_INTERVAL`] after its last row — with the `n` attempts kept off
    /// the ledger since; `None` if it is counted instead.
    pub fn note_source_throttled(&mut self, source: &str, now: u64) -> Option<u64> {
        let fresh = SourceRecord {
            recorded_at: now,
            suppressed: 0,
        };
        if let Some(r) = self.sources.get_mut(source) {
            if now.saturating_sub(r.recorded_at) < SOURCE_RECORD_INTERVAL {
                r.suppressed = r.suppressed.saturating_add(1);
                return None;
            }
            return Some(std::mem::replace(r, fresh).suppressed);
        }
        if self.sources.len() >= MAX_TRACKED {
            self.evict_source(now);
        }
        self.sources.insert(source.to_string(), fresh);
        Some(0)
    }

    /// How many names are tracked.
    pub fn tracked(&self) -> usize {
        self.names.len()
    }

    /// The bytes the tracked names' keys hold.
    pub fn key_bytes(&self) -> usize {
        self.names.keys().map(String::len).sum()
    }

    /// Drop every source whose last row is over [`SOURCE_RECORD_INTERVAL`] old; if none
    /// is, the one recorded longest ago.
    fn evict_source(&mut self, now: u64) {
        let before = self.sources.len();
        self.sources
            .retain(|_, r| now.saturating_sub(r.recorded_at) < SOURCE_RECORD_INTERVAL);
        if self.sources.len() == before {
            if let Some(oldest) = self
                .sources
                .iter()
                .min_by_key(|(_, r)| r.recorded_at)
                .map(|(source, _)| source.clone())
            {
                self.sources.remove(&oldest);
            }
        }
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

    #[test]
    fn suppressed_attempts_are_counted_per_name() {
        // [VLD-ANB-MIDGARD-0005] [AC-11.4]
        let mut t = LoginThrottle::new();
        for _ in 0..=FREE_FAILURES {
            t.record_failure("olivia", T);
            t.record_failure("victor", T);
        }
        assert!(
            t.note_throttled("olivia"),
            "the lock's first throttled attempt"
        );
        for _ in 0..5 {
            assert!(!t.note_throttled("olivia"), "later ones are counted");
        }
        assert!(t.note_throttled("victor"), "another name's lock is its own");
        assert_eq!(t.take_suppressed("olivia"), 5);
        assert_eq!(t.take_suppressed("olivia"), 0, "the count is taken once");
        assert_eq!(t.take_suppressed("victor"), 0);

        // A new lock has its own first throttled attempt on the ledger.
        let relock = T + FIRST_LOCKOUT;
        t.record_failure("olivia", relock);
        assert!(t.retry_after("olivia", relock).is_some());
        assert!(t.note_throttled("olivia"));
    }

    #[test]
    fn source_records_are_bounded() {
        // [VLD-ANB-MIDGARD-0005] [AC-11.3]
        let mut t = LoginThrottle::new();
        assert_eq!(t.note_source_throttled("10.0.0.9", T), Some(0));
        for _ in 0..4 {
            assert_eq!(t.note_source_throttled("10.0.0.9", T + 1), None);
        }
        assert_eq!(
            t.note_source_throttled("10.0.0.9", T + SOURCE_RECORD_INTERVAL),
            Some(4),
            "once a minute, with the count suppressed since"
        );

        for i in 0..MAX_TRACKED + 50 {
            t.note_source_throttled(&format!("source-{i}"), T + SOURCE_RECORD_INTERVAL);
        }
        assert!(t.sources.len() <= MAX_TRACKED);
    }
}
