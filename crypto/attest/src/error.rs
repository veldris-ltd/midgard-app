//! Errors. Every variant names one rejection reason, because "signing failed"
//! and "verification failed" are useless to the operator holding a red release
//! gate at 02:00. There is deliberately no variant meaning "probably fine".

use std::fmt;
use std::path::PathBuf;

/// Why a signing run refused.
#[derive(Debug)]
#[non_exhaustive]
pub enum SignError {
    /// The artefact does not exist or could not be read.
    ArtefactUnreadable { path: PathBuf, reason: String },
    /// The sibling CycloneDX SBOM is absent. Never sign an artefact whose bill
    /// of materials is unknown.
    SbomMissing { expected: PathBuf },
    /// The SBOM exists but is not a usable CycloneDX document.
    SbomInvalid { path: PathBuf, reason: String },
    /// The signing custodian refused, was unreachable, or the algorithm is
    /// unavailable.
    Custodian(String),
    /// A local development key file was requested without the explicit
    /// environment opt-in.
    FileSigningNotPermitted,
    /// A key file could not be read or parsed.
    KeyUnreadable { path: PathBuf, reason: String },
    /// Writing an output file failed.
    Write { path: PathBuf, reason: String },
}

impl fmt::Display for SignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignError::ArtefactUnreadable { path, reason } => {
                write!(f, "cannot read artefact {}: {reason}", path.display())
            }
            SignError::SbomMissing { expected } => write!(
                f,
                "no SBOM at {} — refusing to sign an artefact whose bill of materials is unknown",
                expected.display()
            ),
            SignError::SbomInvalid { path, reason } => {
                write!(f, "invalid CycloneDX SBOM {}: {reason}", path.display())
            }
            SignError::Custodian(s) => write!(f, "signing custodian: {s}"),
            SignError::FileSigningNotPermitted => write!(
                f,
                "--key-file requires MIDGARD_ALLOW_FILE_SIGNING_KEY=1 (local development only)"
            ),
            SignError::KeyUnreadable { path, reason } => {
                write!(f, "cannot read signing key {}: {reason}", path.display())
            }
            SignError::Write { path, reason } => {
                write!(f, "cannot write {}: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for SignError {}

/// Why a verification refused. Every variant is a rejection; there is no
/// "warning" tier, because a release gate that warns is a release gate that
/// passes. `[AC-B-1]`
#[derive(Debug)]
#[non_exhaustive]
pub enum VerifyError {
    /// The artefact does not exist or could not be read.
    ArtefactUnreadable { path: PathBuf, reason: String },
    /// A required sidecar (`.sig`, `.sbom.cdx.json`, `.att`) is absent.
    SidecarMissing { expected: PathBuf },
    /// The `.sig` envelope is malformed.
    MalformedEnvelope(String),
    /// The SBOM is not a usable CycloneDX document.
    SbomInvalid { path: PathBuf, reason: String },
    /// The envelope's domain label is not the one this verifier accepts — the
    /// signature was made for a different protocol.
    WrongDomain { got: String },
    /// The signing key id is not in the trust anchor set. The signature may be
    /// cryptographically valid and is still refused.
    UntrustedKey { key_id: String },
    /// The anchor for this key id is for a different algorithm than the
    /// envelope declares.
    AlgorithmMismatch { anchor: String, envelope: String },
    /// The algorithm is not one this verifier accepts for release signing.
    UnapprovedAlgorithm { alg: String },
    /// The algorithm is approved but no backend here can check it. Fail closed
    /// rather than accepting something unverifiable. `[AC-M-1]`
    BackendUnavailable { alg: String },
    /// The signature did not verify over the recomputed preimage. This is the
    /// single outcome for every tamper: artefact, SBOM, or attestation.
    BadSignature,
    /// No trust anchors were supplied. Absence of anchors is a refusal, never a
    /// bypass. `[AC-RF-SC-8a]`
    NoTrustAnchors,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::ArtefactUnreadable { path, reason } => {
                write!(f, "cannot read artefact {}: {reason}", path.display())
            }
            VerifyError::SidecarMissing { expected } => {
                write!(f, "missing required sidecar {}", expected.display())
            }
            VerifyError::MalformedEnvelope(s) => write!(f, "malformed signature envelope: {s}"),
            VerifyError::SbomInvalid { path, reason } => {
                write!(f, "invalid CycloneDX SBOM {}: {reason}", path.display())
            }
            VerifyError::WrongDomain { got } => write!(
                f,
                "signature domain {got:?} is not the artefact domain — cross-protocol signature"
            ),
            VerifyError::UntrustedKey { key_id } => {
                write!(f, "signing key {key_id:?} is not a trusted release key")
            }
            VerifyError::AlgorithmMismatch { anchor, envelope } => write!(
                f,
                "trust anchor is {anchor} but the envelope declares {envelope}"
            ),
            VerifyError::UnapprovedAlgorithm { alg } => {
                write!(f, "{alg} is not an approved release-signing algorithm")
            }
            VerifyError::BackendUnavailable { alg } => {
                write!(f, "no backend available to verify {alg}")
            }
            VerifyError::BadSignature => write!(
                f,
                "signature does not verify — the artefact, its SBOM, or its attestation was altered"
            ),
            VerifyError::NoTrustAnchors => {
                write!(f, "no trust anchors supplied — refusing to verify")
            }
        }
    }
}

impl std::error::Error for VerifyError {}

/// Lift the shared acceptance decision into this crate's vocabulary. The mapping
/// is total and loses nothing: every way to be refused stays a distinct refusal.
impl From<crate::accept::AcceptError> for VerifyError {
    fn from(e: crate::accept::AcceptError) -> Self {
        use crate::accept::AcceptError as A;
        match e {
            A::UnapprovedAlgorithm { got } => VerifyError::UnapprovedAlgorithm {
                alg: got.to_string(),
            },
            A::AlgorithmMismatch { anchor, signature } => VerifyError::AlgorithmMismatch {
                anchor: anchor.to_string(),
                envelope: signature.to_string(),
            },
            A::BackendUnavailable { alg } => VerifyError::BackendUnavailable {
                alg: alg.to_string(),
            },
            A::BadSignature => VerifyError::BadSignature,
        }
    }
}
