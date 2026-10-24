//! The artefact registry inside `bifrost-server`: signed, SBOM-attested service builds,
//! verified by the `bifrost-artefact` gate on the way in and again on the way out, and
//! sealed at rest. [FR-B-1] [FR-B-4] [NFR-PQ-3] [RC-12]
//!
//! ## Trust
//! The registry accepts artefacts signed by the release signers whose public keys are
//! in `<state>/trust/release-signers/*.pub` — one or more `<alg> <key-id> <hex>` lines
//! per file, as `mg-sign keygen --export-verifying-key` writes them. An upload names its
//! signer by key id; a key id that is not anchored there is refused as an unknown
//! signer. Bifrost manifests are signed with [`BIFROST_SIG_ALG`] (ML-DSA-65) only, so an
//! anchor for any other algorithm refuses the start rather than every upload. With no
//! anchors the registry still starts, and refuses everything.
//!
//! ## Uploads
//! A built service is several MB and a mesh frame carries at most 1 MiB, so an upload
//! is three steps over one PQ-mTLS session: [`Registry::begin`] with the signed
//! manifest, the SBOM and the signature; [`Registry::chunk`] for each slice of up to
//! [`CHUNK_BYTES`], in order; then the commit, where the whole artefact goes through the
//! gate ([`Registry::gate`]) and is stored ([`Registry::admit`]) or refused and recorded
//! ([`Registry::reject`]). An upload belongs to the identity that began it, lives in
//! memory only, and is dropped after [`UPLOAD_IDLE_SECS`] without a chunk.
//!
//! ## At rest
//! Under `<state>/registry/`:
//! - `artefacts/<hex(id\0version)>.sealed` — one file per verified artefact (manifest,
//!   SBOM, signature, bytes, signer, who registered it and when), sealed with the
//!   service's data sealer (AES-256-GCM under an ML-KEM-1024-wrapped key, the sealer
//!   `DurableFleet` uses) and bound by its AAD to its id and version, so a file renamed
//!   onto another artefact does not open. Written atomically (temp, fsync, rename).
//! - `rejected.journal` — refusals, a sealed append-only journal framed and
//!   sequence-bound like the fleet journal. Rejected uploads keep their metadata and
//!   reason; their bytes are never written.
//!
//! On open every artefact is unsealed and re-verified. A file that does not unseal or
//! decode fails the start closed. One that unseals but no longer verifies — its
//! signer's anchor has been removed — stays listed, as `unverifiable`, and is never
//! served.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use attest::TrustAnchors;
use audit_writer::Sealer;
use bifrost_artefact::{
    ArtefactManifest, ArtefactRef, SignedArtefact, VerifyError, BIFROST_SIG_ALG,
};
use pqc_agility::{SigId, Signature};

/// The largest slice of artefact bytes one upload chunk or one download reply carries.
/// Base64 makes it about 683 KiB, under the transport's 1 MiB frame.
pub const CHUNK_BYTES: usize = 512 * 1024;
/// The largest artefact the registry accepts.
pub const MAX_ARTEFACT_BYTES: u64 = 128 * 1024 * 1024;
/// The largest SBOM the registry accepts; it travels whole in the first request.
pub const MAX_SBOM_BYTES: usize = 640 * 1024;
/// Uploads in flight at once, and the bytes they may declare between them.
pub const MAX_UPLOADS: usize = 8;
/// The bytes all uploads in flight may declare between them.
pub const MAX_PENDING_BYTES: u64 = 256 * 1024 * 1024;
/// An upload with no chunk for this long is dropped.
pub const UPLOAD_IDLE_SECS: u64 = 900;

const ARTEFACT_DS: &[u8] = b"midgard-bifrost-registry-artefact-v1";
const REJECTED_DS: &[u8] = b"midgard-bifrost-registry-rejected-v1";
const RECORD_FORMAT: u8 = 1;
const SEALED_EXT: &str = "sealed";

/// Why the gate refused an artefact. Each is recorded and listed as rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The upload names a signer whose key is not anchored in `trust/release-signers`.
    UnknownSigner,
    /// The ML-DSA signature over the manifest does not verify under the signer's key.
    BadSignature,
    /// The SBOM does not hash to the digest in the signed manifest.
    SbomMismatch,
    /// The SBOM is the signed one but is not usable CycloneDX 1.5 with purls.
    SbomInvalid,
    /// The bytes do not hash to the digest in the signed manifest.
    DigestMismatch,
}

