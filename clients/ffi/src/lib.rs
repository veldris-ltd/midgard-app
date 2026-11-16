//! # midgard-client-ffi — the UniFFI surface over the client core
//!
//! The crate and its vendored UniFFI 0.28.3 closure; the binding surface follows in
//! the next commit. `[FR-C-1]` `[DD-6]`

// UniFFI's scaffolding emits `unsafe` FFI glue, so `forbid` cannot apply; `deny`
// stops any hand-written `unsafe`.
#![deny(unsafe_code)]

uniffi::setup_scaffolding!();

/// The core version every shell links. `[AC-C-1]`
#[uniffi::export]
pub fn core_version() -> String {
    midgard_client_core::CORE_VERSION.to_string()
}
