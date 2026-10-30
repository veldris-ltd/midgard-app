//! # attest — the release-artefact attestation format
//!
//! One implementation of *sign* and one of **reject-unless-proven**, shared by
//! every place the suite makes that decision: the CI release gate (`mg-sign`,
//! `mg-verify`), Bifrost's pre-deploy check (`AC-B-1`), and Mimir's model-load
//! check (`AC-MS-2`). A second implementation of "is this artefact acceptable"
//! is a second place for the answer to be wrong, so there is only one.
//! `[NFR-PQ-3]` `[NFR-OSS-1]` `[AC-B-1]` `[AC-B-4]`
//!
//! ## The bundle
//! A signed release artefact is four files:
//!
//! | File | What it is |
//! |---|---|
//! | `<artefact>` | the deployable bytes |
//! | `<artefact>.sbom.cdx.json` | its CycloneDX bill of materials |
//! | `<artefact>.att` | in-toto provenance, byte-reproducible |
//! | `<artefact>.sig` | the detached ML-DSA signature over all three |
//!
//! All three digests are bound into one signing preimage
//! ([`preimage`]), so any tamper — to the binary, to its bill of materials, or to
//! its provenance — is the same single signature failure. There is no way to swap
//! content under a valid signature, and the acceptance decision never depends on
//! parsing the attestation. The SBOM *is* parsed — strictly, by [`json`] — because
//! "is this a bill of materials" is a structural question a digest cannot answer.
//!
//! ## Fail closed
//! [`sign_artefact`] writes nothing until every step has succeeded — including
//! verifying its own signature under the release rules — so a failed run leaves
//! no `.sig` behind for a later step to mistake for success, and a successful one
//! never leaves a `.sig` that [`verify_artefact`] would refuse.
//! [`verify_artefact`] returns `Result<Verdict, VerifyError>` — never a boolean,
//! never a warning tier — and refuses when trust anchors are absent, when the key
//! is not among them, when the algorithm is not approved for release signing, and
//! when the domain label is not this protocol's.
//!
//! ## No ambient authority
//! Nothing here looks for a key, a trust anchor, or a configuration file on its
//! own. Callers name every input.

#![forbid(unsafe_code)]

pub mod accept;
pub mod attestation;
pub mod backend;
pub mod envelope;
pub mod error;
pub mod hex;
pub mod json;
#[cfg(feature = "custody")]
pub mod keystore;
pub mod preimage;
pub mod release_floor;
pub mod sbom;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pqc_agility::hash::sha256;
use pqc_agility::{CryptoProvider, SigId, VerifyingKey};

pub use accept::{accept, AcceptError, Domain};
pub use attestation::{Provenance, ReproEntry};
pub use backend::SigningBackend;
pub use error::{SignError, VerifyError};
pub use preimage::DOMAIN_LABEL;
pub use release_floor::{Admission, ReleaseFloor, ReleaseVersion, RollbackError};

#[cfg(feature = "custody")]
pub use backend::{FileSigner, KmsSigner};
#[cfg(feature = "custody")]
pub use keystore::{DevKeyFile, Keystore};

/// The crypto provider this crate verifies and signs with.
///
/// One place, so a caller cannot accidentally verify against a provider with a
/// different set of algorithms enabled than the one that signed.
pub(crate) fn provider() -> CryptoProvider {
    pqc_agility::backend::fips::provider()
}

/// Signature algorithms accepted for release artefacts.
///
/// ML-DSA-87 is the default and ML-DSA-65 is accepted so that artefacts signed
/// under the suite's per-artefact algorithm (`bifrost_artefact::BIFROST_SIG_ALG`)
/// remain verifiable. Anything else — including SLH-DSA, which is reserved for
/// the offline root in RF-SC-8 — is refused for this purpose.
pub const APPROVED_RELEASE_ALGS: [SigId; 2] = [SigId::MlDsa87, SigId::MlDsa65];

/// Sidecar suffixes.
pub const SIG_SUFFIX: &str = ".sig";
/// Attestation sidecar suffix.
pub const ATT_SUFFIX: &str = ".att";

