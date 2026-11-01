//! **The** acceptance decision.
//!
//! Every place in the suite that asks "is this signed thing acceptable?" —  the
//! CI release gate, Bifrost's artefact ingest and pre-deploy re-check, Bifrost's
//! offline-bundle import, Mimir's verify-on-load — calls [`accept`]. A second
//! implementation would be a second place for the answer to be wrong, and the
//! answer is "no" far more often than the happy path suggests.
//! `[AC-B-1]` `[AC-B-3]` `[AC-MS-2]` `[AC-MI-7]` `[NFR-PQ-3]`
//!
//! ## Domain separation
//! Each construct signs under its own [`Domain`]. The preimage is
//!
//! ```text
//! len(label) as u8 ‖ label ‖ body
//! ```
//!
//! The length prefix means no two domains can ever produce the same preimage,
//! even when one label is a prefix of another. Without it, a signature over a
//! Bifrost artefact manifest could in principle be presented as a signature over
//! a Mimir model manifest — both are length-prefixed strings followed by digests,
//! so a contrived pair could collide. With it, that is impossible by
//! construction rather than by inspection.
//!
//! ## What this does not decide
//! Only cryptographic acceptance: approved algorithm, key agreement, signature
//! validity. Licence policy, revocation, hardware eligibility, freshness and
//! lifecycle state are the caller's gates, applied *after* this one passes —
//! nothing is trusted enough to evaluate until the signature holds.

use core::fmt;

use pqc_agility::{SigId, Signature, VerifyingKey};

/// A signing context. Two constructs must never share one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Domain {
    label: &'static [u8],
}

impl Domain {
    /// Define a domain. The label must be at most 255 bytes so it can carry a
    /// one-byte length prefix; this is checked at construction in debug builds
    /// and saturates rather than truncating silently in release.
    pub const fn new(label: &'static [u8]) -> Self {
        assert!(
            label.len() <= u8::MAX as usize,
            "domain label must fit in a u8 length prefix"
        );
        Self { label }
    }

    /// The raw label, as written into a signature envelope.
    pub const fn label(&self) -> &'static [u8] {
        self.label
    }
}

/// The CI release bundle: an artefact with its SBOM and provenance.
pub const RELEASE_ARTEFACT: Domain = Domain::new(b"midgard-artefact-v1");
/// A Bifrost artefact manifest.
pub const BIFROST_ARTEFACT: Domain = Domain::new(b"midgard-bifrost-artefact-v1");
/// A Bifrost offline-bundle manifest.
pub const BIFROST_BUNDLE: Domain = Domain::new(b"midgard-bifrost-bundle-v1");
/// A Mimir model manifest.
pub const MIMIR_MODEL: Domain = Domain::new(b"midgard-mimir-model-v1");
/// A Mimir model manifest **file**: the signature covers the file's bytes exactly as
/// they lie on disk, so it is checked before the file is parsed. [MG-SEC-14]
pub const MIMIR_MODEL_FILE: Domain = Domain::new(b"midgard-mimir-model-file-v2");

/// Build the canonical preimage for `body` in `domain`.
pub fn preimage(domain: &Domain, body: &[u8]) -> Vec<u8> {
    let label = domain.label();
    let mut o = Vec::with_capacity(1 + label.len() + body.len());
    o.push(label.len() as u8);
    o.extend_from_slice(label);
    o.extend_from_slice(body);
    o
}

/// Why acceptance was refused. Every variant is a rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AcceptError {
    /// The signature's algorithm is not approved for this construct.
    UnapprovedAlgorithm {
        /// What the signature declares.
        got: &'static str,
    },
    /// The trust anchor and the signature disagree on the algorithm.
    AlgorithmMismatch {
        /// The anchor's algorithm.
        anchor: &'static str,
        /// The signature's algorithm.
        signature: &'static str,
    },
    /// The algorithm is approved but no backend can verify it here. Fail closed
    /// rather than accepting something we cannot check. `[AC-M-1]`
    BackendUnavailable {
        /// The algorithm.
        alg: &'static str,
    },
    /// The signature did not verify over the recomputed preimage.
    BadSignature,
}

impl fmt::Display for AcceptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AcceptError::UnapprovedAlgorithm { got } => {
                write!(f, "{got} is not an approved algorithm for this artefact")
            }
            AcceptError::AlgorithmMismatch { anchor, signature } => write!(
                f,
                "trust anchor is {anchor} but the signature is {signature}"
            ),
            AcceptError::BackendUnavailable { alg } => {
                write!(f, "no backend available to verify {alg}")
            }
            AcceptError::BadSignature => write!(f, "signature does not verify"),
        }
    }
}

impl std::error::Error for AcceptError {}

/// Accept, or refuse, a signature over `body` in `domain`.
///
/// Order matters: the cheap structural checks run before the expensive
/// cryptographic one, and every one of them is a refusal. There is no argument
/// combination that returns `Ok` without a verified signature.
pub fn accept(
    domain: &Domain,
    body: &[u8],
    anchor: &VerifyingKey,
    signature: &Signature,
    approved: &[SigId],
) -> Result<(), AcceptError> {
    let alg = signature.alg();
    if !approved.contains(&alg) {
        return Err(AcceptError::UnapprovedAlgorithm { got: alg.as_str() });
    }
    if anchor.alg() != alg {
        return Err(AcceptError::AlgorithmMismatch {
            anchor: anchor.alg().as_str(),
            signature: alg.as_str(),
        });
    }

    let provider = crate::provider();
    let scheme = provider
        .sig_for(alg)
        .map_err(|_| AcceptError::BackendUnavailable { alg: alg.as_str() })?;

    scheme
        .verify(anchor, &preimage(domain, body), signature)
        .map_err(|_| AcceptError::BadSignature)
}