impl Refusal {
    /// A stable code for the reason, as listed.
    pub fn code(self) -> &'static str {
        match self {
            Refusal::UnknownSigner => "unknown-signer",
            Refusal::BadSignature => "bad-signature",
            Refusal::SbomMismatch => "sbom-mismatch",
            Refusal::SbomInvalid => "sbom-invalid",
            Refusal::DigestMismatch => "digest-mismatch",
        }
    }

    /// The reason in words.
    pub fn detail(self) -> &'static str {
        match self {
            Refusal::UnknownSigner => "the signer is not an anchored release signer",
            Refusal::BadSignature => "the ML-DSA signature does not verify under the signer's key",
            Refusal::SbomMismatch => "the SBOM does not match the digest in the signed manifest",
            Refusal::SbomInvalid => "the SBOM is not CycloneDX 1.5 with a purl on every component",
            Refusal::DigestMismatch => "the bytes do not match the digest in the signed manifest",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        [
            Refusal::UnknownSigner,
            Refusal::BadSignature,
            Refusal::SbomMismatch,
            Refusal::SbomInvalid,
            Refusal::DigestMismatch,
        ]
        .into_iter()
        .find(|r| r.code() == code)
    }
}

impl From<VerifyError> for Refusal {
    fn from(e: VerifyError) -> Self {
        match e {
            VerifyError::PayloadDigestMismatch => Refusal::DigestMismatch,
            VerifyError::SbomDigestMismatch => Refusal::SbomMismatch,
            VerifyError::SbomInvalid => Refusal::SbomInvalid,
            VerifyError::BadSignature => Refusal::BadSignature,
        }
    }
}

/// Why a registry call did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The request does not make sense: a bad field, an out-of-order chunk, a commit
    /// before every byte has arrived.
    Malformed(String),
    /// No such artefact or upload (for this identity).
    NotFound(String),
    /// That id and version are already registered.
    Conflict(String),
    /// The gate refused it, for this reason.
    Refused(Refusal),
    /// The artefact is held but does not verify now, so it is not served.
    Unverifiable(String),
    /// Too many uploads in flight.
    Busy(String),
    /// Reading or writing the store failed; nothing changed.
    Io(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Malformed(m)
            | StoreError::NotFound(m)
            | StoreError::Conflict(m)
            | StoreError::Unverifiable(m)
            | StoreError::Busy(m)
            | StoreError::Io(m) => write!(f, "{m}"),
            StoreError::Refused(r) => write!(f, "refused ({}): {}", r.code(), r.detail()),
        }
    }
}

/// A verified artefact as the registry lists it. The bytes stay sealed on disk.
#[derive(Debug, Clone)]
pub struct Record {
    /// The signed manifest.
    pub manifest: ArtefactManifest,
    /// The key id of the release signer that signed it.
    pub signer: String,
    /// The payload size in bytes.
    pub size: u64,
    /// Who registered it (their mesh identity).
    pub registered_by: String,
    /// When, in Unix seconds.
    pub registered_at: u64,
    /// `Err(reason)` when it failed re-verification at start: listed, never served.
    pub state: Result<(), String>,
}

/// A refused upload: its claimed identity and the reason, never its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    /// Claimed artefact id.
    pub id: String,
    /// Claimed version.
    pub version: String,
    /// Claimed component kind.
    pub component: String,
    /// The signer key id the upload named.
    pub signer: String,
    /// The payload digest the manifest claimed, hex.
    pub payload_sha256: String,
    /// Why.
    pub reason: Refusal,
    /// Who uploaded it.
    pub by: String,
    /// When, in Unix seconds.
    pub at: u64,
}

impl Rejection {
    /// The rejection of `upload`, by `by` at `at`.
    pub fn of(upload: &Upload, reason: Refusal, by: &str, at: u64) -> Self {
        Self {
            id: upload.manifest.id.clone(),
            version: upload.manifest.version.clone(),
            component: upload.manifest.component.clone(),
            signer: upload.signer.clone(),
            payload_sha256: attest::hex::hex(&upload.manifest.payload_sha256),
            reason,
            by: by.to_string(),
            at,
        }
    }
}

/// What a client declares when it begins an upload.
#[derive(Debug, Clone)]
pub struct NewUpload {
    /// The signed manifest.
    pub manifest: ArtefactManifest,
    /// The CycloneDX SBOM the manifest's `sbom_sha256` commits to.
    pub sbom: String,
    /// The signature over the manifest.
    pub signature: Signature,
    /// The signing key's id.
    pub signer: String,
    /// How many payload bytes will follow.
    pub size: u64,
}

/// An upload in flight.
#[derive(Debug)]
pub struct Upload {
    owner: String,
    /// The signed manifest.
    pub manifest: ArtefactManifest,
    sbom: String,
    signature: Signature,
    /// The signing key's id.
    pub signer: String,
    size: u64,
    bytes: Vec<u8>,
    touched: u64,
}

impl Upload {
    /// The artefact this upload carries, once every byte has arrived.
    pub fn artefact(&self) -> SignedArtefact {
        SignedArtefact {
            manifest: self.manifest.clone(),
            sbom: self.sbom.clone(),
            payload: self.bytes.clone(),
            signature: self.signature.clone(),
        }
    }
}

