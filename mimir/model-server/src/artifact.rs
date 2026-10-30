//! Model-artefact provenance: the **verify-on-load invariant** (AC-MI-7).
//!
//! A model may be placed into serving only if its Veldris **ML-DSA signature**
//! verifies, its **licence** policy is met, it is **not revoked**, and the node
//! satisfies its **hardware profile** — otherwise loading **fails closed**. This
//! is the Phase-2 form of the invariant; Phase 3 wraps it in the full Model
//! Registry and the air-gapped Bifrost supply chain (AC-MS-2 generalises this).
//! [NFR-PQ-3]
//!
//! A model on disk is a directory holding a signed manifest file ([`MANIFEST_FILE`])
//! and the weights file it names. The manifest file is format 2: its signature covers
//! the file's bytes exactly as they lie on disk, and is checked **before** a byte of it
//! is parsed, so a tampered manifest is the signature's failure and never the parser's
//! input ([`verify_manifest_file`]). The signed fields bind the weights' SHA-256, file
//! name, format, quantisation and size; [`open_verified_weights`] opens the weights
//! **once**, streams that handle through SHA-256, checks each field, recognises the
//! format from the file's own first bytes, and hands back the same handle, so whatever
//! reads the weights afterwards reads the bytes that were hashed rather than whatever
//! the path names by then. Only GGUF and safetensors load: formats that can carry
//! executable code, such as pickled checkpoints, never do. [FR-MS-4] [MG-SEC-14]
//!
//! An engine in another process cannot be handed that handle, only a path. For it,
//! [`stage_verified_weights`] copies the weights from the verified handle, hashing them
//! again as they are copied, into a serve directory only the verifier writes, read-only
//! and under an atomic rename, and writes [`STAGED_MARKER`] last; the engine loads the
//! staged copy, never the model directory, so a file swapped in there after the check is
//! never what it loads. [MG-SEC-14]
//!
//! A signature proves where a model came from, not that it is current, so
//! `ModelLoader::verify_dir_at_floor` also refuses a release older than the last one
//! accepted for its id, or the same version with different signed content
//! (`attest::ReleaseFloor`). [MG-SEC-14]

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[cfg(feature = "backend-fips")]
use attest::ReleaseFloor;

use crate::hardware::{HwProfile, NodeProfile};

/// The signing algorithm for Veldris model manifests (ML-DSA-65 / FIPS 204).
pub const VELDRIS_MODEL_SIG_ALG: pqc_agility::SigId = pqc_agility::SigId::MlDsa65;

/// The manifest file a model directory holds.
pub const MANIFEST_FILE: &str = "model.manifest";

/// The first line of a manifest file (format 2: signed over its bytes as they lie on
/// disk). [MG-SEC-14]
pub const MANIFEST_FILE_HEADER: &str = "midgard-model-manifest 2";

/// The first line of a format 1 manifest file, which was signed over its parsed fields
/// and so could not be checked before it was parsed. It is refused: re-sign the model
/// with `mg-model sign`. [MG-SEC-14]
const LEGACY_MANIFEST_FILE_HEADER: &str = "midgard-model-manifest 1";

/// The largest manifest file read. A larger one is refused, never truncated.
pub const MAX_MANIFEST_FILE: usize = 64 * 1024;

/// The governing licence of a model artefact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Licence {
    /// SPDX id or vendor name.
    pub name: String,
    /// Whether OSI-approved.
    pub osi_approved: bool,
    /// Whether redistribution is permitted.
    pub redistribution_ok: bool,
}

/// The on-disk form of a model's weights. Both carry tensors and nothing executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeightFormat {
    /// GGUF (llama.cpp).
    Gguf,
    /// safetensors (vLLM and the Hugging Face tooling).
    Safetensors,
}

impl WeightFormat {
    /// The format's name in a manifest file.
    pub fn as_str(self) -> &'static str {
        match self {
            WeightFormat::Gguf => "gguf",
            WeightFormat::Safetensors => "safetensors",
        }
    }

    /// The format a manifest file names.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "gguf" => Some(WeightFormat::Gguf),
            "safetensors" => Some(WeightFormat::Safetensors),
            _ => None,
        }
    }

    fn tag(self) -> u8 {
        match self {
            WeightFormat::Gguf => 1,
            WeightFormat::Safetensors => 2,
        }
    }

    /// Recognise a format from a weights file's first 16 bytes. GGUF starts with its
    /// magic `GGUF`; safetensors with an 8-byte little-endian header length and then
    /// the header's JSON. Anything else — a pickle, a zip — is not recognised.
    pub fn sniff(head: &[u8]) -> Option<Self> {
        if head.starts_with(b"GGUF") {
            return Some(WeightFormat::Gguf);
        }
        if head.len() >= 9 {
            let header_len = u64::from_le_bytes(head[..8].try_into().expect("8 bytes"));
            if (2..=100 * 1024 * 1024).contains(&header_len) && head[8] == b'{' {
                return Some(WeightFormat::Safetensors);
            }
        }
        None
    }
}

/// The weights file a manifest covers: the signature binds its name, format,
/// quantisation and size as well as its digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightsFile {
    /// The file's name in the model directory (no path).
    pub name: String,
    /// Its format.
    pub format: WeightFormat,
    /// Its quantisation, as the upstream names it (e.g. `Q4_K_M`, `awq`, `bf16`).
    pub quantisation: String,
    /// Its size in bytes.
    pub size: u64,
}

