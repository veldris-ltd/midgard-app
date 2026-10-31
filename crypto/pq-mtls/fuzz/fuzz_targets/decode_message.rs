//! Fuzz the handshake decoder: whatever bytes a peer sends before it is authenticated,
//! `decode_message` returns, refusing what it cannot parse, and never panics, aborts or
//! allocates without bound. [NFR-SEC-1] [AC-35.5]
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = pq_mtls::decode_message(data);
});