/// The registry: verified records, refusals, uploads in flight, and the sealed store.
pub struct Registry {
    dir: PathBuf,
    sealer: Box<dyn Sealer>,
    signers: TrustAnchors,
    records: BTreeMap<ArtefactRef, Record>,
    rejected: Vec<Rejection>,
    journal: File,
    uploads: BTreeMap<String, Upload>,
    /// The artefact most recently verified for a download, so a download's later
    /// chunks come from the copy its first chunk verified.
    cache: Option<(ArtefactRef, Arc<SignedArtefact>)>,
}

/// Read the anchored release signers from every `*.pub` file in `dir`. A missing
/// directory anchors nobody. A malformed line, a key id anchored to two different keys,
/// or a key for an algorithm other than [`BIFROST_SIG_ALG`] is an error.
pub fn load_signers(dir: &Path) -> Result<TrustAnchors, String> {
    let mut anchors = TrustAnchors::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(anchors),
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("pub"))
        .collect();
    files.sort();
    for path in files {
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let found = TrustAnchors::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some((id, key)) = found.iter().find(|(_, k)| k.alg() != BIFROST_SIG_ALG) {
            return Err(format!(
                "{}: signer {id:?} is a {} key, but Bifrost artefacts are signed with {} only",
                path.display(),
                key.alg().as_str(),
                BIFROST_SIG_ALG.as_str()
            ));
        }
        anchors
            .merge(found)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(anchors)
}

