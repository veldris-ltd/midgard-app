//! Bell–LaPadula dominance: a clearance dominates a marking iff it is at least
//! as high in level and holds every required compartment (no read-up + need to
//! know).

use crate::model::{Clearance, Compartment, Marking};

/// Whether `clearance` dominates `marking`.
pub fn dominates(clearance: &Clearance, marking: &Marking) -> bool {
    clearance.level >= marking.level && marking.compartments.is_subset(&clearance.compartments)
}

/// The compartments required by `marking` that `clearance` lacks.
pub fn missing_compartments(clearance: &Clearance, marking: &Marking) -> Vec<Compartment> {
    marking
        .compartments
        .difference(&clearance.compartments)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ClassificationLevel;
    use std::collections::BTreeSet;

    fn cl(level: ClassificationLevel, comps: &[&str]) -> Clearance {
        Clearance {
            level,
            compartments: comps.iter().map(|c| Compartment(c.to_string())).collect(),
        }
    }
    fn mk(level: ClassificationLevel, comps: &[&str]) -> Marking {
        Marking {
            level,
            compartments: comps.iter().map(|c| Compartment(c.to_string())).collect(),
        }
    }

    #[test]
    fn reflexive() {
        let c = cl(ClassificationLevel::SECRET, &["A"]);
        let m = mk(ClassificationLevel::SECRET, &["A"]);
        assert!(dominates(&c, &m));
    }

    #[test]
    fn no_read_up() {
        let c = cl(ClassificationLevel::CONFIDENTIAL, &[]);
        let m = mk(ClassificationLevel::SECRET, &[]);
        assert!(!dominates(&c, &m));
    }

    #[test]
    fn missing_compartment_denies() {
        let c = cl(ClassificationLevel::TOP_SECRET, &["A"]);
        let m = mk(ClassificationLevel::SECRET, &["A", "B"]);
        assert!(!dominates(&c, &m));
        assert_eq!(
            missing_compartments(&c, &m),
            vec![Compartment("B".to_string())]
        );
    }

    #[test]
    fn empty_marking_dominated_by_any_level() {
        let c = cl(ClassificationLevel::UNCLASSIFIED, &[]);
        let m = mk(ClassificationLevel::UNCLASSIFIED, &[]);
        assert!(dominates(&c, &m));
        let _ = BTreeSet::<Compartment>::new();
    }
}
