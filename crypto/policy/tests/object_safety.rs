//! The PDP trait must be object-safe (used as `&dyn PolicyDecisionPoint`).

use policy::PolicyDecisionPoint;

fn _assert_object_safe(_p: &dyn PolicyDecisionPoint) {}

#[test]
fn pdp_is_object_safe() {
    // Compiles iff object-safe.
}
