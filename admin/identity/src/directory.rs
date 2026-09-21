//! The identity directory: the catalog of roles plus the live set of role
//! bindings, and the queries the privilege service asks of it — "does this admin
//! hold this capability, in this scope, right now?", "what is their risk
//! ceiling?", "do they hold this named role?". All queries respect scope and the
//! binding validity window, so an expired or out-of-scope binding grants nothing.
//! `[FR-AD-2]` `[NFR-SEC-1]`

use crate::binding::{AdminId, ResourceScope, RoleBinding};
use crate::capability::Capability;
use crate::role::{AdminRole, RoleCatalog};

/// The roles-and-bindings directory the panel authorises admins against.
#[derive(Debug, Clone)]
pub struct IdentityDirectory {
    catalog: RoleCatalog,
    bindings: Vec<RoleBinding>,
}

impl IdentityDirectory {
    /// Build a directory over a role catalog, with no bindings yet.
    pub fn new(catalog: RoleCatalog) -> Self {
        Self {
            catalog,
            bindings: Vec::new(),
        }
    }

    /// The role catalog backing this directory.
    pub fn catalog(&self) -> &RoleCatalog {
        &self.catalog
    }

    /// Grant a role to an admin (scoped and expiring).
    pub fn bind(&mut self, binding: RoleBinding) {
        self.bindings.push(binding);
    }

    /// The distinct roles an admin holds that are active and cover `scope` at
    /// `now`.
    pub fn active_roles(
        &self,
        admin: &AdminId,
        scope: &ResourceScope,
        now: u64,
    ) -> Vec<&AdminRole> {
        let mut out: Vec<&AdminRole> = Vec::new();
        for b in &self.bindings {
            if &b.admin == admin && b.is_active(now) && b.covers(scope) {
                if let Some(role) = self.catalog.get(&b.role) {
                    if !out.iter().any(|r| r.name == role.name) {
                        out.push(role);
                    }
                }
            }
        }
        out
    }

    /// Whether the admin holds `capability` in `scope` at `now`, via some active
    /// binding's role.
    pub fn has_capability(
        &self,
        admin: &AdminId,
        capability: &str,
        scope: &ResourceScope,
        now: u64,
    ) -> bool {
        let cap = Capability::new(capability);
        self.active_roles(admin, scope, now)
            .iter()
            .any(|r| r.grants(&cap))
    }

    /// Whether the admin holds the named role, active and covering `scope`, at
    /// `now`.
    pub fn holds_role(
        &self,
        admin: &AdminId,
        role_name: &str,
        scope: &ResourceScope,
        now: u64,
    ) -> bool {
        self.active_roles(admin, scope, now)
            .iter()
            .any(|r| r.name == role_name)
    }

    /// The highest risk ceiling across the admin's active roles in `scope` at
    /// `now` (0 if they hold no active role there).
    pub fn risk_ceiling(&self, admin: &AdminId, scope: &ResourceScope, now: u64) -> u8 {
        self.active_roles(admin, scope, now)
            .iter()
            .map(|r| r.risk_ceiling)
            .max()
            .unwrap_or(0)
    }
}
