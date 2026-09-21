//! Domain model for role/marking/purpose access control. `BTreeSet` everywhere
//! so evaluation is deterministic (byte-identical decisions across runs).

use std::collections::BTreeSet;

/// A classification level; higher dominates lower (Bell–LaPadula).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClassificationLevel(pub u8);

impl ClassificationLevel {
    /// Unclassified (lowest).
    pub const UNCLASSIFIED: Self = Self(0);
    /// Confidential.
    pub const CONFIDENTIAL: Self = Self(1);
    /// Secret.
    pub const SECRET: Self = Self(2);
    /// Top Secret (highest).
    pub const TOP_SECRET: Self = Self(3);
}

/// A need-to-know compartment tag.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Compartment(pub String);

/// A role held by a subject.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Role(pub String);

/// A declared purpose of access.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Purpose(pub String);

/// An action being attempted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Action(pub String);

/// Identifier for a subject.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubjectId(pub String);

/// Identifier for a resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceId(pub String);

/// A mandatory classification marking on a resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marking {
    /// Classification level.
    pub level: ClassificationLevel,
    /// Required compartments.
    pub compartments: BTreeSet<Compartment>,
}

/// A subject's clearance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clearance {
    /// Held level.
    pub level: ClassificationLevel,
    /// Held compartments.
    pub compartments: BTreeSet<Compartment>,
}

/// An authenticated subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    /// Subject id.
    pub id: SubjectId,
    /// Roles held.
    pub roles: BTreeSet<Role>,
    /// Clearance.
    pub clearance: Clearance,
    /// Purposes the subject is authorised for.
    pub purposes: BTreeSet<Purpose>,
}

/// A protected resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    /// Resource id.
    pub id: ResourceId,
    /// Mandatory marking.
    pub marking: Marking,
    /// Purposes for which this resource may be accessed.
    pub allowed_purposes: BTreeSet<Purpose>,
}

/// The effect of a rule / decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Access is granted.
    Permit,
    /// Access is denied.
    Deny,
}

/// A single access request to be decided.
#[derive(Debug, Clone)]
pub struct AccessRequest<'a> {
    /// The requesting subject.
    pub subject: &'a Subject,
    /// The target resource.
    pub resource: &'a Resource,
    /// The action attempted.
    pub action: Action,
    /// The declared purpose.
    pub purpose: Purpose,
}
