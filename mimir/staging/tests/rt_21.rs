//! RT-21 — a native-provenance check is `Verified` only when bytes were compared. A
//! vendor signature that is merely present is recorded as unchecked, and an upstream
//! commit is verified only against the files pinned at that revision, where a changed
//! file is a `Mismatch` that stops re-signing. [FR-MS-2] [RT-21]

use std::collections::BTreeMap;

use model_server::HwProfile;
use pqc_agility::hash::sha256;
use staging::{
    verify_provenance, DeclaredProvenance, FetchedModel, ProvenanceCheck, ProvenanceRecord,
    UpstreamSource, Verifiability,
};

const WEIGHTS: &[u8] = b"GRANITE-WEIGHTS-BLOB";
const TOKENIZER: &[u8] = b"{tokenizer}";
const CONFIG: &[u8] = b"{config}";

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn fetched(declared: DeclaredProvenance) -> FetchedModel {
    let mut components = BTreeMap::new();
    components.insert("tokenizer.json".to_string(), TOKENIZER.to_vec());
    components.insert("config.json".to_string(), CONFIG.to_vec());
    FetchedModel {
        source: UpstreamSource::HuggingFace {
            repo: "ibm-granite/granite-3.3-2b-instruct".into(),
            revision: "7cdf86cc".into(),
        },
        proposed_id: "granite-3.3-2b-instruct-1".into(),
        family: "Granite".into(),
        version: "3.3".into(),
        hw: HwProfile::none(),
        weights: WEIGHTS.to_vec(),
        components,
        model_card: "# Granite".into(),
        metadata: "fixture".into(),
        declared,
    }
}

/// The listing an operator recorded for the pinned revision: every fetched file, plus
/// one that was not fetched.
fn pinned() -> BTreeMap<String, String> {
    [
        ("model.weights", hex(&sha256(WEIGHTS))),
        ("tokenizer.json", hex(&sha256(TOKENIZER))),
        ("config.json", hex(&sha256(CONFIG))),
        ("README.md", "ab".repeat(32)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

fn with_commit(files: Option<BTreeMap<String, String>>) -> ProvenanceRecord {
    verify_provenance(&fetched(DeclaredProvenance {
        hf_commit: Some("7cdf86ccd1f1bb3491c9b7017b033f2e51367397".into()),
        pinned_revision_files: files,
        ..DeclaredProvenance::default()
    }))
}

fn check<'a>(record: &'a ProvenanceRecord, name: &str) -> &'a ProvenanceCheck {
    record
        .checks
        .iter()
        .find(|c| c.check == name)
        .expect("check recorded")
}

#[test]
fn a_present_vendor_signature_is_never_reported_verified() {
    let signed = verify_provenance(&fetched(DeclaredProvenance {
        vendor_signature: Some("gpg:0xDEADBEEF".into()),
        ..DeclaredProvenance::default()
    }));
    let vendor = check(&signed, "vendor-signature");
    assert_eq!(vendor.result, Verifiability::Unverifiable);
    assert!(
        vendor.detail.contains("NOT cryptographically checked"),
        "{}",
        vendor.detail
    );
    assert!(
        !signed.any_verified(),
        "nothing was compared, so nothing is verified"
    );

    let unsigned = verify_provenance(&fetched(DeclaredProvenance::default()));
    assert_eq!(
        check(&unsigned, "vendor-signature").result,
        Verifiability::Unverifiable
    );
}

#[test]
fn a_commit_is_verified_only_against_matching_pinned_files() {
    let record = with_commit(Some(pinned()));
    assert_eq!(check(&record, "hf-commit").result, Verifiability::Verified);
    assert!(!record.has_mismatch());

    // Upper-case digests in the listing are the same digests.
    let upper = pinned()
        .into_iter()
        .map(|(k, v)| (k, v.to_ascii_uppercase()))
        .collect();
    assert_eq!(
        check(&with_commit(Some(upper)), "hf-commit").result,
        Verifiability::Verified
    );

    // A commit string alone checks nothing.
    let bare = with_commit(None);
    assert_eq!(
        check(&bare, "hf-commit").result,
        Verifiability::Unverifiable
    );
    assert!(!bare.any_verified());

    // A listing with no commit to pin it to checks nothing either.
    let no_commit = verify_provenance(&fetched(DeclaredProvenance {
        pinned_revision_files: Some(pinned()),
        ..DeclaredProvenance::default()
    }));
    assert_eq!(
        check(&no_commit, "hf-commit").result,
        Verifiability::Unverifiable
    );
}

#[test]
fn a_changed_or_uncovered_file_is_never_verified() {
    let mut tampered = pinned();
    tampered.insert(
        "tokenizer.json".into(),
        hex(&sha256(b"{another tokenizer}")),
    );
    let record = with_commit(Some(tampered));
    let commit = check(&record, "hf-commit");
    assert_eq!(commit.result, Verifiability::Mismatch);
    assert!(
        commit.detail.contains("tokenizer.json"),
        "{}",
        commit.detail
    );
    assert!(record.has_mismatch(), "a mismatch stops re-signing");

    let mut weights = pinned();
    weights.insert("model.weights".into(), "00".repeat(32));
    assert_eq!(
        check(&with_commit(Some(weights)), "hf-commit").result,
        Verifiability::Mismatch
    );

    let mut partial = pinned();
    partial.remove("config.json");
    let record = with_commit(Some(partial));
    let commit = check(&record, "hf-commit");
    assert_eq!(commit.result, Verifiability::Unverifiable);
    assert!(commit.detail.contains("config.json"), "{}", commit.detail);
}
