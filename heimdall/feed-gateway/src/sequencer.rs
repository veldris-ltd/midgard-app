//! The feed SDK's sequencer: hands a feed source the monotonic sequence numbers it
//! puts in each observation before signing it (`Observation::with_sequence`). A
//! gateway admits each feed's sequences only in increasing order, so a source that
//! restarts must carry on above the last number it sent: resume from a saved value
//! with [`FeedSequencer::resume_after`], or take numbers from the clock with
//! [`FeedSequencer::from_clock`]. [VLD-ANB-MIDGARD-0041]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A monotonic counter of observation sequence numbers, starting at 1.
#[derive(Debug)]
pub struct FeedSequencer {
    next: AtomicU64,
}

impl Default for FeedSequencer {
    fn default() -> Self {
        Self::new()
    }
}

impl FeedSequencer {
    /// A sequencer whose first number is 1.
    pub fn new() -> Self {
        Self::resume_after(0)
    }

    /// A sequencer whose first number is `last + 1`: a source that saved the last
    /// number it sent carries on from there.
    pub fn resume_after(last: u64) -> Self {
        Self {
            next: AtomicU64::new(last.saturating_add(1).max(1)),
        }
    }

    /// A sequencer whose first number is the current Unix time in nanoseconds, so a
    /// source that keeps no state (a process per observation, or one restarted)
    /// still sends numbers above any it sent before, as long as its clock does not
    /// step back.
    pub fn from_clock() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
        Self::resume_after(nanos.saturating_sub(1))
    }

    /// The next sequence number.
    pub fn next(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst)
    }
}
