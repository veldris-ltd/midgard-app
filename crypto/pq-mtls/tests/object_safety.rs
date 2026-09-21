//! The transport trait must be object-safe.

use pq_mtls::Channel;

fn _assert_object_safe(_c: &dyn Channel) {}

#[test]
fn channel_is_object_safe() {
    // Compiles iff object-safe.
}