impl Registry {
    /// Open (creating if absent) the registry under `dir`, trusting `signers`, sealing
    /// with `sealer`. Every stored artefact is unsealed and re-verified.
    pub fn open(
        dir: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
        signers: TrustAnchors,
    ) -> Result<Self, String> {
        let dir = dir.as_ref().to_path_buf();
        let artefacts = dir.join("artefacts");
        create_private_dir(&artefacts)?;

        let mut records = BTreeMap::new();
        let mut names: Vec<PathBuf> = fs::read_dir(&artefacts)
            .map_err(|e| format!("cannot read {}: {e}", artefacts.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        names.sort();
        for path in names {
            match path.extension().and_then(|x| x.to_str()) {
                Some(SEALED_EXT) => {}
                // A write that never reached its rename: not an artefact.
                Some("tmp") => {
                    let _ = fs::remove_file(&path);
                    continue;
                }
                _ => continue,
            }
            let (key, stored) = read_artefact(sealer.as_ref(), &path)?;
            let state = gate(&signers, &stored.signer, &stored.artefact)
                .map_err(|r| format!("{} ({})", r.detail(), r.code()));
            records.insert(
                key,
                Record {
                    size: stored.artefact.payload.len() as u64,
                    manifest: stored.artefact.manifest,
                    signer: stored.signer,
                    registered_by: stored.by,
                    registered_at: stored.at,
                    state,
                },
            );
        }

        let journal_path = dir.join("rejected.journal");
        let rejected = read_rejections(sealer.as_ref(), &journal_path)?;
        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let journal = opts
            .open(&journal_path)
            .map_err(|e| format!("cannot open {}: {e}", journal_path.display()))?;

        Ok(Self {
            dir,
            sealer,
            signers,
            records,
            rejected,
            journal,
            uploads: BTreeMap::new(),
            cache: None,
        })
    }

    /// The anchored signers' key ids.
    pub fn signer_ids(&self) -> Vec<String> {
        self.signers.iter().map(|(id, _)| id.to_string()).collect()
    }

    /// Every verified (or no longer verifiable) artefact, by id then version.
    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.records.values()
    }

    /// Every refusal, oldest first.
    pub fn rejections(&self) -> &[Rejection] {
        &self.rejected
    }

    /// The record for `id`@`version`.
    pub fn record(&self, id: &str, version: &str) -> Option<&Record> {
        self.records.get(&ArtefactRef::new(id, version))
    }

    /// Begin an upload for `owner` under `upload_id`. An unknown signer is refused here,
    /// before any bytes are sent; the caller records the refusal with [`Self::reject`].
    pub fn begin(
        &mut self,
        owner: &str,
        upload_id: String,
        new: NewUpload,
        now: u64,
    ) -> Result<(), (StoreError, Option<Box<Upload>>)> {
        self.uploads
            .retain(|_, u| now.saturating_sub(u.touched) < UPLOAD_IDLE_SECS);
        let malformed = |m: String| (StoreError::Malformed(m), None);
        check_name(&new.manifest.id, "id").map_err(malformed)?;
        check_name(&new.manifest.version, "version").map_err(malformed)?;
        check_name(&new.manifest.component, "component").map_err(malformed)?;
        if new.signer.is_empty() || new.signer.len() > 128 {
            return Err(malformed("signer must be a key id".into()));
        }
        if new.size > MAX_ARTEFACT_BYTES {
            return Err(malformed(format!(
                "an artefact may be at most {MAX_ARTEFACT_BYTES} bytes"
            )));
        }
        if new.sbom.len() > MAX_SBOM_BYTES {
            return Err(malformed(format!(
                "an SBOM may be at most {MAX_SBOM_BYTES} bytes"
            )));
        }
        let key = ArtefactRef::new(new.manifest.id.clone(), new.manifest.version.clone());
        if self.records.contains_key(&key) {
            return Err((
                StoreError::Conflict(format!("{}@{} is already registered", key.id, key.version)),
                None,
            ));
        }
        let upload = Upload {
            owner: owner.to_string(),
            manifest: new.manifest,
            sbom: new.sbom,
            signature: new.signature,
            signer: new.signer,
            size: new.size,
            bytes: Vec::new(),
            touched: now,
        };
        if self.signers.get(&upload.signer).is_none() {
            return Err((
                StoreError::Refused(Refusal::UnknownSigner),
                Some(Box::new(upload)),
            ));
        }
        let pending: u64 = self.uploads.values().map(|u| u.size).sum();
        if self.uploads.len() >= MAX_UPLOADS || pending + upload.size > MAX_PENDING_BYTES {
            return Err((
                StoreError::Busy("too many uploads in flight; try again later".into()),
                None,
            ));
        }
        self.uploads.insert(upload_id, upload);
        Ok(())
    }

    /// Append `data` at `offset` to `owner`'s upload. Chunks arrive in order. Returns
    /// the bytes received so far.
    pub fn chunk(
        &mut self,
        owner: &str,
        upload_id: &str,
        offset: u64,
        data: &[u8],
        now: u64,
    ) -> Result<u64, StoreError> {
        let upload = self.upload_mut(owner, upload_id)?;
        if data.len() > CHUNK_BYTES {
            return Err(StoreError::Malformed(format!(
                "a chunk may carry at most {CHUNK_BYTES} bytes"
            )));
        }
        let have = upload.bytes.len() as u64;
        if offset != have {
            return Err(StoreError::Malformed(format!(
                "expected the chunk at offset {have}, not {offset}"
            )));
        }
        if have + data.len() as u64 > upload.size {
            return Err(StoreError::Malformed(format!(
                "the upload declared {} bytes",
                upload.size
            )));
        }
        upload.bytes.extend_from_slice(data);
        upload.touched = now;
        Ok(upload.bytes.len() as u64)
    }

    /// Take `owner`'s complete upload out for the commit. An incomplete one stays.
    pub fn take(&mut self, owner: &str, upload_id: &str) -> Result<Upload, StoreError> {
        let upload = self.upload_mut(owner, upload_id)?;
        if upload.bytes.len() as u64 != upload.size {
            return Err(StoreError::Malformed(format!(
                "{} of {} bytes have arrived",
                upload.bytes.len(),
                upload.size
            )));
        }
        Ok(self.uploads.remove(upload_id).expect("just found"))
    }

    /// The gate: verify `upload` against its named signer's anchored key.
    pub fn gate(&self, upload: &Upload) -> Result<(), Refusal> {
        gate(&self.signers, &upload.signer, &upload.artefact())
    }

    /// Record a refusal durably. Its bytes are never written.
    pub fn reject(&mut self, rejection: Rejection) -> Result<(), StoreError> {
        let seq = self.rejected.len() as u64;
        let sealed = self
            .sealer
            .seal(&rejected_aad(seq), &encode_rejection(&rejection))
            .map_err(|e| StoreError::Io(format!("cannot seal the refusal: {e}")))?;
        let mut frame = Vec::with_capacity(4 + sealed.len());
        frame.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
        frame.extend_from_slice(&sealed);
        self.journal
            .write_all(&frame)
            .and_then(|_| self.journal.sync_all())
            .map_err(|e| StoreError::Io(format!("cannot record the refusal: {e}")))?;
        self.rejected.push(rejection);
        Ok(())
    }

    /// Store a gate-verified upload, sealed. Call only after [`Self::gate`] passed.
    pub fn admit(&mut self, upload: Upload, by: &str, now: u64) -> Result<Record, StoreError> {
        let key = ArtefactRef::new(upload.manifest.id.clone(), upload.manifest.version.clone());
        if self.records.contains_key(&key) {
            return Err(StoreError::Conflict(format!(
                "{}@{} is already registered",
                key.id, key.version
            )));
        }
        let stored = Stored {
            signer: upload.signer.clone(),
            by: by.to_string(),
            at: now,
            artefact: upload.artefact(),
        };
        let sealed = self
            .sealer
            .seal(&artefact_aad(&key), &encode_stored(&stored))
            .map_err(|e| StoreError::Io(format!("cannot seal the artefact: {e}")))?;
        atomic_write(&self.artefact_path(&key), &sealed)
            .map_err(|e| StoreError::Io(format!("cannot store the artefact: {e}")))?;
        let record = Record {
            manifest: upload.manifest,
            signer: upload.signer,
            size: upload.size,
            registered_by: by.to_string(),
            registered_at: now,
            state: Ok(()),
        };
        self.records.insert(key, record.clone());
        Ok(record)
    }

    /// Fetch `id`@`version` and verify it before handing it out. `fresh` reads and
    /// re-verifies the sealed copy on disk (every download starts so); otherwise the
    /// copy the last fresh read verified may be used, so a download's chunks all come
    /// from one verified artefact.
    pub fn fetch(
        &mut self,
        id: &str,
        version: &str,
        fresh: bool,
    ) -> Result<(Record, Arc<SignedArtefact>), StoreError> {
        let key = ArtefactRef::new(id, version);
        let record = self
            .records
            .get(&key)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("no artefact {id}@{version}")))?;
        if let Err(reason) = &record.state {
            return Err(StoreError::Unverifiable(format!(
                "{id}@{version} does not verify: {reason}"
            )));
        }
        if !fresh {
            if let Some((cached, artefact)) = &self.cache {
                if *cached == key {
                    return Ok((record, artefact.clone()));
                }
            }
        }
        let (_, stored) = read_artefact(self.sealer.as_ref(), &self.artefact_path(&key))
            .map_err(StoreError::Unverifiable)?;
        if stored.artefact.manifest != record.manifest || stored.signer != record.signer {
            return Err(StoreError::Unverifiable(format!(
                "{id}@{version} at rest is not the artefact that was registered"
            )));
        }
        gate(&self.signers, &stored.signer, &stored.artefact).map_err(|r| {
            StoreError::Unverifiable(format!(
                "{id}@{version} does not verify: {} ({})",
                r.detail(),
                r.code()
            ))
        })?;
        let artefact = Arc::new(stored.artefact);
        self.cache = Some((key, artefact.clone()));
        Ok((record, artefact))
    }

    fn upload_mut(&mut self, owner: &str, upload_id: &str) -> Result<&mut Upload, StoreError> {
        match self.uploads.get_mut(upload_id) {
            Some(u) if u.owner == owner => Ok(u),
            _ => Err(StoreError::NotFound(format!(
                "no upload {upload_id} in flight for {owner}"
            ))),
        }
    }

    fn artefact_path(&self, key: &ArtefactRef) -> PathBuf {
        let mut raw = key.id.clone().into_bytes();
        raw.push(0);
        raw.extend_from_slice(key.version.as_bytes());
        self.dir
            .join("artefacts")
            .join(format!("{}.{SEALED_EXT}", attest::hex::hex(&raw)))
    }
}

