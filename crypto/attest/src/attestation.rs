//! The `.att` file: an in-toto Statement v1 carrying build provenance.
//!
//! Rendered by a deterministic encoder in the style of `tools/mg-sbom` — keys in
//! a fixed order, arrays pre-sorted, **no timestamps**, no map-iteration
//! dependence — so signing the same artefact from the same source revision twice
//! produces byte-identical attestations. `[AC-RF-SC-1f]` `[NFR-OSS-1]`
//!
//! The attestation is bound into the signing preimage (see [`crate::preimage`]),
//! so it cannot be swapped for one from another build without breaking the
//! signature. That is why nothing here parses JSON: the verifier recomputes this
//! file's digest and lets the signature adjudicate, rather than trusting a parser
//! over attacker-controlled bytes.

use crate::hex::hex;

/// in-toto statement type.
pub const STATEMENT_TYPE: &str = "https://in-toto.io/Statement/v1";
/// The suite's provenance predicate type.
pub const PREDICATE_TYPE: &str = "https://veldris.io/midgard/Provenance/v1";
/// The build type this gate attests to.
pub const BUILD_TYPE: &str = "https://veldris.io/midgard/BuildType/cargo-hermetic/v1";

/// One package's reproducibility result, as recorded by `mg-repro`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReproEntry {
    /// Workspace package name.
    pub package: String,
    /// The digest both builds produced.
    pub sha256: String,
}

/// The build facts recorded in the predicate. Supplied by the caller; this crate
/// does not go looking at the environment, so the attestation is a pure function
/// of its inputs and stays reproducible.
#[derive(Debug, Clone, Default)]
pub struct Provenance {
    /// Source revision (git commit) the artefact was built from.
    pub source_revision: String,
    /// Digest of the pinned toolchain description.
    pub toolchain_sha256: String,
    /// `rustc --version` of the building toolchain.
    pub rustc_version: String,
    /// Per-package reproducibility digests from `mg-repro`, if available.
    pub repro: Vec<ReproEntry>,
}

/// What the signer is attesting to.
pub struct Subject<'a> {
    /// Artefact file name (basename, so the attestation does not leak build paths).
    pub name: &'a str,
    /// SHA-256 of the artefact bytes.
    pub artefact_sha256: &'a [u8; 32],
    /// SHA-256 of the CycloneDX SBOM bytes.
    pub sbom_sha256: &'a [u8; 32],
    /// Signing algorithm, for the reader's benefit (authority comes from the
    /// `.sig` envelope, not from here).
    pub algorithm: &'a str,
    /// Signing key id.
    pub key_id: &'a str,
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

