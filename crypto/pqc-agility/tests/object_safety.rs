//! Compile-time proof that the core traits are object-safe (`dyn`-usable).

use pqc_agility::{Kem, SignatureScheme};

fn _assert_object_safe(_k: &dyn Kem, _s: &dyn SignatureScheme) {}

#[test]
fn traits_are_object_safe() {
    // If this file compiles, the traits are object-safe.
}
