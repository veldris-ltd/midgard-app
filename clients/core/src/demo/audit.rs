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
//!   [`SignatureStatus::Unsigned`] when made. The session that made the action signs
//!   such a row with this device's audit key ([`crate::device_audit`], under its own
//!   domain [`attest::accept::DEVICE_AUDIT`], the key sealed to the device's key
//!   root), and only a check under that key, on every read, makes one
//!   [`SignatureStatus::Device`] ([`crate::session`]).
//!
//! Nothing here marks a row verified without an [`attest::accept::accept`] check, and
//! the mock's `pqSignature` text (`ML-DSA ✓`) is never passed on: the enclosure
//! delivers [`SignatureStatus::as_str`] in its place.

use attest::accept::{accept, preimage, DEMO_AUDIT};
use attest::envelope::SigEnvelope;
use attest::hex::{hex, unhex};
use attest::json::{parse, Json};
use attest::{SignError, SigningBackend, TrustAnchors};
use pqc_agility::{Signature, VerifyingKey};

use super::{canon, dataset};
use crate::marking::{MarkingLevel, RowMarking};

/// The row field holding a build-time audit-row signature (hex `MGSIG1` envelope).
pub const ROW_SIG_FIELD: &str = "auditSig";

/// The read-model type whose rows carry a signature status.
pub const AUDIT_TYPE: &str = "AuditRecord";

/// What an audit row's signature check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureStatus {
    /// Its build-time signature verified under a pinned demo-audit anchor.
    Verified,
    /// Signed by this device's audit key and verified under it (set only by the
    /// session, after the check; a source's claim of it is not believed, and the
    /// demo enclosure never sets it).
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
pub const NOT_CHECKED: &str = "not checked (sample)";

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

/// Whether `msg` is exactly `preimage(DEMO_AUDIT, signed_bytes(..))` for some audit
/// row: the canonical `{collection, id, item, marking}` with string collection and
/// id, an object item and a well-formed marking, and nothing after it.
fn is_row_preimage(msg: &[u8]) -> bool {
    let Some(rest) = msg.strip_prefix(preimage(&DEMO_AUDIT, b"").as_slice()) else {
        return false;
    };
    let Some(v) = std::str::from_utf8(rest).ok().and_then(|t| parse(t).ok()) else {
        return false;
    };
    let Json::Obj(pairs) = &v else {
        return false;
    };
    let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    if sorted != ["collection", "id", "item", "marking"] {
        return false;
    }
    let (Some(collection), Some(id), Some(item @ Json::Obj(_)), Some(m)) = (
        v.get("collection").and_then(Json::as_str),
        v.get("id").and_then(Json::as_str),
        v.get("item"),
        v.get("marking").and_then(|m| marking_json(m).ok()),
    ) else {
        return false;
    };
    // Rebuilding the bytes from the parts must give the message back exactly: this
    // refuses non-canonical text, extra marking keys and unsorted compartments.
    signed_bytes(collection, id, &m, item) == rest
}

/// A row marking `{"compartments": [..], "level": ..}`, strictly.
fn marking_json(m: &Json) -> Result<RowMarking, String> {
    let Json::Obj(pairs) = m else {
        return Err("marking is not an object".into());
    };
    if pairs.len() != 2 {
        return Err("marking has keys other than level and compartments".into());
    }
    let level = m
        .get("level")
        .and_then(Json::as_str)
        .and_then(MarkingLevel::parse)
        .ok_or("marking level")?;
    let compartments = m
        .get("compartments")
        .and_then(Json::as_array)
        .ok_or("marking compartments")?
        .iter()
        .map(|c| c.as_str().map(str::to_string).ok_or("marking compartment"))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RowMarking::new(level, compartments))
}

/// A signer that signs audit-row preimages and nothing else (the whole message is
/// checked, see `is_row_preimage`). The build step wraps the unsealed demo-audit
/// key in this type at once and never hands it to a general signer.
pub struct DemoAuditOnly<S> {
    inner: S,
}

