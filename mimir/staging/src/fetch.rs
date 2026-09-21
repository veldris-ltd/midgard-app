//! Stage-1 acquisition: fetch a candidate variant from an upstream source.
//!
//! Fetching is the **one place in the whole suite that touches an untrusted,
//! network-connected upstream**, and it lives **only** here in the staging
//! enclave — never in the enclosure. This crate deliberately has no enclosure
//! credentials (no gateway / PQ-mTLS / governance dependency): the only thing
//! that ever crosses the air gap is the signed bundle this pipeline emits, so a
//! staging compromise cannot reach in. [FR-MS-1] [NFR-SEC-1]
//!
//! The concrete upstream client (HuggingFace / NVIDIA NGC / vendor CDN over the
//! network) plugs in behind the [`Fetcher`] seam. The [`ReferenceFetcher`] here
//! is a deterministic, in-memory stand-in that proves the entire intake pipeline
//! **offline, with no network** — exactly as `LocalReference` does for serving.

use std::collections::BTreeMap;

/// Where a candidate model was fetched from (recorded verbatim in provenance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamSource {
    /// A Hugging Face repo at a specific revision.
    HuggingFace {
        /// `owner/name`.
        repo: String,
        /// The revision (commit sha or tag) requested.
        revision: String,
    },
    /// An NVIDIA NGC model.
    Ngc {
        /// The NGC model path.
        model: String,
        /// The version requested.
        version: String,
    },
    /// A vendor CDN URL.
    VendorCdn {
        /// The fetch URL.
        url: String,
    },
}

impl UpstreamSource {
    /// A stable, human-readable identifier for the source (for the record).
    pub fn label(&self) -> String {
        match self {
            UpstreamSource::HuggingFace { repo, revision } => {
                format!("huggingface:{repo}@{revision}")
            }
            UpstreamSource::Ngc { model, version } => format!("ngc:{model}@{version}"),
            UpstreamSource::VendorCdn { url } => format!("cdn:{url}"),
        }
    }
}

/// Native provenance material declared by the upstream, to be *verified* in
/// stage 1 (never trusted as-is). Any field may be absent — absence is itself a
/// recorded fact (`Unverifiable`), not a failure. [FR-MS-2]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclaredProvenance {
    /// A published SHA-256 of the weights, lowercase hex, if the upstream states one.
    pub published_sha256_hex: Option<String>,
    /// A vendor GPG/native signature marker, if present (opaque here; the real
    /// fetcher carries the detached signature bytes).
    pub vendor_signature: Option<String>,
    /// The resolved Hugging Face commit hash, if the source is HF.
    pub hf_commit: Option<String>,
    /// The SHA-256 of each file at that commit, lowercase hex, keyed by in-bundle path
    /// (`model.weights` for the weights), as the operator recorded it from the
    /// upstream's own listing of the pinned revision. It is what makes the commit check
    /// real: the commit is verified only when the fetched bytes match it. [RT-21]
    pub pinned_revision_files: Option<BTreeMap<String, String>>,
}

/// A model as fetched into staging: the raw component blobs plus the card,
/// metadata, and declared provenance. The blobs are keyed by their in-bundle
/// path (e.g. `model.safetensors`, `tokenizer.json`, `config.json`).
#[derive(Debug, Clone)]
pub struct FetchedModel {
    /// The source it came from.
    pub source: UpstreamSource,
    /// Proposed artefact id (family-variant-version), used downstream.
    pub proposed_id: String,
    /// Declared family (GLM / Nemotron / Gemma).
    pub family: String,
    /// Version label.
    pub version: String,
    /// The variant's declared hardware eligibility (catalogued at fetch time;
    /// full hardware-fingerprint enforcement is Prompt 3.b). [FR-MS-5]
    pub hw: model_server::HwProfile,
    /// The weight blob bytes (what the SHA-256 provenance check hashes).
    pub weights: Vec<u8>,
    /// Auxiliary component blobs (tokenizer, config, …) keyed by in-bundle path.
    pub components: BTreeMap<String, Vec<u8>>,
    /// The upstream model card (README / MODEL_CARD), verbatim text.
    pub model_card: String,
    /// The upstream fetch metadata record (what the fetch observed), verbatim text.
    pub metadata: String,
    /// Declared native provenance to verify.
    pub declared: DeclaredProvenance,
}

/// Why a fetch failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The upstream had no such artefact.
    NotFound(String),
    /// The upstream was unreachable / transport failed.
    Unreachable(String),
}

/// The acquisition seam. The real implementation reaches the network; the
/// reference implementation is offline and deterministic.
pub trait Fetcher {
    /// Fetch a candidate variant. Runs in staging only. [FR-MS-1]
    fn fetch(&self, source: &UpstreamSource) -> Result<FetchedModel, FetchError>;
}

/// A deterministic, offline fetcher: it serves pre-registered fixtures keyed by
/// the source label. It touches no network, so the full intake pipeline and
/// AC-MS-1 are provable in CI under `--network none`.
#[derive(Debug, Default, Clone)]
pub struct ReferenceFetcher {
    fixtures: BTreeMap<String, FetchedModel>,
}

impl ReferenceFetcher {
    /// An empty fetcher.
    pub fn new() -> Self {
        Self {
            fixtures: BTreeMap::new(),
        }
    }

    /// Register a fixture to be returned for its source label.
    pub fn with(mut self, model: FetchedModel) -> Self {
        self.fixtures.insert(model.source.label(), model);
        self
    }
}

impl Fetcher for ReferenceFetcher {
    fn fetch(&self, source: &UpstreamSource) -> Result<FetchedModel, FetchError> {
        self.fixtures
            .get(&source.label())
            .cloned()
            .ok_or_else(|| FetchError::NotFound(source.label()))
    }
}
