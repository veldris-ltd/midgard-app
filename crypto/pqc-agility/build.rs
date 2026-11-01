//! Emits `cfg(midgard_release)` for release builds of this crate. [RT-08]
//! [VLD-ANB-MIDGARD-0067]
//!
//! The guard in `src/backend/mod.rs` that keeps the deterministic, forgeable
//! `test-backend` out of anything shippable is keyed on this cfg rather than on
//! `debug_assertions`, which a profile can turn back on (`debug-assertions = true`
//! in `[profile.release]`, or a custom profile) without making the build any less
//! of a release. Cargo sets `PROFILE` to `release` for the release profile and for
//! every profile that inherits from it, whatever its debug settings.
//!
//! It reads nothing but `PROFILE`, so it adds nothing a reproducible build could
//! differ on. [AC-M-6]

fn main() {
    // Registered unconditionally, so `unexpected_cfgs` stays clean under
    // `-D warnings` in every profile.
    println!("cargo::rustc-check-cfg=cfg(midgard_release)");
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        println!("cargo::rustc-cfg=midgard_release");
    }
    // Cargo re-runs a build script on any source change unless told otherwise;
    // its only input is the profile, which Cargo already builds separately.
    println!("cargo::rerun-if-changed=build.rs");
}
