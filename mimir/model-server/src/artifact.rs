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
//! and the weights file it names. The signature covers the weights' SHA-256, file name,
//! format, quantisation and size; [`verify_weights`] streams the file through SHA-256 and
//! checks each, and recognises the format from the file's own first bytes. Only GGUF and
//! safetensors load: formats that can carry executable code, such as pickled
//! checkpoints, never do. [FR-MS-4]

use std::collections::BTreeSet;
use std::io::Read;
#[cfg(feature = "backend-fips")]
use std::path::{Path, PathBuf};

use crate::hardware::{HwProfile, NodeProfile};

/// The signing algorithm for Veldris model manifests (ML-DSA-65 / FIPS 204).
pub const VELDRIS_MODEL_SIG_ALG: pqc_agility::SigId = pqc_agility::SigId::MlDsa65;

/// The manifest file a model directory holds.
pub const MANIFEST_FILE: &str = "model.manifest";

/// The first line of a manifest file.
pub const MANIFEST_FILE_HEADER: &str = "midgard-model-manifest 1";

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

/// A model loaded from its directory: the verified artefact and its weights file.
#[cfg(feature = "backend-fips")]
#[derive(Debug, Clone)]
pub struct VerifiedModel {
    /// The signed artefact.
    pub artifact: SignedArtifact,
    /// What passed the load gates.
    pub loaded: LoadedModel,
    /// The verified weights file.
    pub weights_path: PathBuf,
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

        // 2. Supported family.
        if !self.families.contains(&artifact.manifest.family) {
            return Err(LoadError::UnsupportedFamily);
        }
        // 3. Revocation (fails closed even with a valid signature).
        if self.revoked.contains(&artifact.manifest.id) {
            return Err(LoadError::Revoked);
        }
        // 4. Licence policy.
        if self.policy.require_redistribution_ok && !artifact.manifest.licence.redistribution_ok {
            return Err(LoadError::LicenceDenied);
        }
        // 5. Hardware eligibility.
        if !artifact.manifest.hw.is_satisfied_by(node) {
            return Err(LoadError::HardwareIneligible);
        }

        Ok(LoadedModel {
            id: artifact.manifest.id.clone(),
            family: artifact.manifest.family.clone(),
        })
    }

    /// Load a model from a directory holding [`MANIFEST_FILE`] and the weights file it
    /// names. The manifest must parse and pass every gate of
    /// [`verify_and_load`](Self::verify_and_load) before its weights are read; then the
    /// weights must match it. Fails closed at the first failure. [AC-MI-7] [FR-MS-4]
    pub fn verify_dir(&self, dir: &Path, node: &NodeProfile) -> Result<VerifiedModel, String> {
        let path = dir.join(MANIFEST_FILE);
        let mut text = String::new();
        std::fs::File::open(&path)
            .and_then(|file| file.take(64 * 1024).read_to_string(&mut text))
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let artifact =
            parse_manifest_file(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let loaded = self
            .verify_and_load(&artifact, node)
            .map_err(|e| format!("{}: refused: {e}", path.display()))?;
        let name = &artifact
            .manifest
            .weights
            .as_ref()
            .expect("a parsed manifest names its weights file")
            .name;
        let weights_path = dir.join(name);
        verify_weights(&weights_path, &artifact.manifest)
            .map_err(|e| format!("{}: refused: {e}", weights_path.display()))?;
        Ok(VerifiedModel {
            artifact,
            loaded,
            weights_path,
        })
    }
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

/// Check a weights file against a manifest whose signature has already verified: its
/// size, its format (from its own first bytes, not its name) and its SHA-256, streamed.
/// [AC-MI-7] [FR-MS-4]
pub fn verify_weights(path: &std::path::Path, manifest: &ModelManifest) -> Result<(), LoadError> {
    let expected = manifest
        .weights
        .as_ref()
        .ok_or(LoadError::WeightsUnreadable)?;
    let mut file = std::fs::File::open(path).map_err(|_| LoadError::WeightsUnreadable)?;
    let on_disk = file
        .metadata()
        .map_err(|_| LoadError::WeightsUnreadable)?
        .len();
    if on_disk != expected.size {
        return Err(LoadError::WeightsUnreadable);
    }
    let found = digest_weights(&mut file).map_err(|_| LoadError::WeightsUnreadable)?;
    if found.size != expected.size {
        return Err(LoadError::WeightsUnreadable);
    }
    if found.format != Some(expected.format) {
        return Err(LoadError::UnsupportedWeightsFormat);
    }
    if !pqc_agility::hash::ct_eq(&found.sha256, &manifest.sha256) {
        return Err(LoadError::WeightsMismatch);
    }
    Ok(())
}

const MANIFEST_KEYS: [&str; 15] = [
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
    "signature",
];

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

/// Render a signed artefact as its manifest file: a header line, one `key value` line
/// per field in a fixed order, then the signature. Only a manifest that names its
/// weights file can be written, and what is written always parses back.
pub fn render_manifest_file(artifact: &SignedArtifact) -> Result<String, String> {
    let m = &artifact.manifest;
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
    if artifact.signature.alg() != VELDRIS_MODEL_SIG_ALG {
        return Err(format!(
            "the signature must be {}",
            VELDRIS_MODEL_SIG_ALG.as_str()
        ));
    }
    let yes_no = |b: bool| if b { "yes" } else { "no" };
    Ok(format!(
        "{MANIFEST_FILE_HEADER}\nid {}\nfamily {}\nversion {}\nsha256 {}\nmin-gpus {}\n\
         min-vram-gb {}\nmultinode {}\nlicence {}\nosi-approved {}\nredistribution-ok {}\n\
         weights-file {}\nweights-format {}\nquantisation {}\nweights-size {}\nsignature {} {}\n",
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
        VELDRIS_MODEL_SIG_ALG.as_str(),
        hex(artifact.signature.as_bytes()),
    ))
}

/// Parse a manifest file strictly: the header, every key in order, nothing after the
/// signature. Parsing checks form only — the signature is checked by
/// [`ModelLoader::verify_and_load`].
pub fn parse_manifest_file(text: &str) -> Result<SignedArtifact, String> {
    let mut lines = text.lines().map(|l| l.strip_suffix('\r').unwrap_or(l));
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
    if lines.any(|l| !l.trim().is_empty()) {
        return Err("unexpected lines after the signature".into());
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
    let (alg, signature) = values[14]
        .split_once(' ')
        .ok_or("expected `signature <alg> <hex>`")?;
    if alg != VELDRIS_MODEL_SIG_ALG.as_str() {
        return Err(format!(
            "the signature must be {}, not {alg:?}",
            VELDRIS_MODEL_SIG_ALG.as_str()
        ));
    }
    let signature = unhex(signature)
        .filter(|b| !b.is_empty())
        .ok_or("the signature is not hex")?;

    Ok(SignedArtifact {
        manifest: ModelManifest {
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
        },
        signature: pqc_agility::Signature::new(VELDRIS_MODEL_SIG_ALG, signature),
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
