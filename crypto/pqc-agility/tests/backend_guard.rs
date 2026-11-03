//! The release guard on the insecure test backend is keyed on the build script's
//! `midgard_release` cfg, not on `debug_assertions`. [RT-08] [NFR-PQ-3]
//! [AC-54.1] [VLD-ANB-MIDGARD-0067]
//!
//! Whether a release build sets the cfg is not observable from a test, which always
//! builds in the test profile, so this pins the source contract instead. The guard
//! itself is exercised by `just check-backends`: a release build with
//! `test-backend` must fail with INSECURE in its output, and one with
//! `backend-fips` must succeed. Runs under either backend feature.

const BACKEND_MOD: &str = include_str!("../src/backend/mod.rs");
const BUILD_RS: &str = include_str!("../build.rs");

#[test]
fn release_guard_is_keyed_on_the_build_script_cfg() {
    assert!(
        BACKEND_MOD.contains(r#"#[cfg(all(feature = "test-backend", midgard_release))]"#),
        "the test-backend release guard must be keyed on midgard_release"
    );
    assert!(
        !BACKEND_MOD.contains("not(debug_assertions)"),
        "the guard must not depend on debug_assertions, which a profile can turn back on"
    );
}

#[test]
fn the_guard_still_refuses_test_backend_with_fips() {
    assert!(
        BACKEND_MOD.contains(r#"#[cfg(all(feature = "test-backend", feature = "backend-fips"))]"#),
        "guard 1 (never both backends) must stay"
    );
}

#[test]
fn build_script_registers_and_emits_the_release_cfg() {
    assert!(
        BUILD_RS.contains("rustc-check-cfg=cfg(midgard_release)"),
        "build.rs must register the cfg so unexpected_cfgs stays clean"
    );
    assert!(
        BUILD_RS.contains("rustc-cfg=midgard_release"),
        "build.rs must emit the cfg"
    );
    assert!(
        BUILD_RS.contains(r#"var("PROFILE")"#) && BUILD_RS.contains(r#"Ok("release")"#),
        "build.rs must key the cfg on Cargo's PROFILE being release"
    );
}
