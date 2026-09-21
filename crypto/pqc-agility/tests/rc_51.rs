//! RC-51: an OS entropy failure is **recoverable by the caller**.
//!
//! `Rng::fill` fail-closes with a panic (correct for keygen paths that must never
//! proceed on weak entropy). RT-51 adds a Result-returning path — `OsRng::try_new`
//! and `OsRng::try_fill` — so a service can detect an entropy failure and degrade
//! gracefully instead of crashing. These tests exercise the recoverable API and the
//! caller pattern. [RT-51]
#![cfg(feature = "backend-fips")]

use pqc_agility::{OsRng, Rng};

#[test]
fn try_new_probes_entropy_and_returns_ok() {
    // The recoverable constructor: on a working CSPRNG it yields a usable OsRng.
    assert!(
        OsRng::try_new().is_ok(),
        "entropy is available on the test host"
    );
}

#[test]
fn try_fill_returns_ok_and_produces_entropy() {
    let mut rng = OsRng::try_new().expect("entropy available");
    let mut buf = [0u8; 32];
    assert!(
        rng.try_fill(&mut buf).is_ok(),
        "fallible fill succeeds without panicking"
    );
    assert!(
        buf.iter().any(|&b| b != 0),
        "the CSPRNG actually produced entropy"
    );
}

#[test]
fn caller_can_recover_instead_of_panicking() {
    // The RC-51 pattern: the caller branches on the Result. Whatever the entropy
    // state, control returns to the caller — the library never forces a panic here.
    let outcome: Result<&str, _> = match OsRng::try_new() {
        Ok(mut rng) => {
            let mut b = [0u8; 8];
            rng.try_fill(&mut b).map(|_| "generated")
        }
        // On a real entropy failure the service would degrade here rather than crash.
        Err(_e) => Ok("degraded-gracefully"),
    };
    assert!(
        outcome.is_ok(),
        "an entropy failure is recoverable, not a crash"
    );
}

#[test]
fn infallible_fill_still_works_for_fail_closed_callers() {
    // The panicking Rng::fill remains for keygen paths; it draws through the same
    // source and succeeds on a healthy host.
    let mut rng = OsRng;
    let mut buf = [0u8; 16];
    rng.fill(&mut buf);
    assert!(buf.iter().any(|&b| b != 0));
}
