//! Administrators, scopes, and scoped + expiring role bindings.
//!
//! A binding grants a named role to an admin only within a [`ResourceScope`] and
//! only for a half-open validity window `[not_before, not_after)`. The clock is
//! injected (Unix seconds) so activation and expiry are deterministic. `[FR-AD-2]`

/// Identifier for an administrator.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AdminId(pub String);

/// A managed subsystem the panel administers (e.g. `iam`, `kms`, `bifrost`,
/// `panel`). Bindings and scopes are per-subsystem so authority never leaks
/// across subsystem boundaries. `[NFR-SEC-1]`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Subsystem(pub String);

/// A resource scope: a subsystem plus an opaque selector. The selector `"*"`
/// matches every resource in the subsystem; any other value must match exactly.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceScope {
    /// The subsystem this scope is within.
    pub subsystem: Subsystem,
    /// A selector within the subsystem (`"*"` = all).
    pub selector: String,
}

impl ResourceScope {
    /// A scope covering every resource in a subsystem.
    pub fn all(subsystem: impl Into<String>) -> Self {
        Self {
            subsystem: Subsystem(subsystem.into()),
            selector: "*".to_string(),
        }
    }
}

/// A scoped, expiring grant of a role to an admin. `[FR-AD-2]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleBinding {
    /// The admin the role is granted to.
    pub admin: AdminId,
    /// The granted role name (matches a `ROLE_*` constant / catalog entry).
    pub role: String,
    /// The scope the grant applies to.
    pub scope: ResourceScope,
    /// Start of validity (Unix seconds, inclusive).
    pub not_before: u64,
    /// End of validity (Unix seconds, exclusive) — the binding expires here.
    pub not_after: u64,
}

impl RoleBinding {
    /// Whether the binding is within its validity window at `now`.
    pub fn is_active(&self, now: u64) -> bool {
        now >= self.not_before && now < self.not_after
    }

    /// Whether the binding's scope covers `scope` (same subsystem, and either a
    /// wildcard selector or an exact selector match).
    pub fn covers(&self, scope: &ResourceScope) -> bool {
        self.scope.subsystem == scope.subsystem
            && (self.scope.selector == "*" || self.scope.selector == scope.selector)
    }
}
