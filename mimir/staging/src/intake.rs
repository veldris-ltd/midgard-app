//! The staging-enclave intake pipeline — the AC-MS-1 orchestration. [FR-MS-1]
//! [FR-MS-2] [FR-MS-6] [NFR-PQ-3]
//!
//! Intake is deliberately split into two phases so that the human licence/risk
//! gate sits *between* advice and commitment:
//!
//! 1. [`prepare`] — fetch (staging only), verify native provenance, generate the
//!    CycloneDX SBOM, and run the **advisory** A.9 assessment. It commits to
//!    nothing and signs nothing. It **fails closed** on a provenance *mismatch*
//!    (bytes ≠ upstream-published digest ⇒ possible tampering ⇒ never re-sign);
//!    it does *not* fail on merely-unverifiable provenance — that is recorded for
//!    explicit human risk-acceptance.
//! 2. [`finalize`] — apply the [`HumanLicenceGate`] (the **only** approval in the
//!    whole pipeline — the A.9 assessor never approves), capture the licence the
//!    human accepted, build the manifest over the verified weights digest, and
//!    re-sign the bundle under Veldris's ML-DSA key (the custody transfer).
//!
//! The output [`StagedBundle`] is the staging-side `.mbundle` (manifest + Veldris
//! signature + SBOM + provenance) that Bifrost carries across the air gap.

use model_server::{Licence, ModelManifest, SignedArtifact};

use crate::assessor::{IntakeAssessment, ModelIntakeAssessor};
use crate::fetch::{FetchError, FetchedModel, Fetcher, UpstreamSource};
use crate::provenance::{verify_provenance, ProvenanceRecord};
use crate::sbom::generate_sbom;

/// Why intake failed — every arm is fail-closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeError {
    /// The upstream fetch failed.
    Fetch(FetchError),
    /// The weights did not match the upstream-published digest — possible
    /// tampering; the bundle is never re-signed. [FR-MS-2]
    ProvenanceMismatch(String),
    /// The human reviewer did not accept the licence/risk. Advisory assessment
    /// never approves; only the human gate does, and it said no. [FR-MS-6]
    LicenceRejectedByHuman { reviewer: String },
}

/// Everything the human reviewer needs, produced before any commitment. The
/// assessment here is strictly advisory input to the gate.
#[derive(Debug, Clone)]
pub struct IntakePrep {
    /// The fetched candidate.
    pub fetched: FetchedModel,
    /// Recorded native provenance (with per-check verifiability). [FR-MS-2]
    pub provenance: ProvenanceRecord,
    /// The generated CycloneDX SBOM. [NFR-OSS-1]
    pub sbom: String,
    /// The advisory A.9 licence/risk assessment (informs, never decides). [FR-MS-6]
    pub assessment: IntakeAssessment,
}

/// The human licence/risk decision. Constructing this with `accepted: true` is
/// the single point of approval in intake — a human act, informed by (never
/// performed by) the A.9 assessment. [FR-MS-6]
#[derive(Debug, Clone)]
pub struct HumanLicenceGate {
    /// Who reviewed (recorded in the bundle).
    pub reviewer: String,
    /// The licence terms the human captured for the registry/manifest.
    pub captured: Licence,
    /// The human's decision. `false` halts intake with no re-sign.
    pub accepted: bool,
}

/// The staging-side signed bundle (`.mbundle` contents) ready to cross the air
/// gap via Bifrost.
#[derive(Debug, Clone)]
pub struct StagedBundle {
    /// The re-signed manifest + Veldris ML-DSA signature. [NFR-PQ-3]
    pub artifact: SignedArtifact,
    /// The CycloneDX SBOM travelling in the bundle. [NFR-OSS-1]
    pub sbom: String,
    /// The recorded provenance. [FR-MS-2]
    pub provenance: ProvenanceRecord,
    /// The advisory assessment retained for audit (what the human saw). [FR-MS-6]
    pub assessment: IntakeAssessment,
    /// The captured, human-accepted licence. [FR-MS-6]
    pub licence: Licence,
    /// The reviewer who accepted the gate.
    pub reviewer: String,
}

/// Phase 1: fetch, verify provenance, SBOM, advisory assessment. Fails closed on
/// a provenance mismatch; records unverifiable provenance without failing.
pub fn prepare(fetcher: &dyn Fetcher, source: &UpstreamSource) -> Result<IntakePrep, IntakeError> {
    // Fetch — staging only. [FR-MS-1]
    let fetched = fetcher.fetch(source).map_err(IntakeError::Fetch)?;

    // Verify native provenance, recording verifiability per dimension. [FR-MS-2]
    let provenance = verify_provenance(&fetched);
    if provenance.has_mismatch() {
        return Err(IntakeError::ProvenanceMismatch(provenance.source.clone()));
    }

    // CycloneDX SBOM over the verified components. [NFR-OSS-1]
    let sbom = generate_sbom(&fetched, &provenance);

    // Advisory A.9 assessment for the human gate — no decision. [FR-MS-6]
    let assessment = ModelIntakeAssessor::new().assess(&fetched.model_card, &fetched.metadata);

    Ok(IntakePrep {
        fetched,
        provenance,
        sbom,
        assessment,
    })
}

/// Phase 2: apply the human gate and, if accepted, re-sign under Veldris's ML-DSA
/// key — the custody-transfer step that makes the enclosure trust the artefact.
/// [NFR-PQ-3]
#[cfg(feature = "backend-fips")]
pub fn finalize(
    prep: IntakePrep,
    gate: HumanLicenceGate,
    signing_key: &pqc_agility::SigningKey,
) -> Result<StagedBundle, IntakeError> {
    if !gate.accepted {
        return Err(IntakeError::LicenceRejectedByHuman {
            reviewer: gate.reviewer,
        });
    }

    // Build the manifest over the *verified* weights digest, with the licence the
    // human captured. The manifest is what the Veldris signature covers.
    let manifest = ModelManifest {
        id: prep.fetched.proposed_id.clone(),
        family: prep.fetched.family.clone(),
        version: prep.fetched.version.clone(),
        sha256: prep.provenance.weights_sha256,
        hw: prep.fetched.hw,
        licence: gate.captured.clone(),
        // A staging record: the weights travel in the bundle, not as a file beside it.
        weights: None,
    };

    // Re-sign: custody transfer under Veldris's key. [NFR-PQ-3]
    let artifact = model_server::sign_manifest(manifest, signing_key);

    Ok(StagedBundle {
        artifact,
        sbom: prep.sbom,
        provenance: prep.provenance,
        assessment: prep.assessment,
        licence: gate.captured,
        reviewer: gate.reviewer,
    })
}