/// Render the attestation. Deterministic: identical inputs give identical bytes.
///
/// `repro` entries are sorted here rather than trusted to arrive sorted, so a
/// caller cannot accidentally make the output order-dependent.
pub fn render(subject: &Subject<'_>, prov: &Provenance) -> String {
    let mut repro = prov.repro.clone();
    repro.sort();
    repro.dedup();

    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!("  \"_type\": \"{}\",\n", esc(STATEMENT_TYPE)));
    s.push_str(&format!(
        "  \"predicateType\": \"{}\",\n",
        esc(PREDICATE_TYPE)
    ));
    s.push_str("  \"subject\": [\n");
    s.push_str("    {\n");
    s.push_str(&format!(
        "      \"digest\": {{ \"sha256\": \"{}\" }},\n",
        hex(subject.artefact_sha256)
    ));
    s.push_str(&format!("      \"name\": \"{}\"\n", esc(subject.name)));
    s.push_str("    }\n");
    s.push_str("  ],\n");
    s.push_str("  \"predicate\": {\n");
    s.push_str(&format!("    \"buildType\": \"{}\",\n", esc(BUILD_TYPE)));
    s.push_str("    \"hermetic\": true,\n");
    s.push_str("    \"reproducible\": [");
    if repro.is_empty() {
        s.push(']');
        s.push_str(",\n");
    } else {
        s.push('\n');
        for (i, e) in repro.iter().enumerate() {
            s.push_str(&format!(
                "      {{ \"package\": \"{}\", \"sha256\": \"{}\" }}",
                esc(&e.package),
                esc(&e.sha256)
            ));
            if i + 1 < repro.len() {
                s.push(',');
            }
            s.push('\n');
        }
        s.push_str("    ],\n");
    }
    s.push_str(&format!(
        "    \"rustcVersion\": \"{}\",\n",
        esc(&prov.rustc_version)
    ));
    s.push_str(&format!(
        "    \"sbomSha256\": \"{}\",\n",
        hex(subject.sbom_sha256)
    ));
    s.push_str("    \"signature\": {\n");
    s.push_str(&format!(
        "      \"algorithm\": \"{}\",\n",
        esc(subject.algorithm)
    ));
    s.push_str(&format!("      \"keyId\": \"{}\"\n", esc(subject.key_id)));
    s.push_str("    },\n");
    s.push_str(&format!(
        "    \"sourceRevision\": \"{}\",\n",
        esc(&prov.source_revision)
    ));
    s.push_str(&format!(
        "    \"toolchainSha256\": \"{}\"\n",
        esc(&prov.toolchain_sha256)
    ));
    s.push_str("  }\n");
    s.push_str("}\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject<'a>(art: &'a [u8; 32], sbom: &'a [u8; 32]) -> Subject<'a> {
        Subject {
            name: "mg-sbom",
            artefact_sha256: art,
            sbom_sha256: sbom,
            algorithm: "ML-DSA-87",
            key_id: "release-key-1",
        }
    }

    fn prov() -> Provenance {
        Provenance {
            source_revision: "71e2a58".into(),
            toolchain_sha256: "aa".repeat(32),
            rustc_version: "rustc 1.82.0".into(),
            repro: vec![
                ReproEntry {
                    package: "mg-sign".into(),
                    sha256: "bb".repeat(32),
                },
                ReproEntry {
                    package: "mg-sbom".into(),
                    sha256: "cc".repeat(32),
                },
            ],
        }
    }

    #[test]
    fn is_byte_identical_across_runs() {
        let (a, b) = ([1u8; 32], [2u8; 32]);
        assert_eq!(
            render(&subject(&a, &b), &prov()),
            render(&subject(&a, &b), &prov())
        );
    }

    #[test]
    fn sorts_repro_entries_regardless_of_input_order() {
        let (a, b) = ([1u8; 32], [2u8; 32]);
        let mut reversed = prov();
        reversed.repro.reverse();
        assert_eq!(
            render(&subject(&a, &b), &prov()),
            render(&subject(&a, &b), &reversed),
            "output must not depend on the order entries arrived in"
        );
    }

    #[test]
    fn carries_no_timestamp() {
        let (a, b) = ([1u8; 32], [2u8; 32]);
        let out = render(&subject(&a, &b), &prov());
        for banned in ["timestamp", "buildStartedOn", "buildFinishedOn", "date"] {
            assert!(
                !out.contains(banned),
                "{banned} would break reproducibility"
            );
        }
    }

    #[test]
    fn binds_the_subject_and_sbom_digests() {
        let (a, b) = ([0xab; 32], [0xcd; 32]);
        let out = render(&subject(&a, &b), &prov());
        assert!(out.contains(&hex(&a)), "subject digest present");
        assert!(out.contains(&hex(&b)), "sbom digest present");
        assert!(out.contains("\"_type\": \"https://in-toto.io/Statement/v1\""));
        assert!(out.contains("\"hermetic\": true"));
    }

    #[test]
    fn changing_any_input_changes_the_output() {
        let (a, b) = ([1u8; 32], [2u8; 32]);
        let base = render(&subject(&a, &b), &prov());
        let other = [9u8; 32];
        assert_ne!(render(&subject(&other, &b), &prov()), base);
        assert_ne!(render(&subject(&a, &other), &prov()), base);
        let mut p = prov();
        p.source_revision = "deadbee".into();
        assert_ne!(render(&subject(&a, &b), &p), base);
    }

    #[test]
    fn escapes_hostile_field_content() {
        let (a, b) = ([1u8; 32], [2u8; 32]);
        let mut p = prov();
        p.source_revision = "\"},\"injected\":{\"".into();
        let out = render(&subject(&a, &b), &p);
        assert!(!out.contains("\"injected\""), "quotes must be escaped");
        assert!(out.contains("\\\""));
    }

    #[test]
    fn renders_an_empty_repro_list() {
        let (a, b) = ([1u8; 32], [2u8; 32]);
        let mut p = prov();
        p.repro.clear();
        let out = render(&subject(&a, &b), &p);
        assert!(out.contains("\"reproducible\": [],"));
    }
}
