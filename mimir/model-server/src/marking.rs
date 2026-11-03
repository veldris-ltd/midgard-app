//! The marking an inference request carries, and the clearance a model is accredited
//! to. [VLD-ANB-MIDGARD-0034]
//!
//! A request's marking is the aggregate marking of the data its consumer put in the
//! prompt; the model's accreditation is the clearance it is authorised against. The
//! handler hands both to the existing PDP, whose dominance check refuses a prompt the
//! model is not accredited for, exactly as it refuses any other marked resource.
//!
//! Levels are named as the policy lattice names them (`UNCLASSIFIED`, `CONFIDENTIAL`,
//! `SECRET`, `TOP_SECRET`); a marking is written `LEVEL[:COMPARTMENT,...]`, the form
//! both `--model-marking` and the ledger's completion entry use. A compartment name is
//! 1 to 64 ASCII letters, digits, `-`, `_` or `.`, so a label can never break the
//! space-separated `key=value` fields of a ledger entry.

use std::collections::BTreeSet;

use policy::{ClassificationLevel, Clearance, Compartment, Marking};

/// The level names, lowest first; a name's position is its level.
const LEVELS: [&str; 4] = ["UNCLASSIFIED", "CONFIDENTIAL", "SECRET", "TOP_SECRET"];

/// The longest compartment name accepted.
pub const MAX_COMPARTMENT_LEN: usize = 64;

/// The highest level a request may carry; anything above is unknown (fail closed).
pub const MAX_LEVEL: ClassificationLevel = ClassificationLevel::TOP_SECRET;

/// The marking of a prompt that holds no marked data.
pub fn unclassified() -> Marking {
    Marking {
        level: ClassificationLevel::UNCLASSIFIED,
        compartments: BTreeSet::new(),
    }
}

/// The name of a known level.
pub fn level_name(level: ClassificationLevel) -> Option<&'static str> {
    LEVELS.get(usize::from(level.0)).copied()
}

/// The level a name denotes, if it is one.
pub fn parse_level(name: &str) -> Option<ClassificationLevel> {
    LEVELS
        .iter()
        .position(|l| *l == name)
        .and_then(|i| u8::try_from(i).ok())
        .map(ClassificationLevel)
}

/// Whether `name` is an acceptable compartment name.
pub fn is_valid_compartment(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_COMPARTMENT_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// The label of a marking: the level name, then `:` and the compartments joined by
/// `,` when there are any (`SECRET:ALPHA,BRAVO`). An unknown level is written `L<n>`.
pub fn marking_label(marking: &Marking) -> String {
    let level = level_name(marking.level)
        .map(str::to_string)
        .unwrap_or_else(|| format!("L{}", marking.level.0));
    if marking.compartments.is_empty() {
        level
    } else {
        let compartments: Vec<&str> = marking.compartments.iter().map(|c| c.0.as_str()).collect();
        format!("{level}:{}", compartments.join(","))
    }
}

/// Parse `LEVEL[:COMPARTMENT,...]`; an unknown level, an invalid or repeated
/// compartment, or an empty list after `:` is refused.
pub fn parse_marking(text: &str) -> Result<Marking, String> {
    let (level, compartments) = match text.split_once(':') {
        Some((level, list)) => (level, Some(list)),
        None => (text, None),
    };
    let level = parse_level(level).ok_or_else(|| {
        format!(
            "unknown classification level {level:?}; expected one of {}",
            LEVELS.join(", ")
        )
    })?;
    let mut set = BTreeSet::new();
    if let Some(list) = compartments {
        for name in list.split(',') {
            if !is_valid_compartment(name) {
                return Err(format!("invalid compartment {name:?}"));
            }
            if !set.insert(Compartment(name.to_string())) {
                return Err(format!("compartment {name:?} is repeated"));
            }
        }
    }
    Ok(Marking {
        level,
        compartments: set,
    })
}

/// The clearance a model accredited to `marking` holds.
pub fn accreditation(marking: &Marking) -> Clearance {
    Clearance {
        level: marking.level,
        compartments: marking.compartments.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // [VLD-ANB-MIDGARD-0034] [AC-65.6]
    #[test]
    fn markings_parse_and_label_round_trip() {
        for text in [
            "UNCLASSIFIED",
            "SECRET",
            "TOP_SECRET:ALPHA",
            "SECRET:ALPHA,BRAVO",
        ] {
            assert_eq!(marking_label(&parse_marking(text).unwrap()), text);
        }
        // Compartments are kept sorted, so the label is canonical.
        let m = parse_marking("CONFIDENTIAL:BRAVO,ALPHA").unwrap();
        assert_eq!(marking_label(&m), "CONFIDENTIAL:ALPHA,BRAVO");
        assert_eq!(marking_label(&unclassified()), "UNCLASSIFIED");
        assert_eq!(
            marking_label(&Marking {
                level: ClassificationLevel(9),
                compartments: BTreeSet::new()
            }),
            "L9"
        );
    }

    // [VLD-ANB-MIDGARD-0034] [AC-65.6]
    #[test]
    fn unknown_levels_and_bad_compartments_are_refused() {
        for text in [
            "",
            "OFFICIAL",
            "secret",
            "SECRET:",
            "SECRET:ALPHA,",
            "SECRET:ALPHA,ALPHA",
            "SECRET:AL PHA",
            "SECRET:a=b",
        ] {
            assert!(parse_marking(text).is_err(), "{text:?} must be refused");
        }
        assert!(!is_valid_compartment(&"A".repeat(MAX_COMPARTMENT_LEN + 1)));
    }
}
