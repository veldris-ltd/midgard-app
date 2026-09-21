//! Rollout plans: an ordered set of rings from a small **canary** to the **broad**
//! fleet. Deploying ring-by-ring — smallest blast radius first — is what makes a
//! bad release catchable at the canary before it reaches everyone. [FR-B-2]

/// A software version identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionId(pub String);

impl VersionId {
    /// Build a version id.
    pub fn new(v: impl Into<String>) -> Self {
        Self(v.into())
    }
}

/// One rollout stage: a named ring covering some number of installations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ring {
    /// Ring name, e.g. `canary`, `broad`.
    pub name: String,
    /// How many installations this ring covers.
    pub installs: usize,
}

impl Ring {
    /// Build a ring.
    pub fn new(name: impl Into<String>, installs: usize) -> Self {
        Self {
            name: name.into(),
            installs,
        }
    }
}

/// An ordered rollout plan (rings applied front to back).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutPlan {
    /// Rings in application order (canary first).
    pub rings: Vec<Ring>,
}

impl RolloutPlan {
    /// A plan from explicit rings.
    pub fn new(rings: Vec<Ring>) -> Self {
        Self { rings }
    }

    /// Total installations across all rings.
    pub fn total_installs(&self) -> usize {
        self.rings.iter().map(|r| r.installs).sum()
    }

    /// A simple canary→broad plan: a canary of `canary` installations then the
    /// remaining `total - canary` as a broad ring. The canary is clamped to the
    /// fleet size, and a zero-size ring is omitted.
    pub fn canary_broad(total: usize, canary: usize) -> Self {
        let canary = canary.min(total);
        let mut rings = Vec::new();
        if canary > 0 {
            rings.push(Ring::new("canary", canary));
        }
        let broad = total - canary;
        if broad > 0 {
            rings.push(Ring::new("broad", broad));
        }
        Self { rings }
    }
}