fn sidecar(artefact: &Path, suffix: &str) -> PathBuf {
    let mut s = artefact.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// Path of the `.sig` beside an artefact.
pub fn sig_path(artefact: &Path) -> PathBuf {
    sidecar(artefact, SIG_SUFFIX)
}

/// Path of the `.att` beside an artefact.
pub fn att_path(artefact: &Path) -> PathBuf {
    sidecar(artefact, ATT_SUFFIX)
}

/// What a successful signing run produced.
#[derive(Debug, Clone)]
pub struct SignOutputs {
    /// Where the signature was written.
    pub sig_path: PathBuf,
    /// Where the attestation was written.
    pub att_path: PathBuf,
    /// The artefact digest that was signed.
    pub artefact_sha256: [u8; 32],
    /// The SBOM digest that was signed.
    pub sbom_sha256: [u8; 32],
    /// The attestation digest that was signed.
    pub attestation_sha256: [u8; 32],
    /// Components listed in the SBOM.
    pub components: usize,
    /// The signing key id.
    pub key_id: String,
    /// The signing algorithm.
    pub alg: SigId,
}

/// Sign `artefact`, binding its SBOM and its provenance.
///
/// `sbom_override` lets a caller point at an SBOM that is not the sibling file;
/// `None` uses `<artefact>.sbom.cdx.json`. Passing `Some` does not relax
/// anything — the named SBOM is validated and bound exactly the same way.
pub fn sign_artefact(
    artefact: &Path,
    sbom_override: Option<&Path>,
    signer: &dyn SigningBackend,
    prov: &Provenance,
) -> Result<SignOutputs, SignError> {
    // 0. The key must be one a verifier will accept. Signing under any other
    //    algorithm would exit successfully and produce a bundle that can never
    //    pass the gate.
    let alg = signer.verifying_key().alg();
    if !APPROVED_RELEASE_ALGS.contains(&alg) {
        return Err(SignError::UnapprovedAlgorithm {
            alg: alg.as_str().to_string(),
        });
    }

    // 1. The artefact itself.
    let bytes = std::fs::read(artefact).map_err(|e| SignError::ArtefactUnreadable {
        path: artefact.to_path_buf(),
        reason: e.to_string(),
    })?;
    let artefact_sha256 = sha256(&bytes);

    // 2. Its bill of materials. Absent or unusable ⇒ refuse; an artefact whose
    //    dependency graph is unknown must never carry a signature. [AC-B-4]
    let sbom = match sbom_override {
        Some(p) => {
            let b = std::fs::read(p).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    SignError::SbomMissing {
                        expected: p.to_path_buf(),
                    }
                } else {
                    SignError::SbomInvalid {
                        path: p.to_path_buf(),
                        reason: e.to_string(),
                    }
                }
            })?;
            let components = sbom::validate(&b).map_err(|reason| SignError::SbomInvalid {
                path: p.to_path_buf(),
                reason,
            })?;
            sbom::Sbom {
                path: p.to_path_buf(),
                bytes: b,
                components,
            }
        }
        None => sbom::load(artefact).map_err(|e| match e {
            sbom::LoadError::Missing(expected) => SignError::SbomMissing { expected },
            sbom::LoadError::Invalid(path, reason) => SignError::SbomInvalid { path, reason },
        })?,
    };
    let sbom_sha256 = sha256(&sbom.bytes);

    // 3. Provenance. Rendered before signing because its digest is signed.
    let name = artefact
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| artefact.to_string_lossy().into_owned());
    let att = attestation::render(
        &attestation::Subject {
            name: &name,
            artefact_sha256: &artefact_sha256,
            sbom_sha256: &sbom_sha256,
            algorithm: alg.as_str(),
            key_id: signer.key_id(),
        },
        prov,
    );
    let attestation_sha256 = sha256(att.as_bytes());

    // 4. Sign the one preimage that covers all three.
    let signature = signer.sign(&preimage::preimage(
        &artefact_sha256,
        &sbom_sha256,
        &attestation_sha256,
    ))?;

    //    Then check it exactly as a verifier will, against the key this signer
    //    advertises. That catches a custodian whose signing key does not match
    //    its published verifying key — which would otherwise sign "successfully"
    //    under an anchor nobody can verify against — and a faulted signature,
    //    before either reaches disk.
    accept::accept(
        &accept::RELEASE_ARTEFACT,
        &preimage::body(&artefact_sha256, &sbom_sha256, &attestation_sha256),
        signer.verifying_key(),
        &signature,
        &APPROVED_RELEASE_ALGS,
    )
    .map_err(|e| SignError::SelfVerifyFailed(e.to_string()))?;

    // 5. Only now write anything. Everything above can fail without leaving a
    //    partial bundle a later gate stage might treat as success. [AC-RF-SC-1c]
    let env = envelope::SigEnvelope {
        domain: DOMAIN_LABEL.to_vec(),
        key_id: signer.key_id().to_string(),
        signature,
    };
    let sig_path = sig_path(artefact);
    let att_path = att_path(artefact);
    std::fs::write(&att_path, att.as_bytes()).map_err(|e| SignError::Write {
        path: att_path.clone(),
        reason: e.to_string(),
    })?;
    std::fs::write(&sig_path, env.encode()).map_err(|e| SignError::Write {
        path: sig_path.clone(),
        reason: e.to_string(),
    })?;

    Ok(SignOutputs {
        sig_path,
        att_path,
        artefact_sha256,
        sbom_sha256,
        attestation_sha256,
        components: sbom.components,
        key_id: env.key_id,
        alg,
    })
}

