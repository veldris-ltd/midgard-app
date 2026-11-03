//! Replay protection for signed observations: a signature proves who sent an
//! observation, not that this is the first time it was sent. The [`ReplayGuard`]
//! holds, per feed, the last sequence number admitted and a bounded window of recent
//! observation ids, and a [`FreshnessPolicy`] bounds how far `observed_at` may stray
//! from the gateway's clock. The gateway consults it only after a signature verifies,
//! so unauthenticated traffic cannot consume sequence or window state.
//! [VLD-ANB-MIDGARD-0041] [VLD-ANB-MIDGARD-0015]

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::{Mutex, MutexGuard};

use heimdall_picture::{FeedId, ObservationId};

use crate::error::FeedError;

/// How many recent observation ids are remembered per feed.
pub const MAX_SEEN_PER_FEED: usize = 4096;

/// How far an observation's `observed_at` may stray from the gateway's clock, in
/// seconds. The defaults are the feed service's (`heimdall-feed`
/// `--max-observation-skew` and `--max-observation-age`): five minutes ahead, seven
/// days behind, so delayed and backfilled feeds still land. [VLD-ANB-MIDGARD-0015]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreshnessPolicy {
    /// How far behind the clock `observed_at` may be.
    pub max_age_secs: u64,
    /// How far ahead of the clock `observed_at` may be (feed clock drift).
    pub max_future_skew_secs: u64,
}

impl Default for FreshnessPolicy {
    fn default() -> Self {
        Self {
            max_age_secs: 604_800,
            max_future_skew_secs: 300,
        }
    }
}

impl FreshnessPolicy {
    /// Whether an observation dated `observed_at` is fresh at `now`: no more than
    /// `max_future_skew_secs` ahead and no more than `max_age_secs` behind, both
    /// bounds inclusive.
    pub fn admits(&self, observed_at: u64, now: u64) -> bool {
        observed_at <= now.saturating_add(self.max_future_skew_secs)
            && observed_at.saturating_add(self.max_age_secs) >= now
    }
}

/// One feed's replay state.
#[derive(Default)]
struct FeedState {
    last_sequence: u64,
    order: VecDeque<ObservationId>,
    seen: HashSet<ObservationId>,
}

impl FeedState {
    fn record(&mut self, id: &ObservationId, sequence: u64) {
        self.last_sequence = sequence;
        if self.seen.insert(id.clone()) {
            self.order.push_back(id.clone());
            if self.order.len() > MAX_SEEN_PER_FEED {
                if let Some(oldest) = self.order.pop_front() {
                    self.seen.remove(&oldest);
                }
            }
        }
    }
}

#[derive(Default)]
struct Inner {
    feeds: BTreeMap<FeedId, FeedState>,
    admitted: u64,
    refused: u64,
}

/// Per-feed replay state, shared by every connection or request a gateway serves
/// (hold it in an `Arc` and hand it to each [`crate::FeedGateway`]). In memory: a
/// service that must keep it across a restart saves [`ReplayGuard::last_sequences`]
/// and puts it back with [`ReplayGuard::restore`]. [VLD-ANB-MIDGARD-0041]
#[derive(Default)]
pub struct ReplayGuard {
    inner: Mutex<Inner>,
}

impl ReplayGuard {
    /// An empty guard: no feed has a sequence yet.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Check an authenticated observation and, if it passes, record it. The rules, in
    /// order: a sequence of 0 is unset; a sequence not above the feed's last admitted
    /// one is not monotonic; an `observed_at` outside `policy` around `now` is stale;
    /// an id already in the feed's window is replayed. Only an observation that passes
    /// every rule changes the state. [VLD-ANB-MIDGARD-0041]
    pub fn check_and_record(
        &self,
        feed: &FeedId,
        id: &ObservationId,
        sequence: u64,
        observed_at: u64,
        now: u64,
        policy: &FreshnessPolicy,
    ) -> Result<(), FeedError> {
        let mut inner = self.lock();
        let verdict = Self::check(&inner, feed, id, sequence, observed_at, now, policy);
        match verdict {
            Ok(()) => {
                inner
                    .feeds
                    .entry(feed.clone())
                    .or_default()
                    .record(id, sequence);
                inner.admitted += 1;
            }
            Err(_) => inner.refused += 1,
        }
        verdict
    }

    fn check(
        inner: &Inner,
        feed: &FeedId,
        id: &ObservationId,
        sequence: u64,
        observed_at: u64,
        now: u64,
        policy: &FreshnessPolicy,
    ) -> Result<(), FeedError> {
        if sequence == 0 {
            return Err(FeedError::SequenceUnset(feed.clone()));
        }
        let state = inner.feeds.get(feed);
        let last = state.map_or(0, |s| s.last_sequence);
        if sequence <= last {
            return Err(FeedError::SequenceNotMonotonic {
                feed: feed.clone(),
                got: sequence,
                last,
            });
        }
        if !policy.admits(observed_at, now) {
            return Err(FeedError::Stale {
                feed: feed.clone(),
                observed_at,
                now,
            });
        }
        if state.is_some_and(|s| s.seen.contains(id)) {
            return Err(FeedError::Replayed {
                feed: feed.clone(),
                id: id.clone(),
            });
        }
        Ok(())
    }

    /// The last sequence admitted from each feed, to save across a restart.
    pub fn last_sequences(&self) -> BTreeMap<FeedId, u64> {
        self.lock()
            .feeds
            .iter()
            .map(|(feed, state)| (feed.clone(), state.last_sequence))
            .collect()
    }

    /// Put back a feed's last admitted sequence (from [`ReplayGuard::last_sequences`]
    /// saved before a restart). It only ever raises the feed's sequence. The id window
    /// starts empty; the sequence alone refuses anything admitted before.
    pub fn restore(&self, feed: FeedId, last_sequence: u64) {
        let mut inner = self.lock();
        let state = inner.feeds.entry(feed).or_default();
        state.last_sequence = state.last_sequence.max(last_sequence);
    }

    /// The last sequence admitted from `feed`, or 0 if none has been.
    pub fn last_sequence(&self, feed: &FeedId) -> u64 {
        self.lock().feeds.get(feed).map_or(0, |s| s.last_sequence)
    }

    /// How many observation ids are remembered for `feed` (at most
    /// [`MAX_SEEN_PER_FEED`]).
    pub fn seen_len(&self, feed: &FeedId) -> usize {
        self.lock().feeds.get(feed).map_or(0, |s| s.order.len())
    }

    /// How many observations the guard has admitted and refused.
    pub fn counters(&self) -> (u64, u64) {
        let inner = self.lock();
        (inner.admitted, inner.refused)
    }
}
