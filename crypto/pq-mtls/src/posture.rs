//! Device/workload posture presented during the handshake, mapped to a clearance
//! for the authorization decision.

use std::collections::BTreeSet;

use policy::{ClassificationLevel, Compartment};

/// Attested posture of the connecting workload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Posture {
    /// Posture level (maps to clearance level for the channel decision).
    pub level: ClassificationLevel,
    /// Posture compartments.
    pub compartments: BTreeSet<Compartment>,
}

impl Posture {
    /// A minimal unclassified posture with no compartments.
    pub fn unclassified() -> Self {
        Self::at(ClassificationLevel::UNCLASSIFIED)
    }

    /// A posture at `level` with no compartments.
    pub fn at(level: ClassificationLevel) -> Self {
        Self {
            level,
            compartments: BTreeSet::new(),
        }
    }
}
