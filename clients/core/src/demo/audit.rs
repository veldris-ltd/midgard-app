//! Audit-row signatures in the demo: what a row's check mark rests on. `[NFR-PQ-3]`
//! `[AC-C-13]`
//!
//! Two kinds of audit row reach a demo screen, and neither is signed by the dataset
//! key, which never ships in an app:
//!
//! - **Rows built into the dataset** may carry their own signature, made at build
//!   time by a separate demo-audit key under its own domain,
//!   [`attest::accept::DEMO_AUDIT`]. The signature covers
//!   `preimage(DEMO_AUDIT, canonical {collection, id, item, marking})` and travels in
//!   the row as [`ROW_SIG_FIELD`]: the hex of an attest `MGSIG1` envelope. A row that
//!   carries one is accepted only if it verifies under a pinned demo-audit anchor;
//!   otherwise the whole dataset is refused. A row without one is
//!   [`SignatureStatus::Unsigned`].
//! - **Rows the enclosure appends at run time** (one per action) are
//!   [`SignatureStatus::Unsigned`] when made. The device key signs them later (sealed
//!   keystore and session, Tasks A9/A10; Secure Enclave root, A13), and only a check
//!   under that key makes one [`SignatureStatus::Device`].
//!
//! Nothing here marks a row verified without an [`attest::accept::accept`] check, and
//! the mock's `pqSignature` text (`ML-DSA ✓`) is never passed on: the enclosure
//! delivers [`SignatureStatus::as_str`] in its place.

use attest::accept::{accept, preimage, DEMO_AUDIT};
use attest::envelope::SigEnvelope;
use attest::hex::unhex;
use attest::json::Json;
use attest::TrustAnchors;

use super::{canon, dataset};
use crate::marking::RowMarking;

/// The row field holding a build-time audit-row signature (hex `MGSIG1` envelope).
pub const ROW_SIG_FIELD: &str = "auditSig";

/// The read-model type whose rows carry a signature status.
pub const AUDIT_TYPE: &str = "AuditRecord";

/// What an audit row's signature check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureStatus {
    /// Its build-time signature verified under a pinned demo-audit anchor.
    Verified,
    /// Signed by this device's key and verified under it (set by the session once
    /// the device key exists; the demo enclosure never sets it).
    Device,
    /// No signature has been checked: show no check mark.
    Unsigned,
}

impl SignatureStatus {
    /// `verified`, `device` or `none`: the words a screen shows.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Device => "device",
            Self::Unsigned => "none",
        }
    }

    /// Whether a screen may show a check mark.
    pub fn checked(self) -> bool {
        !matches!(self, Self::Unsigned)
    }
}

/// The bytes an audit-row signature covers, before the domain preimage: the
/// canonical JSON of `{collection, id, item, marking}`, so neither the row's place,
/// its id nor its marking can be swapped under a valid signature.
pub fn signed_bytes(collection: &str, id: &str, marking: &RowMarking, item: &Json) -> Vec<u8> {
    let marking = Json::Obj(vec![
        (
            "compartments".into(),
            Json::Arr(
                marking
                    .compartments()
                    .iter()
                    .map(|c| Json::Str(c.clone()))
                    .collect(),
            ),
        ),
        ("level".into(), Json::Str(marking.level().as_str().into())),
    ]);
    canon::encode(&Json::Obj(vec![
        ("collection".into(), Json::Str(collection.into())),
        ("id".into(), Json::Str(id.into())),
        ("item".into(), item.clone()),
        ("marking".into(), marking),
    ]))
    .into_bytes()
}