/// The signed manifest describing a model build (weights digest + provenance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelManifest {
    /// Stable artefact id.
    pub id: String,
    /// Family (Mistral / Nemotron / Gemma / Granite).
    pub family: String,
    /// Version label.
    pub version: String,
    /// SHA-256 of the weights.
    pub sha256: [u8; 32],
    /// Hardware eligibility.
    pub hw: HwProfile,
    /// Governing licence.
    pub licence: Licence,
    /// The weights file, for a model loaded from disk. `None` for a manifest that
    /// records weights held elsewhere, such as a staging record.
    pub weights: Option<WeightsFile>,
}

impl ModelManifest {
    /// Canonical byte encoding signed by the Veldris key (hand-rolled, no serde).
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        let put_str = |o: &mut Vec<u8>, s: &str| {
            o.extend_from_slice(&(s.len() as u32).to_le_bytes());
            o.extend_from_slice(s.as_bytes());
        };
        put_str(&mut o, &self.id);
        put_str(&mut o, &self.family);
        put_str(&mut o, &self.version);
        o.extend_from_slice(&self.sha256);
        o.extend_from_slice(&self.hw.min_gpus.to_le_bytes());
        o.extend_from_slice(&self.hw.min_vram_gb.to_le_bytes());
        o.push(self.hw.multinode as u8);
        put_str(&mut o, &self.licence.name);
        o.push(self.licence.osi_approved as u8);
        o.push(self.licence.redistribution_ok as u8);
        match &self.weights {
            None => o.push(0),
            Some(w) => {
                o.push(1);
                put_str(&mut o, &w.name);
                o.push(w.format.tag());
                put_str(&mut o, &w.quantisation);
                o.extend_from_slice(&w.size.to_le_bytes());
            }
        }
        o
    }
}

/// A manifest plus its detached Veldris ML-DSA signature — the loadable bundle.
#[derive(Debug, Clone)]
pub struct SignedArtifact {
    /// The manifest.
    pub manifest: ModelManifest,
    /// The ML-DSA signature over `manifest.encode()`.
    pub signature: pqc_agility::Signature,
}

/// The deployment's licence policy.
#[derive(Debug, Clone, Copy)]
pub struct LicencePolicy {
    /// Refuse artefacts that are not redistributable.
    pub require_redistribution_ok: bool,
}

/// The default is permissive on redistribution: a deployment that cares must opt
/// in, because a non-redistributable model is a licensing problem, not a security
/// one. Enforcement of the recorded restrictions is RF-MS-2.
impl Default for LicencePolicy {
    #[allow(clippy::derivable_impls)] // written out so the default is a stated choice, not a fallout of field order
    fn default() -> Self {
        Self {
            require_redistribution_ok: false,
        }
    }
}

/// Why a load was refused (all fail closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadError {
    /// The ML-DSA signature did not verify.
    BadSignature,
    /// The family is not supported in this enclosure.
    UnsupportedFamily,
    /// The artefact has been revoked.
    Revoked,
    /// The licence policy is unmet for this deployment.
    LicenceDenied,
    /// The node cannot satisfy the artefact's hardware profile.
    HardwareIneligible,
    /// The artefact is not in the Model Registry (not a system-of-record entry).
    /// A model the registry does not know cannot be loaded. [FR-MS-3]
    NotRegistered,
    /// The artefact is registered but not in a loadable lifecycle state (e.g.
    /// still `staged`, not yet approved-on-import). [FR-MS-3]
    NotApproved,
    /// The weights file is missing, unreadable, or not the size the manifest records,
    /// or the manifest names no weights file.
    WeightsUnreadable,
    /// The weights are not the bytes that were signed: their SHA-256 differs.
    WeightsMismatch,
    /// The weights are not in the format the manifest records, or in no format the
    /// plane loads.
    UnsupportedWeightsFormat,
    /// The weights file changed while it was being hashed. [MG-SEC-14]
    WeightsChanged,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LoadError::BadSignature => "the manifest signature does not verify",
            LoadError::UnsupportedFamily => "the model family is not supported",
            LoadError::Revoked => "the artefact is revoked",
            LoadError::LicenceDenied => "the licence policy is not met",
            LoadError::HardwareIneligible => "this node cannot serve the model",
            LoadError::NotRegistered => "the artefact is not registered",
            LoadError::NotApproved => "the artefact is not approved",
            LoadError::WeightsUnreadable => {
                "the weights file is missing, unreadable or the wrong size"
            }
            LoadError::WeightsMismatch => "the weights do not match the signed digest",
            LoadError::UnsupportedWeightsFormat => {
                "the weights are not in the signed format (only GGUF and safetensors load)"
            }
            LoadError::WeightsChanged => "the weights file changed while it was being hashed",
        })
    }
}

impl std::error::Error for LoadError {}

/// A model that passed every load gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedModel {
    /// The artefact id.
    pub id: String,
    /// The family.
    pub family: String,
}

/// A model loaded from its directory: the verified manifest and the weights file it
/// covers, held open.
#[cfg(feature = "backend-fips")]
#[derive(Debug)]
pub struct VerifiedModel {
    /// The verified manifest.
    pub manifest: ModelManifest,
    /// The manifest file's bytes, exactly as they were verified.
    pub manifest_bytes: Vec<u8>,
    /// What passed the load gates.
    pub loaded: LoadedModel,
    /// The verified weights: read them through this handle, not by re-opening the path.
    pub weights: VerifiedWeights,
}

