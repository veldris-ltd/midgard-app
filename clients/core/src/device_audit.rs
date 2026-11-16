//! The device-audit key: what a run-time audit row's `device` status rests on.
//! `[NFR-PQ-3]` `[AC-C-13]` `[FR-C-2]` `[NFR-PQ-2]`
//!
//! An audit row the source appends for one of this device's actions arrives
//! unchecked ([`SignatureStatus::Unsigned`]). The session signs a **device
//! statement** about it with this key when the action's answer comes back, keeps the
//! signature in the persona's sealed cache, and shows the row as
//! [`SignatureStatus::Device`] only while the statement still verifies under this
//! key ([`DeviceAuditKey::verify_statement`]).
//!
//! - **What is signed.** Not the bare row: the row (collection, id, item, marking)
//!   together with what this device itself recorded when it made the action
//!   ([`DeviceStatement`]: action id, verb, target, SHA-256 of the canonical
//!   parameters) and the status the source answered. The session takes the action's
//!   fields from its own sealed record, never from the source, so a source cannot
//!   have the device sign content of its choosing ([`statement_bytes`]).
//! - **Own domain.** Signatures are made over
//!   `preimage(DEVICE_AUDIT, statement_bytes(..))`
//!   ([`attest::accept::DEVICE_AUDIT`]), so a device signature can never stand for a
//!   built-in demo-audit row ([`attest::accept::DEMO_AUDIT`]), a dataset or a release.
//!   The key signs nothing else: it has no general signing method, and the one
//!   signing method is crate-private.
//! - **Custody.** The ML-DSA-65 key is made here. At rest its secret is sealed to the
//!   device's key root ([`TrustRoot`]: the ML-KEM root that the platform key root,
//!   [`RootSealer`](crate::keystore::sealed_root::RootSealer), seals in turn) with
//!   the suite's KEM-DEM envelope ([`kms::seal_to_root`]). The secret never crosses
//!   to the platform side and no method returns it.
//! - **Honest status.** Verification takes only this key's verifying key: a
//!   signature by any other key, under any other domain, or over a changed row is
//!   refused, and then the row is `none`.
//!
//! ```text
//! magic 8 "MGDAKEY1" | alg 1 | verifying key 4+n | kms sealed blob (rest)
//!                     (AAD of the sealed secret = everything before the sealed blob)
//! ```

use std::fmt;

use attest::accept::{accept, sign_in_domain, DEVICE_AUDIT};
use attest::envelope::SigEnvelope;
use attest::hex::{hex, unhex};
use attest::json::{parse, Json};
use kms::{seal_to_root, unseal_from_root, SealedBlob};
use pqc_agility::backend::fips::provider;
use pqc_agility::{sha256, Rng, SecretBytes, SigId, SigningKey, VerifyingKey};

use crate::demo::audit::{signed_bytes, SignatureStatus};
use crate::demo::canon;
use crate::keystore::TrustRoot;
use crate::marking::{MarkingLevel, RowMarking};

/// File magic and format version of a sealed device-audit key.
pub const MAGIC: &[u8; 8] = b"MGDAKEY1";
/// The device-audit key's algorithm.
pub const ALG: SigId = SigId::MlDsa65;
/// The key id prefix; the rest is a digest of the verifying key, so it names the
/// key and nothing about the device or a persona.
pub const KEY_ID_PREFIX: &str = "device-audit-";

/// What this device recorded about one of its actions, and the status the source
/// answered: the part of a device statement that does not come from the row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceStatement {
    /// The action object id.
    pub action_id: String,
    /// `<console>.<verb>`.
    pub verb: String,
    /// `<collection>/<row id>` the action was on.
    pub target: String,
    /// Hex SHA-256 of the canonical JSON parameters.
    pub params_sha256: String,
    /// `applied`, `refused` or `conflict`.
    pub status: String,
}