/// Verify a row's [`ROW_SIG_FIELD`] value under `anchors`. `Ok` is the only way to
/// [`SignatureStatus::Verified`]; every finding is an `Err`.
pub fn verify_row(
    collection: &str,
    id: &str,
    marking: &RowMarking,
    item: &Json,
    sig_hex: &str,
    anchors: &TrustAnchors,
) -> Result<SignatureStatus, String> {
    let bytes = unhex(sig_hex).ok_or("audit signature is not hex")?;
    let env = SigEnvelope::parse(&bytes).map_err(|e| format!("audit signature: {e}"))?;
    if env.domain != DEMO_AUDIT.label() {
        return Err("audit signature is not under the demo-audit domain".into());
    }
    let anchor = anchors.get(&env.key_id).ok_or_else(|| {
        let shown: String = env.key_id.chars().take(dataset::KEY_ID_SHOWN).collect();
        format!("audit signing key {shown:?} is not pinned")
    })?;
    accept(
        &DEMO_AUDIT,
        &signed_bytes(collection, id, marking, item),
        anchor,
        &env.signature,
        &dataset::APPROVED,
    )
    .map_err(|e| format!("audit signature refused: {e}"))?;
    Ok(SignatureStatus::Verified)
}

/// `item` with its `pqSignature` text replaced by `status`'s word.
pub fn with_status(item: &Json, status: SignatureStatus) -> Json {
    match item {
        Json::Obj(pairs) => Json::Obj(
            pairs
                .iter()
                .map(|(k, v)| {
                    if k == "pqSignature" {
                        (k.clone(), Json::Str(status.as_str().into()))
                    } else {
                        (k.clone(), v.clone())
                    }
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The word that replaces an unchecked signature or verification claim.
pub const NOT_CHECKED: &str = "not checked (demo)";

/// Fields outside audit rows whose fixture text claims a signature or a verification
/// (`ML-DSA ✓`, `Verified`) that nothing in the demo checks: `(read-model type, field)`.
/// [`without_claims`] replaces a claim in one of these with [`NOT_CHECKED`] at
/// delivery; a check mark anywhere else refuses the dataset ([`unlisted_claim`]).
pub const CLAIM_FIELDS: &[(&str, &str)] = &[
    ("BifrostArtefact", "signature"),
    ("BifrostArtefact", "stateLabel"),
    ("BifrostFleetNode", "agent"),
    ("HeimdallDecision", "signature"),
];

/// Whether `text` claims a signature or a verification.
pub fn claims(text: &str) -> bool {
    text.contains('\u{2713}') || text.to_lowercase().contains("verified")
}

/// `item` (of `type_name`) with each claim in a [`CLAIM_FIELDS`] field replaced by
/// [`NOT_CHECKED`].
pub fn without_claims(type_name: &str, item: &Json) -> Json {
    let listed = |k: &str| CLAIM_FIELDS.iter().any(|(t, f)| *t == type_name && *f == k);
    match item {
        Json::Obj(pairs) => Json::Obj(
            pairs
                .iter()
                .map(|(k, v)| match v {
                    Json::Str(text) if listed(k) && claims(text) => {
                        (k.clone(), Json::Str(NOT_CHECKED.into()))
                    }
                    _ => (k.clone(), v.clone()),
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The path of a check mark in `item` (of `type_name`) that delivery would not
/// replace, if any: not an audit row's `pqSignature`, not a [`CLAIM_FIELDS`] field.
pub fn unlisted_claim(type_name: &str, item: &Json) -> Option<String> {
    fn walk(v: &Json, path: &str, top: bool, skip: &dyn Fn(&str) -> bool) -> Option<String> {
        match v {
            Json::Str(s) if s.contains('\u{2713}') => Some(path.to_string()),
            Json::Arr(items) => items
                .iter()
                .enumerate()
                .find_map(|(i, x)| walk(x, &format!("{path}[{i}]"), false, skip)),
            Json::Obj(pairs) => pairs.iter().find_map(|(k, x)| {
                if top && skip(k) {
                    None
                } else {
                    walk(x, &format!("{path}.{k}"), false, skip)
                }
            }),
            _ => None,
        }
    }
    let skip = |k: &str| {
        (type_name == AUDIT_TYPE && k == "pqSignature")
            || CLAIM_FIELDS.iter().any(|(t, f)| *t == type_name && *f == k)
    };
    walk(item, "$", true, &skip)
}

/// The whole message an audit-row signature is made over, so a build step that signs
/// rows (and the tests) make exactly what [`verify_row`] checks.
pub fn row_preimage(collection: &str, id: &str, marking: &RowMarking, item: &Json) -> Vec<u8> {
    preimage(&DEMO_AUDIT, &signed_bytes(collection, id, marking, item))
}