/// The verify-on-load gate. Holds the trusted Veldris verifying key, the licence
/// policy, the supported families, and the revocation set.
#[cfg(feature = "backend-fips")]
pub struct ModelLoader {
    trust_key: pqc_agility::VerifyingKey,
    policy: LicencePolicy,
    families: BTreeSet<String>,
    revoked: BTreeSet<String>,
}

#[cfg(feature = "backend-fips")]
impl ModelLoader {
    /// Build a loader trusting `trust_key` (the Veldris signing root). Supported
    /// families default to Mistral / Nemotron / Gemma / Granite; Granite, the Apache-2.0
    /// family of the default CPU model, is among them.
    pub fn new(trust_key: pqc_agility::VerifyingKey, policy: LicencePolicy) -> Self {
        Self {
            trust_key,
            policy,
            families: ["Mistral", "Nemotron", "Gemma", "Granite"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            revoked: BTreeSet::new(),
        }
    }

    /// Revoke an artefact id — a load kill-switch (Phase-3 registry generalises it).
    pub fn revoke(&mut self, id: &str) {
        self.revoked.insert(id.to_string());
    }

    /// Verify an artefact against the load invariant and, if every gate passes,
    /// return the loadable model. **Fails closed** on any mismatch. [AC-MI-7]
    pub fn verify_and_load(
        &self,
        artifact: &SignedArtifact,
        node: &NodeProfile,
    ) -> Result<LoadedModel, LoadError> {
        // 1. Signature — the custody root. Nothing else is trusted until this
        // passes, so it runs first and every later gate assumes it did.
        //
        // The decision itself is `attest::accept`, shared with the CI
        // release gate, Bifrost's artefact ingest and pre-deploy re-check, and
        // the offline-bundle import — one implementation of "reject unless
        // proven". Signing under the Mimir model domain also means a model
        // manifest signature can never be replayed as a Bifrost artefact or
        // bundle signature. [AC-RF-SC-2d] [AC-MS-2]
        attest::accept::accept(
            &attest::accept::MIMIR_MODEL,
            &artifact.manifest.encode(),
            &self.trust_key,
            &artifact.signature,
            &[VELDRIS_MODEL_SIG_ALG],
        )
        .map_err(|_| LoadError::BadSignature)?;
        self.gates(&artifact.manifest, node)
    }

    /// The gates after the signature: family, revocation, licence and hardware. Only
    /// ever applied to a manifest whose signature has verified.
    fn gates(
        &self,
        manifest: &ModelManifest,
        node: &NodeProfile,
    ) -> Result<LoadedModel, LoadError> {
        // 2. Supported family.
        if !self.families.contains(&manifest.family) {
            return Err(LoadError::UnsupportedFamily);
        }
        // 3. Revocation (fails closed even with a valid signature).
        if self.revoked.contains(&manifest.id) {
            return Err(LoadError::Revoked);
        }
        // 4. Licence policy.
        if self.policy.require_redistribution_ok && !manifest.licence.redistribution_ok {
            return Err(LoadError::LicenceDenied);
        }
        // 5. Hardware eligibility.
        if !manifest.hw.is_satisfied_by(node) {
            return Err(LoadError::HardwareIneligible);
        }

        Ok(LoadedModel {
            id: manifest.id.clone(),
            family: manifest.family.clone(),
        })
    }

    /// Load a model from a directory holding [`MANIFEST_FILE`] and the weights file it
    /// names. The manifest file's signature is checked over its raw bytes before it is
    /// parsed; then it must pass every gate of [`verify_and_load`](Self::verify_and_load)
    /// before its weights are opened; then the weights must match it, hashed through
    /// the handle returned in [`VerifiedModel::weights`]. Fails closed at the first
    /// failure. [AC-MI-7] [FR-MS-4] [MG-SEC-14]
    ///
    /// This checks a model; it keeps no release floor. A service that serves the model
    /// loads it with [`verify_dir_at_floor`](Self::verify_dir_at_floor).
    pub fn verify_dir(&self, dir: &Path, node: &NodeProfile) -> Result<VerifiedModel, String> {
        self.load_dir(dir, node, None)
    }

    /// [`verify_dir`](Self::verify_dir), and refuse a release older than the last one
    /// `floor` accepted for this model id, or the same version with different signed
    /// content (anti-rollback). Once every check has passed, the floor rises to this
    /// release (persisted, for a floor opened over a file). [MG-SEC-14]
    pub fn verify_dir_at_floor(
        &self,
        dir: &Path,
        node: &NodeProfile,
        floor: &mut ReleaseFloor,
    ) -> Result<VerifiedModel, String> {
        self.load_dir(dir, node, Some(floor))
    }

    fn load_dir(
        &self,
        dir: &Path,
        node: &NodeProfile,
        floor: Option<&mut ReleaseFloor>,
    ) -> Result<VerifiedModel, String> {
        let path = dir.join(MANIFEST_FILE);
        let mut bytes = Vec::new();
        File::open(&path)
            .and_then(|file| {
                file.take(MAX_MANIFEST_FILE as u64 + 1)
                    .read_to_end(&mut bytes)
            })
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if bytes.len() > MAX_MANIFEST_FILE {
            return Err(format!(
                "{}: refused: larger than {MAX_MANIFEST_FILE} bytes",
                path.display()
            ));
        }
        // 1. The signature, over the bytes as read — before any of them is parsed.
        let manifest = verify_manifest_file(&bytes, &self.trust_key)
            .map_err(|e| format!("{}: refused: {e}", path.display()))?;
        // 2–5. The gates on what the signature vouches for.
        let loaded = self
            .gates(&manifest, node)
            .map_err(|e| format!("{}: refused: {e}", path.display()))?;
        // 6. Anti-rollback, before the weights are read; the floor rises only once
        // everything has passed.
        let content = manifest_content_digest(&bytes);
        if let Some(floor) = floor.as_deref() {
            floor
                .check(&manifest.id, &manifest.version, &content)
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        // 7. The weights, opened once and hashed through that handle.
        let name = &manifest
            .weights
            .as_ref()
            .expect("a parsed manifest names its weights file")
            .name;
        let weights_path = dir.join(name);
        let weights = open_verified_weights(&weights_path, &manifest)
            .map_err(|e| format!("{}: refused: {e}", weights_path.display()))?;
        if let Some(floor) = floor {
            floor
                .admit(&manifest.id, &manifest.version, &content)
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(VerifiedModel {
            manifest,
            manifest_bytes: bytes,
            loaded,
            weights,
        })
    }
}

/// The digest of a manifest file's signed content (everything but the signature line):
/// what the release floor records for a model, so a re-signed copy of the same manifest
/// is the same release. Only call it on bytes that verified.
#[cfg(feature = "backend-fips")]
fn manifest_content_digest(bytes: &[u8]) -> [u8; 32] {
    let body = split_manifest_file(bytes).map_or(bytes, |(body, _)| body);
    pqc_agility::hash::sha256(body)
}

/// Sign a manifest under an Veldris ML-DSA signing key (the staging-side custody
/// step; also used by tests). [NFR-PQ-3]
#[cfg(feature = "backend-fips")]
pub fn sign_manifest(
    manifest: ModelManifest,
    signing_key: &pqc_agility::SigningKey,
) -> SignedArtifact {
    // Signed through the shared helper so the preimage cannot drift from the one
    // `verify_and_load` reconstructs.
    let signature = attest::accept::sign_in_domain(
        &attest::accept::MIMIR_MODEL,
        &manifest.encode(),
        signing_key,
    )
    .expect("ml-dsa signing");
    SignedArtifact {
        manifest,
        signature,
    }
}

/// What reading a weights file found: its size, format and SHA-256.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightsDigest {
    /// Bytes read.
    pub size: u64,
    /// The format its first bytes show, if any.
    pub format: Option<WeightFormat>,
    /// Its SHA-256.
    pub sha256: [u8; 32],
}

/// Stream a weights file through SHA-256, one mebibyte at a time.
pub fn digest_weights(reader: &mut impl Read) -> std::io::Result<WeightsDigest> {
    use pqc_agility::hash::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut head = Vec::with_capacity(16);
    let mut size = 0u64;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if head.len() < 16 {
            let take = (16 - head.len()).min(n);
            head.extend_from_slice(&buf[..take]);
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok(WeightsDigest {
        size,
        format: WeightFormat::sniff(&head),
        sha256: hasher.finalize(),
    })
}

/// The verified weights file, held open: the handle the SHA-256 was computed through.
/// Whatever loads the weights reads them through [`file`](Self::file), so a file
/// swapped in at the same path after the check is never what is read. [MG-SEC-14]
#[derive(Debug)]
pub struct VerifiedWeights {
    path: PathBuf,
    file: File,
    size: u64,
    modified: Option<SystemTime>,
}

impl VerifiedWeights {
    /// Where the weights were opened from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Their size in bytes, as signed.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// The verified file, rewound to its first byte.
    pub fn file(&mut self) -> std::io::Result<&mut File> {
        self.file.seek(SeekFrom::Start(0))?;
        Ok(&mut self.file)
    }

    /// Whether the path still names the file that was verified, unchanged: the same
    /// file (device and inode, on Unix), the same size and the same modification time.
    /// This only detects a swap; a loader that can only be handed a path — an engine in
    /// another process — is given a staged copy instead ([`stage_verified_weights`]), so
    /// that a swap cannot reach it. [MG-SEC-14]
    pub fn still_at_path(&self) -> bool {
        let (Ok(now), Ok(held)) = (std::fs::metadata(&self.path), self.file.metadata()) else {
            return false;
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if now.dev() != held.dev() || now.ino() != held.ino() {
                return false;
            }
        }
        now.len() == self.size
            && held.len() == self.size
            && now.modified().ok() == self.modified
            && held.modified().ok() == self.modified
    }
}

/// Open a weights file **once** and check it against a manifest whose signature has
/// already verified: its size, its format (from its own first bytes, not its name) and
/// its SHA-256, streamed through the same handle that is returned, rewound. The file
/// must not change while it is hashed. [AC-MI-7] [FR-MS-4] [MG-SEC-14]
pub fn open_verified_weights(
    path: &Path,
    manifest: &ModelManifest,
) -> Result<VerifiedWeights, LoadError> {
    let expected = manifest
        .weights
        .as_ref()
        .ok_or(LoadError::WeightsUnreadable)?;
    let mut file = File::open(path).map_err(|_| LoadError::WeightsUnreadable)?;
    let before = file.metadata().map_err(|_| LoadError::WeightsUnreadable)?;
    if !before.is_file() || before.len() != expected.size {
        return Err(LoadError::WeightsUnreadable);
    }
    let found = digest_weights(&mut file).map_err(|_| LoadError::WeightsUnreadable)?;
    let after = file.metadata().map_err(|_| LoadError::WeightsUnreadable)?;
    if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
        return Err(LoadError::WeightsChanged);
    }
    if found.size != expected.size {
        return Err(LoadError::WeightsUnreadable);
    }
    if found.format != Some(expected.format) {
        return Err(LoadError::UnsupportedWeightsFormat);
    }
    if !pqc_agility::hash::ct_eq(&found.sha256, &manifest.sha256) {
        return Err(LoadError::WeightsMismatch);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| LoadError::WeightsUnreadable)?;
    Ok(VerifiedWeights {
        path: path.to_path_buf(),
        file,
        size: expected.size,
        modified: before.modified().ok(),
    })
}

/// [`open_verified_weights`], keeping only the verdict. Prefer the handle: checking a
/// path and then opening it again to load is the race this exists to avoid.
pub fn verify_weights(path: &Path, manifest: &ModelManifest) -> Result<(), LoadError> {
    open_verified_weights(path, manifest).map(|_| ())
}

/// The marker [`stage_verified_weights`] writes into a serve directory last, once the
/// staged weights are in place: `<file name> <sha256 hex>`. An engine in another process
/// starts only once it exists, and it is removed before a new model is verified, so the
/// engine never starts on a file that is still being staged. [MG-SEC-14]
pub const STAGED_MARKER: &str = "staged.ready";

/// Reads from `reader` and copies every byte read into `writer`, so one pass both hashes
/// and copies.
struct Tee<'a, R, W> {
    reader: &'a mut R,
    writer: &'a mut W,
}

impl<R: Read, W: std::io::Write> Read for Tee<'_, R, W> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.reader.read(buf)?;
        self.writer.write_all(&buf[..n])?;
        Ok(n)
    }
}

