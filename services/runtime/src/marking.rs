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

use policy::ClassificationLevel;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_round_trip_and_nothing_else_is_a_band() {
        for name in BANDS {
            assert_eq!(band(level_of(name).unwrap()), name);
        }
        assert_eq!(level_of("secret"), None);
        assert_eq!(band(ClassificationLevel(9)), "confidential");
    }
}
