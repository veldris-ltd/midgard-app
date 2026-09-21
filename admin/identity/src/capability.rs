//! Capabilities: the atomic, least-privilege permissions an admin role may hold.
//!
//! A capability names one administrative action class on one subsystem, dotted and
//! coarse-grained (e.g. `kms.key.rotate`, `iam.binding.write`, `audit.read`). Roles
//! are assembled from *subsets* of the universe, never the whole of it — the
//! structural basis for "no all-powerful role". By convention a capability ending
//! in `.read` is non-mutating; everything else mutates state, which is how the
//! Auditor role is proven incapable of any mutation. `[FR-AD-2]` `[AC-AD-2]`

/// An atomic administrative permission (a dotted, least-privilege action class).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Capability(pub String);

impl Capability {
    /// Build a capability from anything string-like.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Whether this capability mutates state. Anything not suffixed `.read` is
    /// treated as mutating — the invariant that lets [`crate::role::AdminRole`]
    /// prove a read-only role can perform no mutation. `[FR-AD-2]` `[AC-AD-2]`
    pub fn is_mutating(&self) -> bool {
        !self.0.ends_with(".read")
    }
}