/// Weights staged by [`stage_verified_weights`] for an engine that loads by path: a
/// read-only copy in a directory only the verifier writes, held open. [MG-SEC-14]
#[derive(Debug)]
pub struct StagedWeights {
    path: PathBuf,
    file: File,
    size: u64,
    modified: Option<SystemTime>,
    sha256: [u8; 32],
}

impl StagedWeights {
    /// The staged file: what the engine is told to load.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Its size in bytes, as signed.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Its SHA-256, as signed.
    pub fn sha256(&self) -> [u8; 32] {
        self.sha256
    }

    /// Whether the staged path still names the file that was staged, unchanged and still
    /// read-only: the same file (device and inode, on Unix), the same size and the same
    /// modification time. Cheap enough for every readiness probe. [MG-SEC-14]
    pub fn still_in_place(&self) -> bool {
        let (Ok(now), Ok(held)) = (std::fs::metadata(&self.path), self.file.metadata()) else {
            return false;
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if now.dev() != held.dev() || now.ino() != held.ino() {
                return false;
            }
        }
        now.permissions().readonly()
            && now.len() == self.size
            && held.len() == self.size
            && now.modified().ok() == self.modified
            && held.modified().ok() == self.modified
    }

    /// Hash the staged file again, through the handle held on it, and check it is still
    /// the signed bytes and still in place. This reads the whole file. [MG-SEC-14]
    pub fn verify_digest(&mut self) -> Result<(), LoadError> {
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|_| LoadError::WeightsUnreadable)?;
        let found = digest_weights(&mut self.file).map_err(|_| LoadError::WeightsUnreadable)?;
        if found.size != self.size || !pqc_agility::hash::ct_eq(&found.sha256, &self.sha256) {
            return Err(LoadError::WeightsMismatch);
        }
        if !self.still_in_place() {
            return Err(LoadError::WeightsChanged);
        }
        Ok(())
    }
}