fn gate(signers: &TrustAnchors, signer: &str, artefact: &SignedArtefact) -> Result<(), Refusal> {
    let key = signers.get(signer).ok_or(Refusal::UnknownSigner)?;
    bifrost_artefact::verify(key, artefact).map_err(Refusal::from)
}

/// Ids, versions and component kinds: 1 to 128 of letters, digits, `.`, `_`, `-`, `+`.
fn check_name(value: &str, what: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
    {
        return Err(format!(
            "{what} must be 1 to 128 letters, digits, '.', '_', '-' or '+'"
        ));
    }
    Ok(())
}

fn create_private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for d in [dir, dir.parent().unwrap_or(dir)] {
            fs::set_permissions(d, fs::Permissions::from_mode(0o700))
                .map_err(|e| format!("cannot restrict {}: {e}", d.display()))?;
        }
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut opts = OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    if let Some(parent) = path.parent() {
        // Make the rename itself durable.
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn artefact_aad(key: &ArtefactRef) -> Vec<u8> {
    let mut a = ARTEFACT_DS.to_vec();
    put_bytes(&mut a, key.id.as_bytes());
    put_bytes(&mut a, key.version.as_bytes());
    a
}

fn rejected_aad(seq: u64) -> Vec<u8> {
    let mut a = REJECTED_DS.to_vec();
    a.extend_from_slice(&seq.to_le_bytes());
    a
}

// --- codecs (no serde; length-prefixed, little-endian) ----------------------------

struct Stored {
    signer: String,
    by: String,
    at: u64,
    artefact: SignedArtefact,
}

fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

fn encode_stored(s: &Stored) -> Vec<u8> {
    let a = &s.artefact;
    let mut o = Vec::with_capacity(a.payload.len() + a.sbom.len() + 8192);
    o.push(RECORD_FORMAT);
    put_bytes(&mut o, s.signer.as_bytes());
    put_bytes(&mut o, s.by.as_bytes());
    o.extend_from_slice(&s.at.to_le_bytes());
    put_bytes(&mut o, a.manifest.id.as_bytes());
    put_bytes(&mut o, a.manifest.version.as_bytes());
    put_bytes(&mut o, a.manifest.component.as_bytes());
    o.extend_from_slice(&a.manifest.payload_sha256);
    o.extend_from_slice(&a.manifest.sbom_sha256);
    put_bytes(&mut o, a.sbom.as_bytes());
    put_bytes(&mut o, &a.payload);
    o.push(a.signature.alg().as_u8());
    put_bytes(&mut o, a.signature.as_bytes());
    o
}

fn encode_rejection(r: &Rejection) -> Vec<u8> {
    let mut o = vec![RECORD_FORMAT];
    for field in [
        &r.id,
        &r.version,
        &r.component,
        &r.signer,
        &r.payload_sha256,
        r.reason.code(),
        &r.by,
    ] {
        put_bytes(&mut o, field.as_bytes());
    }
    o.extend_from_slice(&r.at.to_le_bytes());
    o
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or("record truncated")?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u64(&mut self) -> Result<u64, String> {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(a))
    }
    fn arr32(&mut self) -> Result<[u8; 32], String> {
        let mut a = [0u8; 32];
        a.copy_from_slice(self.take(32)?);
        Ok(a)
    }
    fn bytes(&mut self) -> Result<Vec<u8>, String> {
        let mut n = [0u8; 4];
        n.copy_from_slice(self.take(4)?);
        Ok(self.take(u32::from_le_bytes(n) as usize)?.to_vec())
    }
    fn string(&mut self) -> Result<String, String> {
        String::from_utf8(self.bytes()?).map_err(|_| "record holds invalid UTF-8".to_string())
    }
    fn end(&self) -> Result<(), String> {
        if self.pos == self.b.len() {
            Ok(())
        } else {
            Err("record has trailing bytes".into())
        }
    }
}