/// The bytes a device signature covers, before the domain preimage: the canonical
/// JSON of `{collection, id, item, marking, statement}`, where the first four are
/// exactly an audit row's [`signed_bytes`] and `statement` is `st`.
pub fn statement_bytes(
    collection: &str,
    id: &str,
    marking: &RowMarking,
    item: &Json,
    st: &DeviceStatement,
) -> Vec<u8> {
    let row = signed_bytes(collection, id, marking, item);
    let mut pairs = match std::str::from_utf8(&row).ok().and_then(|t| parse(t).ok()) {
        Some(Json::Obj(pairs)) => pairs,
        // signed_bytes always encodes an object; keep the row's bytes regardless.
        _ => vec![(
            "row".to_string(),
            Json::Str(String::from_utf8_lossy(&row).into_owned()),
        )],
    };
    let s = |v: &str| Json::Str(v.to_string());
    pairs.push((
        "statement".to_string(),
        Json::Obj(vec![
            ("actionId".into(), s(&st.action_id)),
            ("paramsSha256".into(), s(&st.params_sha256)),
            ("status".into(), s(&st.status)),
            ("target".into(), s(&st.target)),
            ("verb".into(), s(&st.verb)),
        ]),
    ));
    canon::encode(&Json::Obj(pairs)).into_bytes()
}

/// Why a device-audit key did not open, or could not sign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceKeyError {
    /// The sealed key's structure or version.
    Malformed(String),
    /// Sealing or unsealing under the key root failed (another root, a changed blob).
    Seal(String),
    /// The signing backend refused, or the unsealed key does not match its
    /// verifying key.
    Sign(String),
}

impl fmt::Display for DeviceKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceKeyError::Malformed(d) => write!(f, "device-audit key malformed: {d}"),
            DeviceKeyError::Seal(d) => write!(f, "device-audit key did not unseal: {d}"),
            DeviceKeyError::Sign(d) => write!(f, "device-audit key: {d}"),
        }
    }
}

impl std::error::Error for DeviceKeyError {}

/// This device's audit-row signing key. Its secret is held in mlocked, zeroised
/// memory ([`SigningKey`]) while open, and sealed to the device's key root at rest.
pub struct DeviceAuditKey {
    key_id: String,
    verifying: VerifyingKey,
    signing: SigningKey,
}

impl fmt::Debug for DeviceAuditKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceAuditKey")
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

fn key_id_of(vk: &VerifyingKey) -> String {
    let d = sha256(vk.as_bytes());
    format!("{KEY_ID_PREFIX}{}", hex(&d[..16]))
}

fn header(vk: &VerifyingKey) -> Vec<u8> {
    let mut o = MAGIC.to_vec();
    o.push(vk.alg().as_u8());
    o.extend_from_slice(&(vk.as_bytes().len() as u32).to_le_bytes());
    o.extend_from_slice(vk.as_bytes());
    o
}

impl DeviceAuditKey {
    /// Make a key and seal it to `root`. Returns the key and the blob to store
    /// beside the sealed root (it holds no secret in the clear).
    pub fn create(
        root: &dyn TrustRoot,
        rng: &mut dyn Rng,
    ) -> Result<(Self, Vec<u8>), DeviceKeyError> {
        let p = provider();
        let scheme = p
            .sig_for(ALG)
            .map_err(|e| DeviceKeyError::Sign(e.to_string()))?;
        let (verifying, signing) = scheme
            .keypair(rng)
            .map_err(|e| DeviceKeyError::Sign(e.to_string()))?;
        let mut blob = header(&verifying);
        let sealed = seal_to_root(&p, root, &blob, signing.expose(), rng)
            .map_err(|e| DeviceKeyError::Seal(e.to_string()))?;
        blob.extend_from_slice(&sealed.encode());
        let key = Self {
            key_id: key_id_of(&verifying),
            verifying,
            signing,
        };
        Ok((key, blob))
    }