/// Sign `body` in `domain`. The counterpart to [`accept`], so a caller cannot
/// accidentally sign a preimage that differs from the one verification builds.
///
/// Callers holding a key behind a custodian should sign
/// [`preimage(domain, body)`](preimage) through that custodian instead; this is
/// for the in-process case (tests, and the subsystem signers that hold their own
/// key).
pub fn sign_in_domain(
    domain: &Domain,
    body: &[u8],
    signing_key: &pqc_agility::SigningKey,
) -> Result<Signature, AcceptError> {
    let alg = signing_key.alg();
    let provider = crate::provider();
    let scheme = provider
        .sig_for(alg)
        .map_err(|_| AcceptError::BackendUnavailable { alg: alg.as_str() })?;
    scheme
        .sign(signing_key, &preimage(domain, body))
        .map_err(|_| AcceptError::BadSignature)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keypair(alg: SigId) -> (VerifyingKey, pqc_agility::SigningKey) {
        let mut rng = pqc_agility::OsRng;
        crate::provider()
            .sig_for(alg)
            .expect("registered")
            .keypair(&mut rng)
            .expect("keypair")
    }

    #[test]
    fn accepts_a_signature_it_produced() {
        let (vk, sk) = keypair(SigId::MlDsa87);
        let sig = sign_in_domain(&RELEASE_ARTEFACT, b"body", &sk).expect("sign");
        accept(&RELEASE_ARTEFACT, b"body", &vk, &sig, &[SigId::MlDsa87]).expect("accepts");
    }

    #[test]
    fn refuses_a_signature_from_another_domain() {
        // The whole point of domain separation: the same body signed for one
        // construct must not be acceptable as another.
        let (vk, sk) = keypair(SigId::MlDsa65);
        let sig = sign_in_domain(&BIFROST_ARTEFACT, b"body", &sk).expect("sign");
        assert_eq!(
            accept(&MIMIR_MODEL, b"body", &vk, &sig, &[SigId::MlDsa65]),
            Err(AcceptError::BadSignature)
        );
    }

    #[test]
    fn refuses_a_modified_body() {
        let (vk, sk) = keypair(SigId::MlDsa65);
        let sig = sign_in_domain(&BIFROST_ARTEFACT, b"body", &sk).expect("sign");
        assert_eq!(
            accept(&BIFROST_ARTEFACT, b"bodz", &vk, &sig, &[SigId::MlDsa65]),
            Err(AcceptError::BadSignature)
        );
    }

    #[test]
    fn refuses_an_unapproved_algorithm() {
        let (vk, sk) = keypair(SigId::MlDsa44);
        let sig = sign_in_domain(&RELEASE_ARTEFACT, b"body", &sk).expect("sign");
        assert!(matches!(
            accept(&RELEASE_ARTEFACT, b"body", &vk, &sig, &[SigId::MlDsa87]),
            Err(AcceptError::UnapprovedAlgorithm { .. })
        ));
    }

    #[test]
    fn refuses_when_anchor_and_signature_disagree_on_algorithm() {
        let (vk87, _) = keypair(SigId::MlDsa87);
        let (_, sk65) = keypair(SigId::MlDsa65);
        let sig = sign_in_domain(&RELEASE_ARTEFACT, b"body", &sk65).expect("sign");
        assert!(matches!(
            accept(
                &RELEASE_ARTEFACT,
                b"body",
                &vk87,
                &sig,
                &[SigId::MlDsa87, SigId::MlDsa65]
            ),
            Err(AcceptError::AlgorithmMismatch { .. })
        ));
    }

    #[test]
    fn refuses_a_valid_signature_from_the_wrong_key() {
        let (_, sk) = keypair(SigId::MlDsa87);
        let (other_vk, _) = keypair(SigId::MlDsa87);
        let sig = sign_in_domain(&RELEASE_ARTEFACT, b"body", &sk).expect("sign");
        assert_eq!(
            accept(
                &RELEASE_ARTEFACT,
                b"body",
                &other_vk,
                &sig,
                &[SigId::MlDsa87]
            ),
            Err(AcceptError::BadSignature)
        );
    }

    #[test]
    fn domains_cannot_collide_even_when_one_label_prefixes_another() {
        const SHORT: Domain = Domain::new(b"midgard-x");
        const LONG: Domain = Domain::new(b"midgard-xy");
        // Without the length prefix, SHORT ‖ "y…" and LONG ‖ "…" would be equal.
        assert_ne!(preimage(&SHORT, b"yBODY"), preimage(&LONG, b"BODY"));
    }

    #[test]
    fn preimage_is_length_prefixed_and_deterministic() {
        let p = preimage(&RELEASE_ARTEFACT, b"BODY");
        assert_eq!(p[0] as usize, RELEASE_ARTEFACT.label().len());
        assert_eq!(&p[1..1 + p[0] as usize], RELEASE_ARTEFACT.label());
        assert_eq!(p, preimage(&RELEASE_ARTEFACT, b"BODY"));
    }

    #[test]
    fn every_declared_domain_is_distinct() {
        let all = [
            RELEASE_ARTEFACT,
            BIFROST_ARTEFACT,
            BIFROST_BUNDLE,
            MIMIR_MODEL,
            MIMIR_MODEL_FILE,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.label(), b.label(), "domains must be unique");
                assert_ne!(preimage(a, b"same"), preimage(b, b"same"));
            }
        }
    }
}
