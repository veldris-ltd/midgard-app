//! One policy for a lock poisoned by a panic: recover it, and say so.
//!
//! A panic while a lock is held poisons it. Panicking on every later lock turns one
//! bug into an outage of the whole service; recovering in silence lets later requests
//! run on state a panic may have left half-updated, with no trace. [`recover`] does
//! neither: it takes the lock back, counts the recovery in
//! `lock_poison_recoveries_total{lock}`, logs it at error level, and clears the
//! poison, so each poisoning is counted and logged once. A caller guarding security
//! state also records [`RECOVERED_POISONED_LOCK`] in its audit ledger.
//! [VLD-ANB-MIDGARD-0030]

use std::sync::{Mutex, MutexGuard};

use telemetry::{Logger, MetricsRegistry};

/// The counter of recovered poisoned locks, labelled by `lock`.
pub const LOCK_POISON_RECOVERIES: &str = "lock_poison_recoveries_total";

/// The audit action a recovered security-state lock is recorded under, on the
/// resource [`lock_resource`] names.
pub const RECOVERED_POISONED_LOCK: &str = "RECOVERED poisoned lock";

/// The audit resource for the lock named `lock`: `lock:<name>`.
pub fn lock_resource(lock: &str) -> String {
    format!("lock:{lock}")
}

/// Lock `mutex`, recovering it if a panic poisoned it: the recovery is counted under
/// `lock`, logged, and the poison cleared. [VLD-ANB-MIDGARD-0030]
pub fn recover<'a, T>(
    mutex: &'a Mutex<T>,
    lock: &str,
    metrics: &MetricsRegistry,
    logger: &Logger,
) -> MutexGuard<'a, T> {
    recover_noting(mutex, lock, metrics, logger).0
}

/// [`recover`], also saying whether the lock was poisoned, so that a caller guarding
/// security state can record the recovery in its ledger. [VLD-ANB-MIDGARD-0030]
pub fn recover_noting<'a, T>(
    mutex: &'a Mutex<T>,
    lock: &str,
    metrics: &MetricsRegistry,
    logger: &Logger,
) -> (MutexGuard<'a, T>, bool) {
    match mutex.lock() {
        Ok(guard) => (guard, false),
        Err(poisoned) => {
            metrics.inc(LOCK_POISON_RECOVERIES, &[("lock", lock)]);
            metrics.describe(
                LOCK_POISON_RECOVERIES,
                "Locks recovered after a panic poisoned them, by lock.",
            );
            logger.error("poisoned lock recovered", &[("lock", lock)]);
            let guard = poisoned.into_inner();
            mutex.clear_poison();
            (guard, true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use telemetry::BufferSink;

    fn poison(mutex: &Mutex<u32>) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = mutex.lock();
            panic!("simulated panic while holding the lock");
        }));
    }

    // A poisoned lock is taken back, counted, logged at error level, and cleared, so
    // the next lock is clean and the recovery is counted once. [VLD-ANB-MIDGARD-0030]
    #[test]
    fn a_poisoned_lock_is_recovered_counted_and_logged_once() {
        let mutex = Mutex::new(7u32);
        let metrics = MetricsRegistry::new();
        let sink = BufferSink::new();
        let logger = Logger::new("test", &sink);

        let (guard, poisoned) = recover_noting(&mutex, "state", &metrics, &logger);
        assert_eq!((*guard, poisoned), (7, false));
        drop(guard);

        poison(&mutex);
        assert!(mutex.is_poisoned());
        let (guard, poisoned) = recover_noting(&mutex, "state", &metrics, &logger);
        assert_eq!((*guard, poisoned), (7, true));
        drop(guard);
        assert!(!mutex.is_poisoned(), "the poison is cleared");
        assert_eq!(*recover(&mutex, "state", &metrics, &logger), 7);

        assert_eq!(
            metrics.counter_value(LOCK_POISON_RECOVERIES, &[("lock", "state")]),
            1
        );
        let records = sink.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].level, telemetry::Level::Error);
        assert_eq!(records[0].message, "poisoned lock recovered");
    }
}