    /// Reopen a key [`create`](Self::create) sealed, under the same `root`. Fails on
    /// another root, any changed or missing byte, or a secret that does not sign
    /// for the stored verifying key.
    pub fn open(root: &dyn TrustRoot, blob: &[u8]) -> Result<Self, DeviceKeyError> {
        let bad = |d: &str| DeviceKeyError::Malformed(d.to_string());
        if blob.len() < MAGIC.len() + 5 {
            return Err(bad("truncated"));
        }
        if blob[..7] != MAGIC[..7] {
            return Err(bad("not a device-audit key"));
        }
        if blob[..8] != MAGIC[..] {
            return Err(bad("unsupported format version"));
        }
        let alg = SigId::from_u8(blob[8])
            .filter(|a| *a == ALG)
            .ok_or_else(|| bad("algorithm"))?;
        let n = u32::from_le_bytes(blob[9..13].try_into().expect("4 bytes")) as usize;
        let end = 13usize
            .checked_add(n)
            .filter(|e| *e <= blob.len())
            .ok_or_else(|| bad("verifying key"))?;
        let verifying = VerifyingKey::new(alg, blob[13..end].to_vec());
        let sealed = SealedBlob::decode(&blob[end..]).map_err(|e| bad(&e.to_string()))?;
        if sealed.root_key_id != root.key_id() {
            return Err(DeviceKeyError::Seal("sealed to another root".into()));
        }
        let secret = unseal_from_root(&provider(), root, &blob[..end], &sealed)
            .map_err(|e| DeviceKeyError::Seal(e.to_string()))?;
        let key = Self {
            key_id: key_id_of(&verifying),
            verifying,
            signing: SigningKey::new(alg, SecretBytes::new(secret)),
        };
        // The secret must sign for the stored verifying key.
        let probe_marking = RowMarking::new(MarkingLevel::Open, Vec::<String>::new());
        let probe = Json::Obj(Vec::new());
        let st = DeviceStatement::default();
        let sig = key.sign_statement("\0probe", "", &probe_marking, &probe, &st)?;
        key.verify_statement("\0probe", "", &probe_marking, &probe, &st, &sig)
            .map_err(|_| DeviceKeyError::Sign("the secret does not match the key".into()))?;
        Ok(key)
    }

    /// The key id carried in every signature: a digest of the verifying key.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// The verifying key.
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying
    }

    /// Sign a device statement about one audit row (as delivered, `pqSignature`
    /// included) under the device-audit domain. Returns the hex of an attest
    /// `MGSIG1` envelope. Crate-private: only the session signs, and only with the
    /// fields of an action it recorded itself.
    pub(crate) fn sign_statement(
        &self,
        collection: &str,
        id: &str,
        marking: &RowMarking,
        item: &Json,
        st: &DeviceStatement,
    ) -> Result<String, DeviceKeyError> {
        let signature = sign_in_domain(
            &DEVICE_AUDIT,
            &statement_bytes(collection, id, marking, item, st),
            &self.signing,
        )
        .map_err(|e| DeviceKeyError::Sign(e.to_string()))?;
        let env = SigEnvelope {
            domain: DEVICE_AUDIT.label().to_vec(),
            key_id: self.key_id.clone(),
            signature,
        };
        Ok(hex(&env.encode()))
    }

    /// [`sign_statement`](Self::sign_statement) for this crate's own tests only
    /// (feature `demo-unpinned`, which a release build graph never selects).
    #[cfg(any(test, feature = "demo-unpinned"))]
    pub fn sign_statement_for_tests(
        &self,
        collection: &str,
        id: &str,
        marking: &RowMarking,
        item: &Json,
        st: &DeviceStatement,
    ) -> Result<String, DeviceKeyError> {
        self.sign_statement(collection, id, marking, item, st)
    }

    /// Check a device statement's signature under this key. `Ok` is the only way to
    /// [`SignatureStatus::Device`]; every finding (not an envelope, another domain,
    /// another key, a changed row, place, id, marking or statement field) is an
    /// `Err`.
    pub fn verify_statement(
        &self,
        collection: &str,
        id: &str,
        marking: &RowMarking,
        item: &Json,
        st: &DeviceStatement,
        sig_hex: &str,
    ) -> Result<SignatureStatus, String> {
        let bytes = unhex(sig_hex).ok_or("device signature is not hex")?;
        let env = SigEnvelope::parse(&bytes).map_err(|e| format!("device signature: {e}"))?;
        if env.domain != DEVICE_AUDIT.label() {
            return Err("device signature is not under the device-audit domain".into());
        }
        if env.key_id != self.key_id {
            return Err("device signature is by another key".into());
        }
        accept(
            &DEVICE_AUDIT,
            &statement_bytes(collection, id, marking, item, st),
            &self.verifying,
            &env.signature,
            &[ALG],
        )
        .map_err(|e| format!("device signature refused: {e}"))?;
        Ok(SignatureStatus::Device)
    }
}
