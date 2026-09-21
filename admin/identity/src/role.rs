//! Admin roles and the standard catalog.
//!
//! A role is a *named subset* of the capability universe, a read-only flag, and a
//! risk ceiling (the highest action risk 1..=5 a holder may **request**). The
//! standard catalog splits the suite's administrative power across five
//! deliberately-disjoint roles so that no single role — and therefore no single
//! compromised admin — can do everything. `[FR-AD-2]` `[AC-AD-2]`

use std::collections::BTreeSet;

use crate::capability::Capability;

/// Named admin roles (SAD_07 §3.1).
pub const ROLE_IAM_ADMIN: &str = "IAM-Admin";
/// Security officer: KMS, markings/policy, break-glass approval, panel privilege.
pub const ROLE_SECURITY_OFFICER: &str = "Security-Officer";
/// Model steward: Mimir model registry, agent scopes, Evals gates.
pub const ROLE_MODEL_STEWARD: &str = "Model-Steward";
/// Release manager: Bifrost channels, rollouts, fleet.
pub const ROLE_RELEASE_MANAGER: &str = "Release-Manager";
/// Auditor: strictly read-only across every subsystem.
pub const ROLE_AUDITOR: &str = "Auditor";

/// A named admin role: a capability subset, a read-only flag, and a risk ceiling.
/// No role in the [`RoleCatalog`] is granted the whole capability universe.
/// `[FR-AD-2]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminRole {
    /// Role name (stable identifier; matches the `ROLE_*` constants).
    pub name: String,
    /// The capabilities this role grants.
    pub capabilities: BTreeSet<Capability>,
    /// Whether the role can perform no mutation (Auditor). `[AC-AD-2]`
    pub read_only: bool,
    /// The highest action risk (1..=5) a holder may request.
    pub risk_ceiling: u8,
}

impl AdminRole {
    /// Whether the role grants `cap`.
    pub fn grants(&self, cap: &Capability) -> bool {
        self.capabilities.contains(cap)
    }

    /// Whether the role holds any mutating capability. A read-only role must
    /// return `false`. `[AC-AD-2]`
    pub fn has_mutating_capability(&self) -> bool {
        self.capabilities.iter().any(Capability::is_mutating)
    }
}

fn role(name: &str, read_only: bool, risk_ceiling: u8, caps: &[&str]) -> AdminRole {
    AdminRole {
        name: name.to_string(),
        capabilities: caps.iter().map(|c| Capability::new(*c)).collect(),
        read_only,
        risk_ceiling,
    }
}

/// The set of admin roles in force. The standard catalog is fixed and
/// role-separated; deployments extend it, never collapse it into a super-role.
#[derive(Debug, Clone)]
pub struct RoleCatalog {
    roles: Vec<AdminRole>,
}

impl RoleCatalog {
    /// The standard, role-separated catalog (SAD_07 §1.1, §3.1). Each role owns a
    /// distinct slice of administrative power; the read caps overlap but the write
    /// caps are disjoint, so no role equals the union of all capabilities.
    pub fn standard() -> Self {
        let roles = vec![
            role(
                ROLE_IAM_ADMIN,
                false,
                4,
                &[
                    "iam.user.write",
                    "iam.group.write",
                    "iam.role.write",
                    "iam.binding.write",
                    "iam.mfa.write",
                    "iam.read",
                    "config.read",
                    "config.write",
                    "breakglass.request",
                ],
            ),
            role(
                ROLE_SECURITY_OFFICER,
                false,
                5,
                &[
                    "kms.key.rotate",
                    "kms.key.wrap",
                    "kms.read",
                    "gov.marking.write",
                    "gov.policy.write",
                    "gov.read",
                    "panel.privilege.write",
                    "breakglass.approve",
                    "config.read",
                ],
            ),
            role(
                ROLE_MODEL_STEWARD,
                false,
                4,
                &[
                    "mimir.model.approve",
                    "mimir.model.deprecate",
                    "mimir.model.revoke",
                    "mimir.agent.scope",
                    "mimir.evals.gate",
                    "mimir.read",
                    "config.read",
                    "breakglass.request",
                ],
            ),
            role(
                ROLE_RELEASE_MANAGER,
                false,
                5,
                &[
                    "bifrost.channel.write",
                    "bifrost.release.rollout",
                    "bifrost.release.fleet",
                    "bifrost.fleet.write",
                    "bifrost.read",
                    "config.read",
                    "breakglass.request",
                ],
            ),
            role(
                ROLE_AUDITOR,
                true,
                1,
                &[
                    "audit.read",
                    "config.read",
                    "iam.read",
                    "gov.read",
                    "mimir.read",
                    "bifrost.read",
                    "kms.read",
                ],
            ),
        ];
        Self { roles }
    }

    /// All roles in the catalog.
    pub fn roles(&self) -> &[AdminRole] {
        &self.roles
    }

    /// Look up a role by name.
    pub fn get(&self, name: &str) -> Option<&AdminRole> {
        self.roles.iter().find(|r| r.name == name)
    }

    /// The union of every capability any role can hold.
    pub fn capability_universe(&self) -> BTreeSet<Capability> {
        let mut u = BTreeSet::new();
        for r in &self.roles {
            for c in &r.capabilities {
                u.insert(c.clone());
            }
        }
        u
    }

    /// Whether **any** role grants every capability in the universe. A catalog
    /// that upholds "no all-powerful role" must return `false`. `[FR-AD-2]`
    /// `[AC-AD-2]`
    pub fn any_role_all_powerful(&self) -> bool {
        let universe = self.capability_universe();
        self.roles.iter().any(|r| r.capabilities == universe)
    }
}
