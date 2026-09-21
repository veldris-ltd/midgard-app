//! The managed subsystems and their per-subsystem service roles.

use policy::Role;

/// The subsystems the admin panel administers (SAD_07 §9). Each gets its own
/// least-privilege identity.
pub const SUBSYSTEMS: &[&str] = &[
    "iam",
    "governance",
    "kms",
    "fabric",
    "mimir",
    "bifrost",
    "observability",
];

/// The narrow service role that authorises administering exactly one subsystem.
/// It is deliberately per-subsystem (`svc:admin->kms`), so a role that admits one
/// subsystem admits no other. `[AC-AD-9]`
pub fn service_role(subsystem: &str) -> Role {
    Role(format!("svc:admin->{subsystem}"))
}
