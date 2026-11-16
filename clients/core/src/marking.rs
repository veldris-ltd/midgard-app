//! Row markings as the consoles show them, on the Bell–LaPadula lattice the core
//! enforces. `[AC-C-13]` `[NFR-SEC-2]`
//!
//! The consoles mark rows `open`, `internal`, `restricted` or `confidential`
//! (`sdk/ts/src/types.ts` `MarkingLevel`), ordered as `MARKING_RANK` in
//! `sdk/ts/src/mock/clients.ts`. They map onto `policy::ClassificationLevel` 0..=3,
//! so `policy::dominates` decides what a session sees, exactly as it decides what
//! `midgard_client_store::LocalStore` may cache.

use std::collections::BTreeSet;

use policy::{dominates, ClassificationLevel, Clearance, Compartment, Marking};

/// A console marking level, weakest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MarkingLevel {
    /// `open` (unclassified).
    Open,
    /// `internal`.
    Internal,
    /// `restricted`.
    Restricted,
    /// `confidential` (the highest the consoles use).
    Confidential,
}

impl MarkingLevel {
    /// Every level, weakest first.
    pub const ALL: [MarkingLevel; 4] = [
        MarkingLevel::Open,
        MarkingLevel::Internal,
        MarkingLevel::Restricted,
        MarkingLevel::Confidential,
    ];

    /// Parse the consoles' word for a level.
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "open" => Some(Self::Open),
            "internal" => Some(Self::Internal),
            "restricted" => Some(Self::Restricted),
            "confidential" => Some(Self::Confidential),
            _ => None,
        }
    }

    /// The consoles' word for this level.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Internal => "internal",
            Self::Restricted => "restricted",
            Self::Confidential => "confidential",
        }
    }

    /// The level on the policy lattice.
    pub fn classification(self) -> ClassificationLevel {
        ClassificationLevel(self as u8)
    }
}

/// A row's marking: a level and the compartments it requires, sorted and unique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowMarking {
    /// The level.
    pub level: MarkingLevel,
    /// Required compartments, sorted, without duplicates.
    pub compartments: Vec<String>,
}

impl RowMarking {
    /// A marking at `level` requiring `compartments`.
    pub fn new<S: Into<String>>(
        level: MarkingLevel,
        compartments: impl IntoIterator<Item = S>,
    ) -> Self {
        let set: BTreeSet<String> = compartments.into_iter().map(Into::into).collect();
        Self {
            level,
            compartments: set.into_iter().collect(),
        }
    }

    /// The policy marking the lattice checks.
    pub fn to_policy(&self) -> Marking {
        Marking {
            level: self.level.classification(),
            compartments: self.compartments.iter().cloned().map(Compartment).collect(),
        }
    }

    /// `CONFIDENTIAL // OPS-N, SIG`, as the marking chips read.
    pub fn label(&self) -> String {
        let level = self.level.as_str().to_uppercase();
        if self.compartments.is_empty() {
            level
        } else {
            format!("{level} // {}", self.compartments.join(", "))
        }
    }
}

/// A session clearance at `level` holding `compartments`.
pub fn clearance(level: MarkingLevel, compartments: &[String]) -> Clearance {
    Clearance {
        level: level.classification(),
        compartments: compartments.iter().cloned().map(Compartment).collect(),
    }
}

/// Whether `clearance` may see a row marked `marking`.
pub fn may_see(clearance: &Clearance, marking: &RowMarking) -> bool {
    dominates(clearance, &marking.to_policy())
}

/// One row as a session may see it: in full, or withheld with its level only.
#[derive(Debug, Clone, PartialEq)]
pub enum Visible<T> {
    /// The session's clearance dominates the row's marking.
    Shown {
        /// The row id.
        id: String,
        /// The row marking (for the chip).
        marking: RowMarking,
        /// The row.
        item: T,
    },
    /// Above the session's clearance: neither the id nor the content is given.
    Withheld {
        /// The level, so the row can say what it withholds.
        level: MarkingLevel,
    },
}

impl<T> Visible<T> {
    /// The row, when shown.
    pub fn shown(&self) -> Option<&T> {
        match self {
            Visible::Shown { item, .. } => Some(item),
            Visible::Withheld { .. } => None,
        }
    }

    /// Map the row, keeping its visibility.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Visible<U> {
        match self {
            Visible::Shown { id, marking, item } => Visible::Shown {
                id,
                marking,
                item: f(item),
            },
            Visible::Withheld { level } => Visible::Withheld { level },
        }
    }
}

/// Apply `clearance` to `(id, marking, item)` rows, in order.
pub fn filter_rows<T>(
    clearance: &Clearance,
    rows: Vec<(String, RowMarking, T)>,
) -> Vec<Visible<T>> {
    rows.into_iter()
        .map(|(id, marking, item)| {
            if may_see(clearance, &marking) {
                Visible::Shown { id, marking, item }
            } else {
                Visible::Withheld {
                    level: marking.level,
                }
            }
        })
        .collect()
}