/// The release keys a verifier will accept.
///
/// There is no constructor that reads a default location: anchors are supplied
/// explicitly, and an empty set is a refusal rather than a bypass. Distribution,
/// rotation and revocation of anchors are RF-SC-8.
#[derive(Debug, Default, Clone)]
pub struct TrustAnchors {
    keys: BTreeMap<String, VerifyingKey>,
}

impl TrustAnchors {
    /// An empty set. Verifying against it always refuses.
    pub fn new() -> Self {
        Self::default()
    }

    /// Trust `key_id` with this verifying key.
    pub fn insert(&mut self, key_id: impl Into<String>, key: VerifyingKey) -> &mut Self {
        self.keys.insert(key_id.into(), key);
        self
    }

    /// Parse anchors from the `<alg> <key-id> <hex>` lines
    /// [`Keystore::export_verifying_key`] emits. Blank lines and `#` comments are
    /// ignored; a malformed line is an error, never a silently skipped anchor.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut out = Self::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 3 {
                return Err(format!("line {}: expected `<alg> <key-id> <hex>`", n + 1));
            }
            let alg = APPROVED_RELEASE_ALGS
                .iter()
                .copied()
                .find(|a| a.as_str() == parts[0])
                .ok_or_else(|| {
                    format!(
                        "line {}: {} is not an approved release algorithm",
                        n + 1,
                        parts[0]
                    )
                })?;
            let bytes = hex::unhex(parts[2])
                .ok_or_else(|| format!("line {}: key is not valid hex", n + 1))?;
            if out.get(parts[1]).is_some() {
                return Err(format!(
                    "line {}: key id {:?} is listed more than once",
                    n + 1,
                    parts[1]
                ));
            }
            out.insert(parts[1], VerifyingKey::new(alg, bytes));
        }
        Ok(out)
    }

    /// Fold another anchor set into this one.
    ///
    /// The same key id carrying the same key is harmless and is accepted. The
    /// same key id carrying a *different* key is an error: which one a verifier
    /// trusts must not depend on the order its anchor files were named in.
    pub fn merge(&mut self, other: TrustAnchors) -> Result<&mut Self, String> {
        for (id, key) in other.keys {
            match self.keys.get(&id) {
                Some(have) if have.alg() == key.alg() && have.as_bytes() == key.as_bytes() => {}
                Some(_) => return Err(format!("key id {id:?} is anchored to two different keys")),
                None => {
                    self.keys.insert(id, key);
                }
            }
        }
        Ok(self)
    }

    /// Whether any anchor is present.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// How many anchors are present.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Look up an anchor.
    pub fn get(&self, key_id: &str) -> Option<&VerifyingKey> {
        self.keys.get(key_id)
    }

    /// Every anchor, by key id, in key-id order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &VerifyingKey)> {
        self.keys.iter().map(|(id, key)| (id.as_str(), key))
    }
}