/// Remove a serve directory's [`STAGED_MARKER`], so an engine waiting on it does not
/// start until new weights are staged. A missing marker is not an error. [MG-SEC-14]
pub fn withdraw_staged(serve_dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(serve_dir.join(STAGED_MARKER)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Make a file read-only for everyone (0444 on Unix).
fn make_read_only(file: &File) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o444))
    }
    #[cfg(not(unix))]
    {
        let mut permissions = file.metadata()?.permissions();
        permissions.set_readonly(true);
        file.set_permissions(permissions)
    }
}

/// Create `path` afresh for writing, readable only by its owner while it is written.
fn create_fresh(path: &Path) -> std::io::Result<File> {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.is_file() {
            let mut permissions = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)] // only to remove our own leftover
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(path, permissions);
        }
        std::fs::remove_file(path)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Rename `from` over `to` and make the rename durable.
fn replace(from: &Path, to: &Path, dir: &Path) -> std::io::Result<()> {
    #[cfg(not(unix))]
    if let Ok(meta) = std::fs::metadata(to) {
        // Windows will not rename over a read-only file.
        let mut permissions = meta.permissions();
        #[allow(clippy::permissions_set_readonly_false)] // only to replace our own file
        permissions.set_readonly(false);
        std::fs::set_permissions(to, permissions)?;
        std::fs::remove_file(to)?;
    }
    std::fs::rename(from, to)?;
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// Stage verified weights for an engine in another process that can only be handed a
/// path, such as a llama.cpp or vLLM sidecar, so that what it loads is the bytes that
/// were verified rather than whatever the model directory's path names by then.
/// [MG-SEC-14]
///
/// The weights are copied **from the verified handle**, never re-opened by path, and
/// hashed again as they are copied: if they are not the signed size, format and SHA-256
/// (the file was changed in place after it was verified), nothing is staged. The copy is
/// written to a temporary file in `serve_dir`, flushed to disk, hashed once more as it
/// lies there, made read-only (0444) and renamed atomically to the weights' signed file
/// name; only then is [`STAGED_MARKER`] written. The marker is withdrawn first, so a
/// failure leaves no marker behind.
///
/// `serve_dir` must be a directory that only the verifier can write — the engine mounts
/// it read-only — and not the model directory.
pub fn stage_verified_weights(
    weights: &mut VerifiedWeights,
    manifest: &ModelManifest,
    serve_dir: &Path,
) -> Result<StagedWeights, String> {
    let expected = manifest
        .weights
        .as_ref()
        .ok_or_else(|| LoadError::WeightsUnreadable.to_string())?;
    check_file_name(&expected.name)?;
    let dir_meta = std::fs::metadata(serve_dir)
        .map_err(|e| format!("cannot use {}: {e}", serve_dir.display()))?;
    if !dir_meta.is_dir() {
        return Err(format!("{} is not a directory", serve_dir.display()));
    }
    withdraw_staged(serve_dir).map_err(|e| {
        format!(
            "cannot withdraw the staged marker in {}: {e}",
            serve_dir.display()
        )
    })?;

    let target = serve_dir.join(&expected.name);
    let temp = serve_dir.join(format!(".{}.staging", expected.name));
    let refuse = |why: String| {
        let _ = std::fs::remove_file(&temp);
        format!("{}: refused to stage: {why}", target.display())
    };
    let mut out = create_fresh(&temp).map_err(|e| refuse(e.to_string()))?;

    // 1. Copy from the verified handle, hashing what is copied.
    let source = weights.file().map_err(|e| refuse(e.to_string()))?;
    let copied = digest_weights(&mut Tee {
        reader: source,
        writer: &mut out,
    })
    .map_err(|e| refuse(e.to_string()))?;
    let matches = |found: &WeightsDigest| {
        found.size == expected.size
            && found.format == Some(expected.format)
            && pqc_agility::hash::ct_eq(&found.sha256, &manifest.sha256)
    };
    if !matches(&copied) {
        return Err(refuse(
            "the weights changed since they were verified: they are no longer the signed bytes"
                .into(),
        ));
    }
    out.sync_all().map_err(|e| refuse(e.to_string()))?;

    // 2. Hash the copy as it lies on disk.
    out.seek(SeekFrom::Start(0))
        .map_err(|e| refuse(e.to_string()))?;
    let landed = digest_weights(&mut out).map_err(|e| refuse(e.to_string()))?;
    if !matches(&landed) {
        return Err(refuse(LoadError::WeightsMismatch.to_string()));
    }

    // 3. Read-only, then into place under its signed name.
    make_read_only(&out).map_err(|e| refuse(e.to_string()))?;
    let meta = out.metadata().map_err(|e| refuse(e.to_string()))?;
    replace(&temp, &target, serve_dir).map_err(|e| refuse(e.to_string()))?;
    let file = File::open(&target).map_err(|e| refuse(e.to_string()))?;
    let staged = StagedWeights {
        path: target.clone(),
        file,
        size: expected.size,
        modified: meta.modified().ok(),
        sha256: manifest.sha256,
    };
    if !staged.still_in_place() {
        return Err(refuse(LoadError::WeightsChanged.to_string()));
    }
    drop(out);

    // 4. The marker, last.
    let marker = serve_dir.join(STAGED_MARKER);
    let marker_temp = serve_dir.join(format!(".{STAGED_MARKER}.staging"));
    let write_marker = || -> std::io::Result<()> {
        let mut file = create_fresh(&marker_temp)?;
        std::io::Write::write_all(
            &mut file,
            format!("{} {}\n", expected.name, hex(&manifest.sha256)).as_bytes(),
        )?;
        file.sync_all()?;
        make_read_only(&file)?;
        replace(&marker_temp, &marker, serve_dir)
    };
    write_marker().map_err(|e| {
        let _ = std::fs::remove_file(&marker_temp);
        format!("cannot write {}: {e}", marker.display())
    })?;
    Ok(staged)
}

/// The keys of a manifest file's body, in order. The signature line follows them.
const MANIFEST_KEYS: [&str; 14] = [
    "id",
    "family",
    "version",
    "sha256",
    "min-gpus",
    "min-vram-gb",
    "multinode",
    "licence",
    "osi-approved",
    "redistribution-ok",
    "weights-file",
    "weights-format",
    "quantisation",
    "weights-size",
];

/// What separates a manifest file's signed body from its signature line.
const SIGNATURE_LINE: &[u8] = b"\nsignature ";

/// Why a manifest file was refused. Every variant is a refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestFileError {
    /// The file has no well-formed signature line to check, or is format 1.
    Unsigned(String),
    /// The signature does not verify over the file's bytes: tampered, or signed by
    /// another key. Nothing in the file was parsed.
    BadSignature,
    /// The signature verified but the signed body is not a well-formed manifest.
    Malformed(String),
}