fn read_artefact(sealer: &dyn Sealer, path: &Path) -> Result<(ArtefactRef, Stored), String> {
    let what = path.display();
    let raw = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(attest::hex::unhex)
        .ok_or_else(|| format!("{what}: not an artefact file name"))?;
    let split = raw
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| format!("{what}: not an artefact file name"))?;
    let key = match (
        String::from_utf8(raw[..split].to_vec()),
        String::from_utf8(raw[split + 1..].to_vec()),
    ) {
        (Ok(id), Ok(version)) => ArtefactRef::new(id, version),
        _ => return Err(format!("{what}: not an artefact file name")),
    };
    let sealed = fs::read(path).map_err(|e| format!("cannot read {what}: {e}"))?;
    let plain = sealer
        .unseal(&artefact_aad(&key), &sealed)
        .map_err(|e| format!("{what} does not unseal (tampered, or another artefact's): {e}"))?;
    let stored = decode_stored(&plain).map_err(|e| format!("{what}: {e}"))?;
    if stored.artefact.manifest.id != key.id || stored.artefact.manifest.version != key.version {
        return Err(format!("{what} holds another artefact"));
    }
    Ok((key, stored))
}

fn decode_stored(b: &[u8]) -> Result<Stored, String> {
    let mut r = Reader { b, pos: 0 };
    if r.u8()? != RECORD_FORMAT {
        return Err("unknown record format".into());
    }
    let signer = r.string()?;
    let by = r.string()?;
    let at = r.u64()?;
    let manifest = ArtefactManifest {
        id: r.string()?,
        version: r.string()?,
        component: r.string()?,
        payload_sha256: r.arr32()?,
        sbom_sha256: r.arr32()?,
    };
    let sbom = r.string()?;
    let payload = r.bytes()?;
    let alg = SigId::from_u8(r.u8()?).ok_or("unknown signature algorithm")?;
    let signature = Signature::new(alg, r.bytes()?);
    r.end()?;
    Ok(Stored {
        signer,
        by,
        at,
        artefact: SignedArtefact {
            manifest,
            sbom,
            payload,
            signature,
        },
    })
}

fn read_rejections(sealer: &dyn Sealer, path: &Path) -> Result<Vec<Rejection>, String> {
    let mut buf = Vec::new();
    match File::open(path) {
        Ok(mut f) => {
            f.read_to_end(&mut buf)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    }
    let what = path.display();
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        let len = buf
            .get(pos..pos + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| format!("{what}: truncated frame header"))?;
        pos += 4;
        let end = pos
            .checked_add(len)
            .filter(|&e| e <= buf.len())
            .ok_or_else(|| format!("{what}: truncated frame"))?;
        let plain = sealer
            .unseal(&rejected_aad(out.len() as u64), &buf[pos..end])
            .map_err(|e| format!("{what} does not unseal: {e}"))?;
        pos = end;
        out.push(decode_rejection(&plain).map_err(|e| format!("{what}: {e}"))?);
    }
    Ok(out)
}