/// What a passing verification established.
#[derive(Debug, Clone)]
pub struct Verdict {
    /// Artefact file name.
    pub artefact: String,
    /// Its digest.
    pub artefact_sha256: [u8; 32],
    /// The key that signed it.
    pub key_id: String,
    /// The algorithm used.
    pub alg: SigId,
    /// Components in the bound SBOM.
    pub components: usize,
}

/// Encode a free-text verdict field so it cannot contain a space or an `=`.
///
/// The artefact name is a file name and the key id is operator-chosen; either
/// could otherwise carry ` key=something` and plant a second field in the line a
/// script parses. Anything outside `[A-Za-z0-9._-]` is percent-encoded.
fn verdict_field(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

impl Verdict {
    /// The one-line machine-readable form the CLI prints on stdout. Each field
    /// appears exactly once and holds no whitespace: `artefact` and `key` are
    /// percent-encoded outside `[A-Za-z0-9._-]`.
    pub fn line(&self) -> String {
        format!(
            "ok artefact={} digest={} key={} alg={} components={}",
            verdict_field(&self.artefact),
            hex::hex(&self.artefact_sha256),
            verdict_field(&self.key_id),
            self.alg.as_str(),
            self.components
        )
    }
}

/// Verify an artefact and its three sidecars. Fails closed on every path.
///
/// This is the file-level verifier: `mg-verify` calls it, in the CI release gate
/// and in every image's entrypoint. Bifrost and Mimir do **not** call it — they
/// verify their own in-memory manifests — but the signature decision inside it is
/// [`accept::accept`], which is the one they call too, so "acceptable" is decided
/// in one place. `[AC-B-1]` `[AC-MS-2]` `[AC-RF-SC-2d]`
pub fn verify_artefact(artefact: &Path, anchors: &TrustAnchors) -> Result<Verdict, VerifyError> {
    if anchors.is_empty() {
        return Err(VerifyError::NoTrustAnchors);
    }

    let bytes = std::fs::read(artefact).map_err(|e| VerifyError::ArtefactUnreadable {
        path: artefact.to_path_buf(),
        reason: e.to_string(),
    })?;
    let artefact_sha256 = sha256(&bytes);

    let read_sidecar = |p: PathBuf| -> Result<Vec<u8>, VerifyError> {
        std::fs::read(&p).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                VerifyError::SidecarMissing { expected: p }
            } else {
                VerifyError::ArtefactUnreadable {
                    path: p,
                    reason: e.to_string(),
                }
            }
        })
    };

    let sbom_bytes = read_sidecar(sbom::sbom_path(artefact))?;
    let att_bytes = read_sidecar(att_path(artefact))?;
    let sig_bytes = read_sidecar(sig_path(artefact))?;

    let env = envelope::SigEnvelope::parse(&sig_bytes)?;

    if env.domain != DOMAIN_LABEL {
        return Err(VerifyError::WrongDomain {
            got: String::from_utf8_lossy(&env.domain).into_owned(),
        });
    }
    let alg = env.signature.alg();
    let anchor = anchors
        .get(&env.key_id)
        .ok_or_else(|| VerifyError::UntrustedKey {
            key_id: env.key_id.clone(),
        })?;

    // Recompute the body from what is actually on disk and hand the decision to
    // the one acceptor. Every tamper — the binary, the SBOM, or the attestation —
    // lands here as one failure.
    let body = preimage::body(&artefact_sha256, &sha256(&sbom_bytes), &sha256(&att_bytes));
    accept::accept(
        &accept::RELEASE_ARTEFACT,
        &body,
        anchor,
        &env.signature,
        &APPROVED_RELEASE_ALGS,
    )?;

    // Only now, with its bytes authenticated, is the SBOM parsed. Nothing is
    // trusted enough to look inside until the signature holds, so a tampered
    // SBOM is always the signature's failure and never the parser's input.
    let components = sbom::validate(&sbom_bytes).map_err(|reason| VerifyError::SbomInvalid {
        path: sbom::sbom_path(artefact),
        reason,
    })?;

    Ok(Verdict {
        artefact: artefact
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| artefact.to_string_lossy().into_owned()),
        artefact_sha256,
        key_id: env.key_id,
        alg,
        components,
    })
}
