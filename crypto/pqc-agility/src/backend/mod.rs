//! Backend selection. The concrete algorithm implementations are feature-gated;
//! the traits, negotiation, registry, and combiner always compile with zero
//! features (that trait/hash-only mode is intentional — e.g. `mg-repro` uses only
//! `sha256`, so a "backend always required" guard would wrongly break it).
//!
//! Security guards (RT-08): the deterministic `test-backend` uses a seeded,
//! FORGEABLE RNG and exists only for `cargo test`. Two compile-time guards keep it
//! out of anything shippable, backed by the `just check-backends` CI gate that no
//! non-test crate enables it in a default build. [NFR-PQ-3]

// 1. The insecure test backend must never coexist with a real backend: with both
//    on, `test-backend` could shadow FIPS in a build that looks production-ready.
#[cfg(all(feature = "test-backend", feature = "backend-fips"))]
compile_error!("`test-backend` is INSECURE and must never be built with `backend-fips`");

// 2. The insecure test backend must never be compiled into a RELEASE / production
//    artefact. `debug_assertions` is off exactly in release profiles, so this fires
//    on `cargo build --release … --features test-backend` (a shippable binary
//    carrying forgeable crypto) while staying SILENT for `cargo test` (debug),
//    including downstream tests that enable `test-backend` on this crate as a
//    dependency — which a naive `not(test)` guard would wrongly reject. [RT-08]
#[cfg(all(feature = "test-backend", not(debug_assertions)))]
compile_error!(
    "`test-backend` is INSECURE (seeded, forgeable RNG) and must NEVER be in a \
     release/production build — it is for `cargo test` only. Select `backend-fips`."
);

#[cfg(feature = "test-backend")]
pub mod test_backend;

#[cfg(feature = "backend-fips")]
pub mod fips;