impl<S: SigningBackend> DemoAuditOnly<S> {
    /// Wrap `inner`.
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S: SigningBackend> SigningBackend for DemoAuditOnly<S> {
    fn key_id(&self) -> &str {
        self.inner.key_id()
    }

    fn sign(&self, msg: &[u8]) -> Result<Signature, SignError> {
        if !is_row_preimage(msg) {
            return Err(SignError::Custodian(
                "the demo-audit key signs audit-row preimages only".into(),
            ));
        }
        self.inner.sign(msg)
    }

    fn verifying_key(&self) -> &VerifyingKey {
        self.inner.verifying_key()
    }
}

/// The pairs of a JSON object.
type Pairs = Vec<(String, Json)>;

/// What [`map_audit_rows`] does to one row: `(collection, row pairs)` to new pairs.
type RowFn<'a> = dyn FnMut(&str, &[(String, Json)]) -> Result<Pairs, String> + 'a;

/// Apply `f` to every row of every `AuditRecord` collection of `body`, as
/// `(collection, row pairs)`; other values are copied.
fn map_audit_rows(body: &Json, f: &mut RowFn<'_>) -> Result<Json, String> {
    let Json::Obj(top) = body else {
        return Err("body is not an object".into());
    };
    let mut out = Vec::with_capacity(top.len());
    for (k, v) in top {
        if k != "collections" {
            out.push((k.clone(), v.clone()));
            continue;
        }
        let Json::Obj(colls) = v else {
            return Err("collections is not an object".into());
        };
        let mut new_colls = Vec::with_capacity(colls.len());
        for (name, c) in colls {
            if c.get("type").and_then(Json::as_str) != Some(AUDIT_TYPE) {
                new_colls.push((name.clone(), c.clone()));
                continue;
            }
            let Json::Obj(cpairs) = c else {
                return Err(format!("{name} is not an object"));
            };
            let mut new_c = Vec::with_capacity(cpairs.len());
            for (ck, cv) in cpairs {
                if ck != "rows" {
                    new_c.push((ck.clone(), cv.clone()));
                    continue;
                }
                let rows = cv.as_array().ok_or(format!("{name}.rows"))?;
                let mut new_rows = Vec::with_capacity(rows.len());
                for r in rows {
                    let Json::Obj(rp) = r else {
                        return Err(format!("{name}: a row is not an object"));
                    };
                    new_rows.push(Json::Obj(f(name, rp)?));
                }
                new_c.push((ck.clone(), Json::Arr(new_rows)));
            }
            new_colls.push((name.clone(), Json::Obj(new_c)));
        }
        out.push((k.clone(), Json::Obj(new_colls)));
    }
    Ok(Json::Obj(out))
}

fn row_parts(row: &[(String, Json)]) -> Result<(String, RowMarking, Json), String> {
    let get = |k: &str| row.iter().find(|(p, _)| p == k).map(|(_, v)| v);
    let id = get("id")
        .and_then(Json::as_str)
        .ok_or("row id")?
        .to_string();
    let marking = marking_json(get("marking").ok_or("row marking")?)?;
    let item = get("item").ok_or("row item")?.clone();
    Ok((id, marking, item))
}

/// `body` with a [`ROW_SIG_FIELD`] signature, made through `signer`, added to every
/// `AuditRecord` row. Refuses a row that already carries one, and a key not in
/// [`dataset::APPROVED`].
pub fn sign_rows(body: &Json, signer: &dyn SigningBackend) -> Result<Json, String> {
    let alg = signer.verifying_key().alg();
    if !dataset::APPROVED.contains(&alg) {
        return Err(format!(
            "{} is not approved for demo audit rows",
            alg.as_str()
        ));
    }
    map_audit_rows(body, &mut |name, row| {
        if row.iter().any(|(k, _)| k == ROW_SIG_FIELD) {
            return Err(format!("{name}: a row is signed already"));
        }
        let (id, marking, item) = row_parts(row)?;
        let signature = signer
            .sign(&row_preimage(name, &id, &marking, &item))
            .map_err(|e| e.to_string())?;
        let env = SigEnvelope {
            domain: DEMO_AUDIT.label().to_vec(),
            key_id: signer.key_id().to_string(),
            signature,
        };
        let mut out = row.to_vec();
        out.push((ROW_SIG_FIELD.into(), Json::Str(hex(&env.encode()))));
        Ok(out)
    })
}

/// Check every `AuditRecord` row of `body` carries a signature that verifies under
/// `anchors`; the number of rows checked.
pub fn verify_rows(body: &Json, anchors: &TrustAnchors) -> Result<usize, String> {
    let mut n = 0;
    map_audit_rows(body, &mut |name, row| {
        let (id, marking, item) = row_parts(row)?;
        let sig = row
            .iter()
            .find(|(k, _)| k == ROW_SIG_FIELD)
            .and_then(|(_, v)| v.as_str())
            .ok_or(format!("{name}/{id}: no audit signature"))?;
        verify_row(name, &id, &marking, &item, sig, anchors)?;
        n += 1;
        Ok(row.to_vec())
    })?;
    Ok(n)
}

/// `body` without any [`ROW_SIG_FIELD`]: what the fixtures build before signing.
pub fn without_row_sigs(body: &Json) -> Json {
    map_audit_rows(body, &mut |_, row| {
        Ok(row
            .iter()
            .filter(|(k, _)| k != ROW_SIG_FIELD)
            .cloned()
            .collect())
    })
    .unwrap_or_else(|_| body.clone())
}