impl std::fmt::Display for ManifestFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestFileError::Unsigned(why) => write!(f, "not a signed model manifest: {why}"),
            ManifestFileError::BadSignature => write!(f, "{}", LoadError::BadSignature),
            ManifestFileError::Malformed(why) => write!(f, "malformed manifest: {why}"),
        }
    }
}

impl std::error::Error for ManifestFileError {}

/// A manifest value: one word of printable ASCII.
fn check_token(what: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 256 || !value.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(format!(
            "`{what}` must be one word of printable ASCII, not {value:?}"
        ));
    }
    Ok(())
}

/// A weights file name: a token that cannot leave the model directory.
fn check_file_name(name: &str) -> Result<(), String> {
    check_token("weights-file", name)?;
    if name.contains(['/', '\\', ':']) || name.starts_with('.') {
        return Err(format!(
            "`weights-file` must be a plain file name, not {name:?}"
        ));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Render a manifest's signed body: the header line and one `key value` line per field
/// in a fixed order. Only a manifest that names its weights file can be rendered, and
/// what is rendered always parses back.
pub fn render_manifest_body(m: &ModelManifest) -> Result<String, String> {
    let w = m
        .weights
        .as_ref()
        .ok_or("the manifest names no weights file")?;
    for (what, value) in [
        ("id", m.id.as_str()),
        ("family", m.family.as_str()),
        ("version", m.version.as_str()),
        ("licence", m.licence.name.as_str()),
        ("quantisation", w.quantisation.as_str()),
    ] {
        check_token(what, value)?;
    }
    check_file_name(&w.name)?;
    let yes_no = |b: bool| if b { "yes" } else { "no" };
    Ok(format!(
        "{MANIFEST_FILE_HEADER}\nid {}\nfamily {}\nversion {}\nsha256 {}\nmin-gpus {}\n\
         min-vram-gb {}\nmultinode {}\nlicence {}\nosi-approved {}\nredistribution-ok {}\n\
         weights-file {}\nweights-format {}\nquantisation {}\nweights-size {}\n",
        m.id,
        m.family,
        m.version,
        hex(&m.sha256),
        m.hw.min_gpus,
        m.hw.min_vram_gb,
        yes_no(m.hw.multinode),
        m.licence.name,
        yes_no(m.licence.osi_approved),
        yes_no(m.licence.redistribution_ok),
        w.name,
        w.format.as_str(),
        w.quantisation,
        w.size,
    ))
}

/// Append the signature line to a rendered body: the manifest file.
pub fn manifest_file_with_signature(body: &str, signature: &pqc_agility::Signature) -> String {
    format!(
        "{body}signature {} {}\n",
        signature.alg().as_str(),
        hex(signature.as_bytes())
    )
}

/// Render and sign a manifest file (format 2): the body, then a signature over the
/// body's exact bytes in the model-file domain. This is what `mg-model sign` writes.
/// [MG-SEC-14]
#[cfg(feature = "backend-fips")]
pub fn sign_manifest_file(
    manifest: &ModelManifest,
    signing_key: &pqc_agility::SigningKey,
) -> Result<String, String> {
    if signing_key.alg() != VELDRIS_MODEL_SIG_ALG {
        return Err(format!(
            "the signature must be {}",
            VELDRIS_MODEL_SIG_ALG.as_str()
        ));
    }
    let body = render_manifest_body(manifest)?;
    let signature = attest::accept::sign_in_domain(
        &attest::accept::MIMIR_MODEL_FILE,
        body.as_bytes(),
        signing_key,
    )
    .map_err(|e| format!("cannot sign the manifest: {e}"))?;
    Ok(manifest_file_with_signature(&body, &signature))
}

/// Split a manifest file into its signed body and its signature, without reading the
/// body: the signature line is the last line, and the body is every byte before it.
/// This is framing, not parsing — nothing in the body is interpreted. [MG-SEC-14]
pub fn split_manifest_file(
    bytes: &[u8],
) -> Result<(&[u8], pqc_agility::Signature), ManifestFileError> {
    let unsigned = |why: &str| ManifestFileError::Unsigned(why.to_string());
    if bytes.starts_with(LEGACY_MANIFEST_FILE_HEADER.as_bytes()) {
        return Err(unsigned(
            "format 1 was signed over its parsed fields and is no longer accepted; re-sign \
             the model with `mg-model sign`",
        ));
    }
    let at = bytes
        .windows(SIGNATURE_LINE.len())
        .rposition(|w| w == SIGNATURE_LINE)
        .ok_or_else(|| unsigned("no signature line"))?;
    let (body, line) = bytes.split_at(at + 1);
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = std::str::from_utf8(line).map_err(|_| unsigned("the signature line is not text"))?;
    let mut parts = line.split(' ');
    let (Some("signature"), Some(alg), Some(sig), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(unsigned(
            "expected the last line to be `signature <alg> <hex>`",
        ));
    };
    if alg != VELDRIS_MODEL_SIG_ALG.as_str() {
        return Err(ManifestFileError::Unsigned(format!(
            "the signature must be {}, not {alg:?}",
            VELDRIS_MODEL_SIG_ALG.as_str()
        )));
    }
    let sig = unhex(sig)
        .filter(|b| !b.is_empty())
        .ok_or_else(|| unsigned("the signature is not hex"))?;
    Ok((
        body,
        pqc_agility::Signature::new(VELDRIS_MODEL_SIG_ALG, sig),
    ))
}

/// Verify a manifest file and only then parse it: the signature is checked over the
/// body's bytes exactly as they lie in the file, through the one acceptance decision in
/// the model-file domain, and a body is parsed only once it has verified. A tampered
/// file is refused as [`ManifestFileError::BadSignature`] whatever it contains.
/// [AC-MI-7] [MG-SEC-14]
pub fn verify_manifest_file(
    bytes: &[u8],
    trust_key: &pqc_agility::VerifyingKey,
) -> Result<ModelManifest, ManifestFileError> {
    let (body, signature) = split_manifest_file(bytes)?;
    attest::accept::accept(
        &attest::accept::MIMIR_MODEL_FILE,
        body,
        trust_key,
        &signature,
        &[VELDRIS_MODEL_SIG_ALG],
    )
    .map_err(|_| ManifestFileError::BadSignature)?;
    let text = std::str::from_utf8(body)
        .map_err(|_| ManifestFileError::Malformed("the body is not text".into()))?;
    parse_manifest_body(text).map_err(ManifestFileError::Malformed)
}

/// Parse a manifest body strictly: the header, every key in order, nothing after. Only
/// ever called on a body whose signature has verified.
fn parse_manifest_body(text: &str) -> Result<ModelManifest, String> {
    let body = text
        .strip_suffix('\n')
        .ok_or("the body must end with a newline")?;
    let mut lines = body.split('\n');
    if lines.next() != Some(MANIFEST_FILE_HEADER) {
        return Err(format!(
            "not a model manifest: the first line must be `{MANIFEST_FILE_HEADER}`"
        ));
    }
    let mut values = Vec::with_capacity(MANIFEST_KEYS.len());
    for key in MANIFEST_KEYS {
        let line = lines.next().ok_or_else(|| format!("missing `{key}`"))?;
        let (found, value) = line
            .split_once(' ')
            .ok_or_else(|| format!("expected `{key} <value>`, found {line:?}"))?;
        if found != key {
            return Err(format!("expected `{key}`, found `{found}`"));
        }
        values.push(value);
    }
    if lines.next().is_some() {
        return Err("unexpected lines before the signature".into());
    }

    let token = |i: usize| -> Result<&str, String> {
        check_token(MANIFEST_KEYS[i], values[i])?;
        Ok(values[i])
    };
    let number = |i: usize| -> Result<u64, String> {
        let v = token(i)?;
        if v.len() > 20 || !v.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!(
                "`{}` must be a number, not {v:?}",
                MANIFEST_KEYS[i]
            ));
        }
        v.parse::<u64>()
            .map_err(|_| format!("`{}` is out of range", MANIFEST_KEYS[i]))
    };
    let small = |i: usize| -> Result<u32, String> {
        u32::try_from(number(i)?).map_err(|_| format!("`{}` is out of range", MANIFEST_KEYS[i]))
    };
    let yes_no = |i: usize| -> Result<bool, String> {
        match token(i)? {
            "yes" => Ok(true),
            "no" => Ok(false),
            other => Err(format!(
                "`{}` must be yes or no, not {other:?}",
                MANIFEST_KEYS[i]
            )),
        }
    };

    let sha256 = unhex(token(3)?)
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
        .ok_or("`sha256` must be 64 hex digits")?;
    let name = token(10)?;
    check_file_name(name)?;
    let format = WeightFormat::from_name(token(11)?).ok_or_else(|| {
        format!(
            "`weights-format` must be gguf or safetensors, not {:?}",
            values[11]
        )
    })?;

    Ok(ModelManifest {
        id: token(0)?.to_string(),
        family: token(1)?.to_string(),
        version: token(2)?.to_string(),
        sha256,
        hw: HwProfile {
            min_gpus: small(4)?,
            min_vram_gb: small(5)?,
            multinode: yes_no(6)?,
        },
        licence: Licence {
            name: token(7)?.to_string(),
            osi_approved: yes_no(8)?,
            redistribution_ok: yes_no(9)?,
        },
        weights: Some(WeightsFile {
            name: name.to_string(),
            format,
            quantisation: token(12)?.to_string(),
            size: number(13)?,
        }),
    })
}

/// Parse the model-signing trust key: the `<alg> <key-id> <hex>` line `mg-model keygen`
/// exports. Blank lines and `#` comments are ignored; exactly one ML-DSA-65 key is
/// accepted.
pub fn parse_trust_key(text: &str) -> Result<pqc_agility::VerifyingKey, String> {
    let mut keys = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    let line = keys.next().ok_or("the trust file holds no key")?;
    if keys.next().is_some() {
        return Err("the trust file must hold exactly one key".into());
    }
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() != 3 {
        return Err("expected `<alg> <key-id> <hex>`".into());
    }
    if parts[0] != VELDRIS_MODEL_SIG_ALG.as_str() {
        return Err(format!(
            "the model-signing key must be {}, not {}",
            VELDRIS_MODEL_SIG_ALG.as_str(),
            parts[0]
        ));
    }
    let bytes = unhex(parts[2])
        .filter(|b| !b.is_empty())
        .ok_or("the key is not hex")?;
    Ok(pqc_agility::VerifyingKey::new(VELDRIS_MODEL_SIG_ALG, bytes))
}