fn decode_rejection(b: &[u8]) -> Result<Rejection, String> {
    let mut r = Reader { b, pos: 0 };
    if r.u8()? != RECORD_FORMAT {
        return Err("unknown record format".into());
    }
    let id = r.string()?;
    let version = r.string()?;
    let component = r.string()?;
    let signer = r.string()?;
    let payload_sha256 = r.string()?;
    let reason = Refusal::from_code(&r.string()?).ok_or("unknown refusal reason")?;
    let by = r.string()?;
    let at = r.u64()?;
    r.end()?;
    Ok(Rejection {
        id,
        version,
        component,
        signer,
        payload_sha256,
        reason,
        by,
        at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use audit_writer::AuditError;
    use bifrost_artefact::{build_and_sign, generate_sbom, SbomComponent};
    use pqc_agility::backend::fips::FipsSig;
    use pqc_agility::hash::sha256;
    use pqc_agility::{OsRng, SignatureScheme, SigningKey, VerifyingKey};

    /// An authenticating test sealer: a tag over the AAD and the plaintext, and the
    /// plaintext reversed so nothing is stored as written.
    struct TestSealer;
    impl Sealer for TestSealer {
        fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
            let mut tagged = [aad, plaintext].concat();
            let tag = sha256(&tagged);
            tagged.clear();
            tagged.extend_from_slice(&tag);
            tagged.extend(plaintext.iter().rev());
            Ok(tagged)
        }
        fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
            let (tag, body) = sealed.split_at(32.min(sealed.len()));
            let plain: Vec<u8> = body.iter().rev().copied().collect();
            if sha256(&[aad, &plain].concat()) != tag {
                return Err(AuditError::Store("tampered".into()));
            }
            Ok(plain)
        }
    }

    fn keys() -> (VerifyingKey, SigningKey) {
        FipsSig::new(BIFROST_SIG_ALG).keypair(&mut OsRng).unwrap()
    }

    fn scratch(tag: &str) -> PathBuf {
        let uniq = Box::into_raw(Box::new(0u8)) as usize;
        let dir = std::env::temp_dir().join(format!(
            "bifrost-registry-{tag}-{}-{uniq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn anchors(vk: &VerifyingKey) -> TrustAnchors {
        let mut a = TrustAnchors::new();
        a.insert("release-1", vk.clone());
        a
    }

    fn signed(sk: &SigningKey, payload: Vec<u8>) -> SignedArtefact {
        let sbom = generate_sbom(
            "forge-worker",
            "0.1.0",
            "service",
            vec![SbomComponent::new("pqc-agility", "0.0.0")],
        );
        build_and_sign("forge-worker", "0.1.0", "service", payload, sbom, sk)
    }

    fn upload(reg: &mut Registry, a: &SignedArtefact, signer: &str) -> Result<Upload, StoreError> {
        let new = NewUpload {
            manifest: a.manifest.clone(),
            sbom: a.sbom.clone(),
            signature: a.signature.clone(),
            signer: signer.into(),
            size: a.payload.len() as u64,
        };
        reg.begin("ci", "u1".into(), new, 10).map_err(|(e, _)| e)?;
        let mut offset = 0u64;
        for piece in a.payload.chunks(CHUNK_BYTES) {
            offset = reg.chunk("ci", "u1", offset, piece, 11)?;
        }
        reg.take("ci", "u1")
    }

    #[test]
    fn a_good_artefact_is_admitted_sealed_and_survives_a_reopen() {
        let dir = scratch("good");
        let (vk, sk) = keys();
        let payload: Vec<u8> = (0..CHUNK_BYTES * 2 + 5)
            .map(|i| (i * 31 % 251) as u8)
            .collect();
        let a = signed(&sk, payload);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        let up = upload(&mut reg, &a, "release-1").unwrap();
        assert_eq!(reg.gate(&up), Ok(()));
        reg.admit(up, "ci", 12).unwrap();

        let files: Vec<_> = fs::read_dir(dir.join("artefacts")).unwrap().collect();
        assert_eq!(files.len(), 1);
        let on_disk = fs::read(files[0].as_ref().unwrap().path()).unwrap();
        assert!(
            !on_disk.windows(64).any(|w| w == &a.payload[..64]),
            "the bytes are not stored in the clear"
        );

        drop(reg);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        let (record, got) = reg.fetch("forge-worker", "0.1.0", true).unwrap();
        assert_eq!(record.registered_by, "ci");
        assert_eq!(got.payload, a.payload);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refusals_are_recorded_with_their_reason_and_survive_a_reopen() {
        let dir = scratch("refused");
        let (vk, sk) = keys();
        let (_, other) = keys();
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();

        let mut tampered = signed(&sk, vec![1u8; 100]);
        tampered.payload[0] ^= 1;
        let forged = signed(&other, vec![1u8; 100]);
        let mut bad_sbom = signed(&sk, vec![1u8; 100]);
        bad_sbom.sbom.push(' ');
        for (a, want) in [
            (tampered, Refusal::DigestMismatch),
            (forged, Refusal::BadSignature),
            (bad_sbom, Refusal::SbomMismatch),
        ] {
            let up = upload(&mut reg, &a, "release-1").unwrap();
            assert_eq!(reg.gate(&up), Err(want));
            reg.reject(Rejection::of(&up, want, "ci", 20)).unwrap();
        }
        let good = signed(&sk, vec![1u8; 100]);
        match upload(&mut reg, &good, "nobody") {
            Err(StoreError::Refused(Refusal::UnknownSigner)) => {}
            other => panic!("{other:?}"),
        }
        assert_eq!(reg.records().count(), 0);

        drop(reg);
        let reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        let reasons: Vec<_> = reg.rejections().iter().map(|r| r.reason).collect();
        assert_eq!(
            reasons,
            [
                Refusal::DigestMismatch,
                Refusal::BadSignature,
                Refusal::SbomMismatch
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tampered_store_fails_the_open_and_a_removed_signer_is_never_served() {
        let dir = scratch("at-rest");
        let (vk, sk) = keys();
        let a = signed(&sk, vec![3u8; 1000]);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        let up = upload(&mut reg, &a, "release-1").unwrap();
        reg.admit(up, "ci", 1).unwrap();
        drop(reg);

        // The signer's anchor is withdrawn: listed, not served.
        let mut reg = Registry::open(&dir, Box::new(TestSealer), TrustAnchors::new()).unwrap();
        assert!(reg.record("forge-worker", "0.1.0").unwrap().state.is_err());
        assert!(matches!(
            reg.fetch("forge-worker", "0.1.0", true),
            Err(StoreError::Unverifiable(_))
        ));
        drop(reg);

        // One flipped byte at rest: the start fails closed.
        let path = fs::read_dir(dir.join("artefacts"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut bytes = fs::read(&path).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 1;
        fs::write(&path, bytes).unwrap();
        assert!(Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn uploads_are_ordered_owned_bounded_and_unique() {
        let dir = scratch("uploads");
        let (vk, sk) = keys();
        let a = signed(&sk, vec![5u8; 10]);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        let new = || NewUpload {
            manifest: a.manifest.clone(),
            sbom: a.sbom.clone(),
            signature: a.signature.clone(),
            signer: "release-1".into(),
            size: 10,
        };
        reg.begin("ci", "u".into(), new(), 0).unwrap();
        assert!(matches!(
            reg.chunk("ci", "u", 4, &[5; 2], 0),
            Err(StoreError::Malformed(_))
        ));
        assert!(matches!(
            reg.chunk("someone-else", "u", 0, &[5; 2], 0),
            Err(StoreError::NotFound(_))
        ));
        reg.chunk("ci", "u", 0, &[5; 4], 0).unwrap();
        assert!(matches!(reg.take("ci", "u"), Err(StoreError::Malformed(_))));
        assert!(matches!(
            reg.chunk("ci", "u", 4, &[5; 7], 0),
            Err(StoreError::Malformed(_))
        ));
        reg.chunk("ci", "u", 4, &[5; 6], 0).unwrap();
        let up = reg.take("ci", "u").unwrap();
        reg.admit(up, "ci", 0).unwrap();
        assert!(matches!(
            reg.begin("ci", "v".into(), new(), 0),
            Err((StoreError::Conflict(_), None))
        ));

        let mut big = new();
        big.manifest.version = "0.2.0".into();
        big.size = MAX_ARTEFACT_BYTES + 1;
        assert!(matches!(
            reg.begin("ci", "w".into(), big, 0),
            Err((StoreError::Malformed(_), None))
        ));

        // An idle upload is dropped.
        let mut later = new();
        later.manifest.version = "0.3.0".into();
        reg.begin("ci", "x".into(), later, 100).unwrap();
        let mut other = new();
        other.manifest.version = "0.4.0".into();
        reg.begin("ci", "y".into(), other, 100 + UPLOAD_IDLE_SECS)
            .unwrap();
        assert!(matches!(
            reg.chunk("ci", "x", 0, &[5; 1], 100 + UPLOAD_IDLE_SECS),
            Err(StoreError::NotFound(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn signers_are_read_from_pub_files_and_only_ml_dsa_65_is_anchored() {
        let dir = scratch("signers");
        assert!(load_signers(&dir.join("absent")).unwrap().is_empty());
        let (vk, _) = keys();
        let line = |alg: &str, id: &str, key: &VerifyingKey| {
            format!("{alg} {id} {}\n", attest::hex::hex(key.as_bytes()))
        };
        fs::write(dir.join("release.pub"), line("ML-DSA-65", "release-1", &vk)).unwrap();
        fs::write(dir.join("notes.txt"), "not a key").unwrap();
        let got = load_signers(&dir).unwrap();
        assert_eq!(got.len(), 1);
        assert!(got.get("release-1").is_some());

        let (vk87, _) = FipsSig::new(SigId::MlDsa87).keypair(&mut OsRng).unwrap();
        fs::write(dir.join("old.pub"), line("ML-DSA-87", "release-0", &vk87)).unwrap();
        assert!(load_signers(&dir).unwrap_err().contains("ML-DSA-65 only"));
        let _ = fs::remove_dir_all(&dir);
    }
}
