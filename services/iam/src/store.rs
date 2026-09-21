//! The in-memory identity store: users with their roles, clearance, and
//! authorised purposes. Fully in-process — no database, no network — so the IdP
//! functions in a fully air-gapped enclosure. [NFR-PRV-1] [NFR-SEC-2]

use std::collections::{BTreeMap, BTreeSet};

use policy::{Clearance, Purpose, Role};

/// A registered principal's authorization attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRecord {
    /// Roles the user holds.
    pub roles: BTreeSet<Role>,
    /// The user's clearance (level + compartments).
    pub clearance: Clearance,
    /// Purposes the user is authorised to declare.
    pub purposes: BTreeSet<Purpose>,
}

/// A directory of principals, keyed by subject id.
#[derive(Debug, Clone, Default)]
pub struct IdentityStore {
    users: BTreeMap<String, UserRecord>,
}

impl IdentityStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a user.
    pub fn upsert(&mut self, id: &str, record: UserRecord) {
        self.users.insert(id.to_string(), record);
    }

    /// Look up a user.
    pub fn get(&self, id: &str) -> Option<&UserRecord> {
        self.users.get(id)
    }

    /// Number of registered users.
    pub fn len(&self) -> usize {
        self.users.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }
}
