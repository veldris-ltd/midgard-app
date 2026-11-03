//! Marking bands: the names deployment files and consoles give the policy classification
//! levels, lowest first — `open`, `internal`, `restricted`, `confidential`. Shared, so
//! the gateway reading a record and the service writing it agree on what a band means.
//! [NFR-SEC-2]
//!
//! The policy lattice and the bands name their four levels differently —
//! `UNCLASSIFIED < CONFIDENTIAL < SECRET < TOP_SECRET` against
//! `open < internal < restricted < confidential` — so the mapping is **by ordinal**.
//! That is the only mapping that preserves dominance: an object labelled
//! "confidential" is genuinely in the highest band, exactly as the PDP enforces it. The
//! names do not line up (the policy crate's `CONFIDENTIAL` is "internal"); the order
//! does, and the order is what is enforced.

use std::collections::BTreeSet;

use policy::{ClassificationLevel, Compartment, Marking};

/// The bands, lowest first; a band's position is its level.
pub const BANDS: [&str; 4] = ["open", "internal", "restricted", "confidential"];

/// The band for a policy classification level; a level above the highest band is the
/// highest band.
pub fn band(level: ClassificationLevel) -> &'static str {
    BANDS[usize::from(level.0).min(BANDS.len() - 1)]
}

/// The policy classification level for a band, if it is one.
pub fn level_of(band: &str) -> Option<ClassificationLevel> {
    BANDS
        .iter()
        .position(|b| *b == band)
        .and_then(|i| u8::try_from(i).ok())
        .map(ClassificationLevel)
}

/// A marking from `BAND[:COMPARTMENT,...]`, as `--resource-marking` takes it: a band
/// and, after a colon, the compartments it requires. [VLD-ANB-MIDGARD-0063]
pub fn parse_marking(spec: &str) -> Result<Marking, String> {
    let (name, compartments) = match spec.split_once(':') {
        Some((name, list)) => (name, Some(list)),
        None => (spec, None),
    };
    let level = level_of(name).ok_or_else(|| {
        format!("a marking band is open, internal, restricted or confidential, not '{name}'")
    })?;
    let mut set = BTreeSet::new();
    for c in compartments.into_iter().flat_map(|l| l.split(',')) {
        let c = c.trim();
        if c.is_empty() {
            return Err(format!("'{spec}' names an empty compartment"));
        }
        set.insert(Compartment(c.to_string()));
    }
    Ok(Marking {
        level,
        compartments: set,
    })
}

/// The marking a mesh resource has when its service is given none: open, with no
/// compartments.
pub fn open_marking() -> Marking {
    Marking {
        level: ClassificationLevel::UNCLASSIFIED,
        compartments: BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resource_marking_is_a_band_and_its_compartments() {
        // [VLD-ANB-MIDGARD-0063]
        assert_eq!(parse_marking("open").unwrap(), open_marking());
        let m = parse_marking("restricted:OPS-N,OPS-S").unwrap();
        assert_eq!(m.level, level_of("restricted").unwrap());
        assert_eq!(
            m.compartments,
            [Compartment("OPS-N".into()), Compartment("OPS-S".into())]
                .into_iter()
                .collect()
        );
        assert!(parse_marking("secret").is_err());
        assert!(parse_marking("internal:").is_err());
        assert!(parse_marking("internal:A,,B").is_err());
    }

    #[test]
    fn bands_round_trip_and_nothing_else_is_a_band() {
        for name in BANDS {
            assert_eq!(band(level_of(name).unwrap()), name);
        }
        assert_eq!(level_of("secret"), None);
        assert_eq!(band(ClassificationLevel(9)), "confidential");
    }
}
