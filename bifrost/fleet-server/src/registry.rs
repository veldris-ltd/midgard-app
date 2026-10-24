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
//! memory only (its bytes are reserved once, on the first chunk, and become the stored
//! artefact without a copy), and is dropped after [`UPLOAD_IDLE_SECS`] without a chunk.
//! An identity may have [`MAX_UPLOADS_PER_OWNER`] in flight.
//!
//! ## Frame budget
//! The transport frame is 1 MiB (1,048,576 bytes). Every message that carries bytes —
//! an upload's SBOM and chunks, a download's slices of the SBOM or the payload — carries
//! at most [`CHUNK_BYTES`] (512 KiB) of them, as base64: 699,052 bytes. The rest of such
//! a message is the manifest (ids, versions and kinds are at most 128 characters from a
//! set JSON never escapes, digests are hex), a signer id of the same kind, and the
//! signature in hex (6,618 bytes for ML-DSA-65): under 10 KB. No free text travels with
//! the bytes, so JSON escaping cannot inflate a message past the frame.
//!
//! ## At rest
//! Under `<state>/registry/`:
//! - `artefacts/<hex(sha256(id\0version))>.sealed` — one file per verified artefact
//!   (manifest, SBOM, signature, bytes, signer, who registered it and when), sealed with
//!   the service's data sealer (AES-256-GCM under an ML-KEM-1024-wrapped key, the sealer
//!   `DurableFleet` uses) and bound by its AAD to that name, so a file renamed onto
//!   another artefact does not open; the unsealed id and version must hash to it too.
//!   Written atomically (temp, fsync, rename). The name is a fixed 64 characters
//!   whatever the id and version.
//! - `rejected.journal` — refusals, a sealed append-only journal framed and
//!   sequence-bound like the fleet journal. Rejected uploads keep their metadata and
//!   reason; their bytes are never written. At most [`MAX_REJECTIONS`] are kept, and an
//!   identity's unknown-signer refusals — the one refusal that needs no upload — are
//!   kept at most [`UNKNOWN_SIGNER_LIMIT`] per [`UNKNOWN_SIGNER_WINDOW_SECS`]; beyond
//!   either the upload is still refused, just not journalled.
//!
//! On open every artefact is unsealed and re-verified. A file that does not unseal or
//! decode fails the start closed. One that unseals but no longer verifies — its
//! signer's anchor has been removed — stays listed, as `unverifiable`, and is never
//! served. One that fails verification while running is marked so too, and the caller
//! audits it.
//!
//! ## Reads
//! A sealed record is immutable, so the full gate need not run on every read of it.
//! Each record remembers the SHA-256 of the exact sealed bytes the gate last passed on
//! (at the commit, at the open, or since). How hard a read checks is a [`Recheck`]:
//! - [`Recheck::Seal`], a download's first slice of a part: the sealed file is read and
//!   unsealed (the AEAD check) every time. If its bytes are the ones verified, that is
//!   enough; any other bytes, even a valid seal, go through the whole gate again.
//! - [`Recheck::Cached`], a download's later slices: served from a verified copy of
//!   exactly those sealed bytes, from a small LRU ([`CACHE_ENTRIES`] copies, at most
//!   [`CACHE_BYTES`] between them), keyed by id@version; on a miss, as `Seal`.
//! - [`Recheck::Full`], `VerifyArtefact`: the whole gate, always.
//!
//! Nothing is served that has not passed the seal check on this read, or on the read
//! whose verified copy a later slice comes from. The file is read and checked outside
//! the registry's lock ([`Registry::begin_fetch`], [`ReadCheck::run`],
//! [`Registry::finish_fetch`]), so one download does not hold up the others.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use attest::TrustAnchors;
use audit_writer::Sealer;
use bifrost_artefact::{
    ArtefactManifest, ArtefactRef, SignedArtefact, VerifyError, BIFROST_SIG_ALG,
};
use pqc_agility::hash::sha256;
use pqc_agility::{SigId, Signature};

/// The most bytes one message carries: an upload chunk, the SBOM in an upload's first
/// request, or one slice of a download. Base64 makes it 699,052 bytes, under the
/// transport's 1 MiB frame (see the module's frame budget).
pub const CHUNK_BYTES: usize = 512 * 1024;
/// The largest artefact the registry accepts.
pub const MAX_ARTEFACT_BYTES: u64 = 128 * 1024 * 1024;
/// The largest SBOM the registry accepts: it travels whole, as base64, in the upload's
/// first request, so it has a chunk's budget.
pub const MAX_SBOM_BYTES: usize = CHUNK_BYTES;
/// Uploads in flight at once.
pub const MAX_UPLOADS: usize = 8;
/// Uploads one identity may have in flight at once.
pub const MAX_UPLOADS_PER_OWNER: usize = 2;
/// The bytes all uploads in flight may declare between them.
pub const MAX_PENDING_BYTES: u64 = 256 * 1024 * 1024;
/// An upload with no chunk for this long is dropped.
pub const UPLOAD_IDLE_SECS: u64 = 900;
/// The most refusals the journal keeps.
pub const MAX_REJECTIONS: usize = 10_000;
/// The unknown-signer refusals one identity may have journalled per window.
pub const UNKNOWN_SIGNER_LIMIT: usize = 10;
/// The window [`UNKNOWN_SIGNER_LIMIT`] counts over.
pub const UNKNOWN_SIGNER_WINDOW_SECS: u64 = 3600;
/// The listing page size when none is asked for, and the largest one served.
pub const LIST_PAGE: usize = 100;
/// The largest listing page served.
pub const MAX_LIST_PAGE: usize = 500;
/// Verified copies kept for downloads' later slices.
pub const CACHE_ENTRIES: usize = 4;
/// The bytes (payloads and SBOMs) those copies may hold between them. It is twice
/// [`MAX_ARTEFACT_BYTES`], so the largest artefact always fits.
pub const CACHE_BYTES: u64 = 2 * MAX_ARTEFACT_BYTES;

const ARTEFACT_DS: &[u8] = b"midgard-bifrost-registry-artefact-v2";
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
    /// The artefact is held but was already found not to verify, so it is not served.
    Unverifiable(String),
    /// The artefact failed verification just now; it is marked unverifiable and will
    /// not be served again. The caller audits this.
    VerifyFailed(String),
    /// Too many uploads in flight, or no memory for one.
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
            | StoreError::VerifyFailed(m)
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
    /// `Err(reason)` once it has failed re-verification, at start or since: listed,
    /// never served.
    pub state: Result<(), String>,
    /// SHA-256 of the exact sealed bytes the full gate last passed on.
    verified_seal: Option<[u8; 32]>,
}

/// How hard a read checks the artefact it serves (see the module's reads).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recheck {
    /// A verified copy of the sealed bytes last verified, if one is cached; else `Seal`.
    Cached,
    /// Read and unseal the sealed copy now; the whole gate only if its bytes are not the
    /// ones last verified.
    Seal,
    /// Read and unseal the sealed copy now and run the whole gate.
    Full,
}

/// A small LRU of verified copies, keyed by id@version and the digest of the sealed
/// bytes they were verified from, bounded in entries and in bytes.
pub(crate) struct VerifiedCache {
    /// Most recently used first.
    entries: VecDeque<(ArtefactRef, [u8; 32], Arc<SignedArtefact>)>,
    max_entries: usize,
    max_bytes: u64,
    bytes: u64,
}

impl VerifiedCache {
    pub(crate) fn new(max_entries: usize, max_bytes: u64) -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries,
            max_bytes,
            bytes: 0,
        }
    }

    /// What a copy counts against the byte bound: its payload and SBOM.
    pub(crate) fn weight(a: &SignedArtefact) -> u64 {
        (a.payload.len() + a.sbom.len()) as u64
    }

    /// The copy of `key` verified from sealed bytes hashing to `sealed`, now most
    /// recently used.
    pub(crate) fn get(
        &mut self,
        key: &ArtefactRef,
        sealed: &[u8; 32],
    ) -> Option<Arc<SignedArtefact>> {
        let at = self
            .entries
            .iter()
            .position(|(k, s, _)| k == key && s == sealed)?;
        let entry = self.entries.remove(at)?;
        let copy = entry.2.clone();
        self.entries.push_front(entry);
        Some(copy)
    }

    /// Keep `copy` as `key`'s, replacing any other, and drop the least recently used
    /// past either bound. A copy larger than the byte bound is not kept.
    pub(crate) fn insert(&mut self, key: ArtefactRef, sealed: [u8; 32], copy: Arc<SignedArtefact>) {
        self.remove(&key);
        let weight = Self::weight(&copy);
        if weight > self.max_bytes || self.max_entries == 0 {
            return;
        }
        self.bytes += weight;
        self.entries.push_front((key, sealed, copy));
        while self.entries.len() > self.max_entries || self.bytes > self.max_bytes {
            match self.entries.pop_back() {
                Some((_, _, old)) => self.bytes -= Self::weight(&old),
                None => break,
            }
        }
    }

    pub(crate) fn remove(&mut self, key: &ArtefactRef) {
        if let Some(at) = self.entries.iter().position(|(k, _, _)| k == key) {
            if let Some((_, _, old)) = self.entries.remove(at) {
                self.bytes -= Self::weight(&old);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn keys(&self) -> Vec<ArtefactRef> {
        self.entries.iter().map(|(k, _, _)| k.clone()).collect()
    }
}

/// A read of a sealed artefact, made outside the registry's lock: from
/// [`Registry::begin_fetch`], [`run`](Self::run), then handed to
/// [`Registry::finish_fetch`].
pub struct ReadCheck {
    key: ArtefactRef,
    record: Record,
    path: PathBuf,
    full: bool,
    sealer: Arc<dyn Sealer>,
    signers: Arc<TrustAnchors>,
}

/// What [`ReadCheck::run`] found.
pub struct ReadOutcome(Result<ReadPassed, String>);

struct ReadPassed {
    artefact: SignedArtefact,
    sealed: [u8; 32],
    gated: bool,
}

impl ReadCheck {
    /// Read and unseal the sealed copy, check it is the registered artefact, and run the
    /// gate unless these are the exact sealed bytes it last passed on (and the read did
    /// not ask for the full check).
    pub fn run(&self) -> ReadOutcome {
        ReadOutcome((|| {
            let (_, stored, sealed) = read_artefact(self.sealer.as_ref(), &self.path)?;
            if stored.artefact.manifest != self.record.manifest
                || stored.signer != self.record.signer
            {
                return Err("the copy at rest is not the artefact that was registered".into());
            }
            let gated = self.full || self.record.verified_seal != Some(sealed);
            if gated {
                gate(&self.signers, &stored.signer, &stored.artefact)
                    .map_err(|r| format!("{} ({})", r.detail(), r.code()))?;
            }
            Ok(ReadPassed {
                artefact: stored.artefact,
                sealed,
                gated,
            })
        })())
    }
}

/// A fetch's first step: served already, or a read to run. Both variants are a few
/// hundred bytes, made and consumed once per call.
#[allow(clippy::large_enum_variant)]
pub enum Fetch {
    /// A verified copy of the last verified sealed bytes.
    Ready(Record, Arc<SignedArtefact>),
    /// The sealed copy must be read and checked.
    Read(ReadCheck),
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
        let m = upload.manifest();
        Self {
            id: m.id.clone(),
            version: m.version.clone(),
            component: m.component.clone(),
            signer: upload.signer.clone(),
            payload_sha256: attest::hex::hex(&m.payload_sha256),
            reason,
            by: by.to_string(),
            at,
        }
    }
}

/// One entry of a listing page.
#[derive(Debug, Clone, Copy)]
pub enum Listed<'a> {
    /// A held artefact, verified or unverifiable.
    Held(&'a Record),
    /// A refusal.
    Rejected(&'a Rejection),
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

/// An upload in flight. Its bytes accumulate in the artefact it will become.
#[derive(Debug)]
pub struct Upload {
    owner: String,
    artefact: SignedArtefact,
    /// The signing key's id.
    pub signer: String,
    size: u64,
    touched: u64,
}

impl Upload {
    /// The signed manifest.
    pub fn manifest(&self) -> &ArtefactManifest {
        &self.artefact.manifest
    }
}

/// The registry: verified records, refusals, uploads in flight, and the sealed store.
pub struct Registry {
    dir: PathBuf,
    sealer: Arc<dyn Sealer>,
    signers: Arc<TrustAnchors>,
    records: BTreeMap<ArtefactRef, Record>,
    rejected: Vec<Rejection>,
    journal: File,
    uploads: BTreeMap<String, Upload>,
    /// Verified copies, so a download's later slices come from the copy its first slice
    /// checked.
    cache: VerifiedCache,
    /// How many times the whole gate has run on a stored artefact (open and reads).
    full_verifications: u64,
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
        let mut full_verifications = 0u64;
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
            let (key, stored, sealed) = read_artefact(sealer.as_ref(), &path)?;
            let state = gate(&signers, &stored.signer, &stored.artefact)
                .map_err(|r| format!("{} ({})", r.detail(), r.code()));
            full_verifications += 1;
            let verified_seal = state.is_ok().then_some(sealed);
            records.insert(
                key,
                Record {
                    size: stored.artefact.payload.len() as u64,
                    manifest: stored.artefact.manifest,
                    signer: stored.signer,
                    registered_by: stored.by,
                    registered_at: stored.at,
                    state,
                    verified_seal,
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
            sealer: Arc::from(sealer),
            signers: Arc::new(signers),
            records,
            rejected,
            journal,
            uploads: BTreeMap::new(),
            cache: VerifiedCache::new(CACHE_ENTRIES, CACHE_BYTES),
            full_verifications,
        })
    }

    /// How many times the whole gate has run on a stored artefact, at the open and on
    /// reads since.
    pub fn full_verifications(&self) -> u64 {
        self.full_verifications
    }

    /// The artefacts with a verified copy cached, most recently used first.
    #[cfg(test)]
    fn cached(&self) -> Vec<ArtefactRef> {
        self.cache.keys()
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

    /// One page of the listing: held artefacts by id then version, then refusals oldest
    /// first, starting after `cursor` (from a previous page's `next`), at most `limit`
    /// (capped at [`MAX_LIST_PAGE`]). Returns the page and the cursor of the next one,
    /// if there is more. Refusals are append-only, so a cursor stays valid as the
    /// registry grows.
    pub fn page(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<Listed<'_>>, Option<String>), StoreError> {
        let limit = limit.clamp(1, MAX_LIST_PAGE);
        let bad = || StoreError::Malformed("the cursor is not one this registry gave".into());
        let (after, rejected_from) = match cursor {
            None => (Bound::Unbounded, 0),
            Some(c) => match c.split_at(c.len().min(1)) {
                ("h", rest) => {
                    let raw = attest::hex::unhex(rest).ok_or_else(bad)?;
                    let split = raw.iter().position(|&b| b == 0).ok_or_else(bad)?;
                    let id = String::from_utf8(raw[..split].to_vec()).map_err(|_| bad())?;
                    let version =
                        String::from_utf8(raw[split + 1..].to_vec()).map_err(|_| bad())?;
                    (Bound::Excluded(ArtefactRef::new(id, version)), 0)
                }
                ("r", rest) => (
                    // Past every held artefact.
                    Bound::Excluded(ArtefactRef::new(
                        char::MAX.to_string(),
                        char::MAX.to_string(),
                    )),
                    rest.parse::<usize>().map_err(|_| bad())?,
                ),
                _ => return Err(bad()),
            },
        };
        let held = self
            .records
            .range((after, Bound::Unbounded))
            .map(|(key, r)| (format!("h{}", cursor_key(key)), Listed::Held(r)));
        let refused = self
            .rejected
            .iter()
            .enumerate()
            .skip(rejected_from)
            .map(|(i, r)| (format!("r{}", i + 1), Listed::Rejected(r)));
        let mut items: Vec<(String, Listed<'_>)> = held.chain(refused).take(limit + 1).collect();
        let next = if items.len() > limit {
            items.truncate(limit);
            items.last().map(|(c, _)| c.clone())
        } else {
            None
        };
        Ok((items.into_iter().map(|(_, l)| l).collect(), next))
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
        self.expire(now);
        let malformed = |m: String| (StoreError::Malformed(m), None);
        check_name(&new.manifest.id, "id").map_err(malformed)?;
        check_name(&new.manifest.version, "version").map_err(malformed)?;
        check_name(&new.manifest.component, "component").map_err(malformed)?;
        check_name(&new.signer, "signer").map_err(malformed)?;
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
            artefact: SignedArtefact {
                manifest: new.manifest,
                sbom: new.sbom,
                payload: Vec::new(),
                signature: new.signature,
            },
            signer: new.signer,
            size: new.size,
            touched: now,
        };
        if self.signers.get(&upload.signer).is_none() {
            return Err((
                StoreError::Refused(Refusal::UnknownSigner),
                Some(Box::new(upload)),
            ));
        }
        let busy = |m: &str| Err((StoreError::Busy(m.to_string()), None));
        if self.uploads.values().filter(|u| u.owner == owner).count() >= MAX_UPLOADS_PER_OWNER {
            return busy("this identity already has its uploads in flight; finish one first");
        }
        let pending: u64 = self.uploads.values().map(|u| u.size).sum();
        if self.uploads.len() >= MAX_UPLOADS || pending + upload.size > MAX_PENDING_BYTES {
            return busy("too many uploads in flight; try again later");
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
        self.expire(now);
        let upload = self.upload_mut(owner, upload_id)?;
        if data.len() > CHUNK_BYTES {
            return Err(StoreError::Malformed(format!(
                "a chunk may carry at most {CHUNK_BYTES} bytes"
            )));
        }
        let payload = &mut upload.artefact.payload;
        let have = payload.len() as u64;
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
        if payload.capacity() == 0 && upload.size > 0 {
            // The whole artefact, once: it is never copied while it grows.
            payload
                .try_reserve_exact(upload.size as usize)
                .map_err(|_| StoreError::Busy("no memory for this upload".into()))?;
        }
        payload.extend_from_slice(data);
        upload.touched = now;
        Ok(payload.len() as u64)
    }

    /// Take `owner`'s complete upload out for the commit. An incomplete one stays.
    pub fn take(&mut self, owner: &str, upload_id: &str, now: u64) -> Result<Upload, StoreError> {
        self.expire(now);
        let upload = self.upload_mut(owner, upload_id)?;
        let have = upload.artefact.payload.len() as u64;
        if have != upload.size {
            return Err(StoreError::Malformed(format!(
                "{have} of {} bytes have arrived",
                upload.size
            )));
        }
        Ok(self.uploads.remove(upload_id).expect("just found"))
    }

    /// The gate: verify `upload` against its named signer's anchored key.
    pub fn gate(&self, upload: &Upload) -> Result<(), Refusal> {
        gate(&self.signers, &upload.signer, &upload.artefact)
    }

    /// Record a refusal durably; its bytes are never written. Returns whether it was
    /// journalled: past [`MAX_REJECTIONS`], or past an identity's
    /// [`UNKNOWN_SIGNER_LIMIT`], it is not (the upload is refused all the same).
    pub fn reject(&mut self, rejection: Rejection) -> Result<bool, StoreError> {
        if self.rejected.len() >= MAX_REJECTIONS {
            return Ok(false);
        }
        if rejection.reason == Refusal::UnknownSigner {
            let recent = self
                .rejected
                .iter()
                .rev()
                .take_while(|r| rejection.at.saturating_sub(r.at) < UNKNOWN_SIGNER_WINDOW_SECS)
                .filter(|r| r.reason == Refusal::UnknownSigner && r.by == rejection.by)
                .count();
            if recent >= UNKNOWN_SIGNER_LIMIT {
                return Ok(false);
            }
        }
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
        Ok(true)
    }

    /// Store a gate-verified upload, sealed. Call only after [`Self::gate`] passed.
    pub fn admit(&mut self, upload: Upload, by: &str, now: u64) -> Result<Record, StoreError> {
        let key = ArtefactRef::new(
            upload.artefact.manifest.id.clone(),
            upload.artefact.manifest.version.clone(),
        );
        if self.records.contains_key(&key) {
            return Err(StoreError::Conflict(format!(
                "{}@{} is already registered",
                key.id, key.version
            )));
        }
        let stored = Stored {
            signer: upload.signer,
            by: by.to_string(),
            at: now,
            artefact: upload.artefact,
        };
        let name = name_hash(&key);
        let sealed = self
            .sealer
            .seal(&artefact_aad(&name), &encode_stored(&stored))
            .map_err(|e| StoreError::Io(format!("cannot seal the artefact: {e}")))?;
        atomic_write(&self.artefact_path(&key), &sealed)
            .map_err(|e| StoreError::Io(format!("cannot store the artefact: {e}")))?;
        // The gate passed on exactly what was sealed.
        let sealed = sha256(&sealed);
        let record = Record {
            manifest: stored.artefact.manifest.clone(),
            signer: stored.signer,
            size: upload.size,
            registered_by: stored.by,
            registered_at: now,
            state: Ok(()),
            verified_seal: Some(sealed),
        };
        self.records.insert(key.clone(), record.clone());
        self.cache.insert(key, sealed, Arc::new(stored.artefact));
        Ok(record)
    }

    /// Undo [`Self::admit`] — for a registration that could not then be audited, so
    /// nothing is held that the ledger does not record.
    pub fn withdraw(&mut self, id: &str, version: &str) -> Result<(), StoreError> {
        let key = ArtefactRef::new(id, version);
        self.cache.remove(&key);
        self.records.remove(&key);
        match fs::remove_file(self.artefact_path(&key)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StoreError::Io(format!(
                "cannot withdraw {id}@{version}: {e}"
            ))),
        }
    }

    /// Fetch `id`@`version`, checked as `recheck` asks, under the lock throughout. For a
    /// service, prefer [`Self::begin_fetch`], [`ReadCheck::run`] outside the lock, and
    /// [`Self::finish_fetch`]. A copy that fails is marked unverifiable for good
    /// ([`StoreError::VerifyFailed`]), so it is never served again.
    pub fn fetch(
        &mut self,
        id: &str,
        version: &str,
        recheck: Recheck,
    ) -> Result<(Record, Arc<SignedArtefact>), StoreError> {
        match self.begin_fetch(id, version, recheck)? {
            Fetch::Ready(record, artefact) => Ok((record, artefact)),
            Fetch::Read(read) => {
                let outcome = read.run();
                self.finish_fetch(read, outcome)
            }
        }
    }

    /// The first step of a fetch: refuse what is unknown or unverifiable, serve a cached
    /// verified copy when `recheck` is [`Recheck::Cached`], or hand back the read to run.
    pub fn begin_fetch(
        &mut self,
        id: &str,
        version: &str,
        recheck: Recheck,
    ) -> Result<Fetch, StoreError> {
        let key = ArtefactRef::new(id, version);
        let record = self.held(&key)?.clone();
        if recheck == Recheck::Cached {
            if let Some(sealed) = &record.verified_seal {
                if let Some(copy) = self.cache.get(&key, sealed) {
                    return Ok(Fetch::Ready(record, copy));
                }
            }
        }
        Ok(Fetch::Read(ReadCheck {
            path: self.artefact_path(&key),
            key,
            record,
            full: recheck == Recheck::Full,
            sealer: self.sealer.clone(),
            signers: self.signers.clone(),
        }))
    }

    /// The last step of a fetch: record what the read found. A pass remembers the sealed
    /// bytes it verified and caches the copy; a failure marks the artefact unverifiable.
    /// If the artefact was marked, withdrawn or replaced meanwhile, the read counts for
    /// nothing.
    pub fn finish_fetch(
        &mut self,
        read: ReadCheck,
        outcome: ReadOutcome,
    ) -> Result<(Record, Arc<SignedArtefact>), StoreError> {
        let key = read.key;
        let (id, version) = (&key.id, &key.version);
        let current = self.held(&key)?;
        if current.manifest != read.record.manifest || current.signer != read.record.signer {
            return Err(StoreError::Io(format!(
                "{id}@{version} changed while it was read; try again"
            )));
        }
        match outcome.0 {
            Ok(passed) => {
                if passed.gated {
                    self.full_verifications += 1;
                }
                let record = self.records.get_mut(&key).expect("held, just checked");
                record.verified_seal = Some(passed.sealed);
                let record = record.clone();
                let copy = Arc::new(passed.artefact);
                self.cache.insert(key, passed.sealed, copy.clone());
                Ok((record, copy))
            }
            Err(reason) => {
                self.cache.remove(&key);
                if let Some(r) = self.records.get_mut(&key) {
                    r.state = Err(reason.clone());
                    r.verified_seal = None;
                }
                Err(StoreError::VerifyFailed(format!(
                    "{id}@{version} does not verify: {reason}"
                )))
            }
        }
    }

    /// The record for `key`, if it is held and not marked unverifiable.
    fn held(&self, key: &ArtefactRef) -> Result<&Record, StoreError> {
        let (id, version) = (&key.id, &key.version);
        let record = self
            .records
            .get(key)
            .ok_or_else(|| StoreError::NotFound(format!("no artefact {id}@{version}")))?;
        if let Err(reason) = &record.state {
            return Err(StoreError::Unverifiable(format!(
                "{id}@{version} does not verify: {reason}"
            )));
        }
        Ok(record)
    }

    fn expire(&mut self, now: u64) {
        self.uploads
            .retain(|_, u| now.saturating_sub(u.touched) < UPLOAD_IDLE_SECS);
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
        self.dir.join("artefacts").join(format!(
            "{}.{SEALED_EXT}",
            attest::hex::hex(&name_hash(key))
        ))
    }
}

fn gate(signers: &TrustAnchors, signer: &str, artefact: &SignedArtefact) -> Result<(), Refusal> {
    let key = signers.get(signer).ok_or(Refusal::UnknownSigner)?;
    bifrost_artefact::verify(key, artefact).map_err(Refusal::from)
}

/// The fixed-length name an artefact is stored under: SHA-256 of `id\0version`.
fn name_hash(key: &ArtefactRef) -> [u8; 32] {
    let mut raw = key.id.clone().into_bytes();
    raw.push(0);
    raw.extend_from_slice(key.version.as_bytes());
    sha256(&raw)
}

fn cursor_key(key: &ArtefactRef) -> String {
    let mut raw = key.id.clone().into_bytes();
    raw.push(0);
    raw.extend_from_slice(key.version.as_bytes());
    attest::hex::hex(&raw)
}

/// Ids, versions, component kinds and signer ids: 1 to 128 of letters, digits, `.`,
/// `_`, `-`, `+` — characters JSON never escapes (see the frame budget).
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

fn artefact_aad(name: &[u8; 32]) -> Vec<u8> {
    let mut a = ARTEFACT_DS.to_vec();
    a.extend_from_slice(name);
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

/// Read, unseal and decode one sealed artefact file. Also returns the SHA-256 of the
/// sealed bytes as read.
fn read_artefact(
    sealer: &dyn Sealer,
    path: &Path,
) -> Result<(ArtefactRef, Stored, [u8; 32]), String> {
    let what = path.display();
    let name: [u8; 32] = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(attest::hex::unhex)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| format!("{what}: not an artefact file name"))?;
    let sealed = fs::read(path).map_err(|e| format!("cannot read {what}: {e}"))?;
    let plain = sealer
        .unseal(&artefact_aad(&name), &sealed)
        .map_err(|e| format!("{what} does not unseal (tampered, or another artefact's): {e}"))?;
    let stored = decode_stored(&plain).map_err(|e| format!("{what}: {e}"))?;
    let key = ArtefactRef::new(
        stored.artefact.manifest.id.clone(),
        stored.artefact.manifest.version.clone(),
    );
    if name_hash(&key) != name {
        return Err(format!("{what} holds another artefact"));
    }
    Ok((key, stored, sha256(&sealed)))
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

    fn signed_as(sk: &SigningKey, id: &str, version: &str, payload: Vec<u8>) -> SignedArtefact {
        let sbom = generate_sbom(
            id,
            version,
            "service",
            vec![SbomComponent::new("pqc-agility", "0.0.0")],
        );
        build_and_sign(id, version, "service", payload, sbom, sk)
    }

    fn signed(sk: &SigningKey, payload: Vec<u8>) -> SignedArtefact {
        signed_as(sk, "forge-worker", "0.1.0", payload)
    }

    fn new_upload(a: &SignedArtefact, signer: &str) -> NewUpload {
        NewUpload {
            manifest: a.manifest.clone(),
            sbom: a.sbom.clone(),
            signature: a.signature.clone(),
            signer: signer.into(),
            size: a.payload.len() as u64,
        }
    }

    fn upload(reg: &mut Registry, a: &SignedArtefact, signer: &str) -> Result<Upload, StoreError> {
        reg.begin("ci", "u1".into(), new_upload(a, signer), 10)
            .map_err(|(e, _)| e)?;
        let mut offset = 0u64;
        for piece in a.payload.chunks(CHUNK_BYTES) {
            offset = reg.chunk("ci", "u1", offset, piece, 11)?;
        }
        reg.take("ci", "u1", 12)
    }

    fn admit(reg: &mut Registry, a: &SignedArtefact) {
        let up = upload(reg, a, "release-1").unwrap();
        assert_eq!(reg.gate(&up), Ok(()));
        reg.admit(up, "ci", 12).unwrap();
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
        admit(&mut reg, &a);

        let files: Vec<_> = fs::read_dir(dir.join("artefacts")).unwrap().collect();
        assert_eq!(files.len(), 1);
        let on_disk = fs::read(files[0].as_ref().unwrap().path()).unwrap();
        assert!(
            !on_disk.windows(64).any(|w| w == &a.payload[..64]),
            "the bytes are not stored in the clear"
        );

        drop(reg);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        let (record, got) = reg.fetch("forge-worker", "0.1.0", Recheck::Seal).unwrap();
        assert_eq!(record.registered_by, "ci");
        assert_eq!(got.payload, a.payload);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_longest_names_store_under_a_fixed_length_file_name() {
        let dir = scratch("long-names");
        let (vk, sk) = keys();
        let (id, version) = ("i".repeat(128), "v".repeat(128));
        let a = signed_as(&sk, &id, &version, vec![9u8; 100]);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        admit(&mut reg, &a);
        let name = fs::read_dir(dir.join("artefacts"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name();
        assert_eq!(name.len(), 64 + 1 + SEALED_EXT.len());
        drop(reg);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        assert!(reg.fetch(&id, &version, Recheck::Seal).is_ok());

        let mut long = new_upload(&a, "release-1");
        long.manifest.version = "v".repeat(129);
        assert!(matches!(
            reg.begin("ci", "x".into(), long, 0),
            Err((StoreError::Malformed(_), None))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_withdrawn_admission_leaves_nothing_behind() {
        let dir = scratch("withdraw");
        let (vk, sk) = keys();
        let a = signed(&sk, vec![4u8; 100]);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        admit(&mut reg, &a);
        reg.withdraw("forge-worker", "0.1.0").unwrap();
        assert!(reg.record("forge-worker", "0.1.0").is_none());
        assert_eq!(fs::read_dir(dir.join("artefacts")).unwrap().count(), 0);
        assert!(matches!(
            reg.fetch("forge-worker", "0.1.0", Recheck::Cached),
            Err(StoreError::NotFound(_))
        ));
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
            assert!(reg.reject(Rejection::of(&up, want, "ci", 20)).unwrap());
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
    fn unknown_signer_refusals_are_journalled_only_up_to_a_rate() {
        let dir = scratch("rate");
        let (vk, sk) = keys();
        let a = signed(&sk, vec![1u8; 10]);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        let refusal = |by: &str, at: u64| {
            let (_, up) = reg_begin_unknown(&a);
            Rejection::of(&up, Refusal::UnknownSigner, by, at)
        };
        for i in 0..UNKNOWN_SIGNER_LIMIT as u64 {
            assert!(reg.reject(refusal("spammer", 100 + i)).unwrap());
        }
        assert!(!reg.reject(refusal("spammer", 200)).unwrap());
        assert!(reg.reject(refusal("someone-else", 200)).unwrap());
        assert!(reg
            .reject(refusal("spammer", 100 + UNKNOWN_SIGNER_WINDOW_SECS + 10))
            .unwrap());
        assert_eq!(reg.rejections().len(), UNKNOWN_SIGNER_LIMIT + 2);
        let _ = fs::remove_dir_all(&dir);
    }

    fn reg_begin_unknown(a: &SignedArtefact) -> ((), Upload) {
        let dir = scratch("unknown-helper");
        let mut reg = Registry::open(&dir, Box::new(TestSealer), TrustAnchors::new()).unwrap();
        let up = match reg.begin("x", "u".into(), new_upload(a, "nobody"), 0) {
            Err((StoreError::Refused(Refusal::UnknownSigner), Some(up))) => *up,
            other => panic!("{other:?}"),
        };
        let _ = fs::remove_dir_all(&dir);
        ((), up)
    }

    #[test]
    fn the_listing_pages_with_a_cursor_across_held_and_refused() {
        let dir = scratch("pages");
        let (vk, sk) = keys();
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        for v in ["0.1.0", "0.2.0", "0.3.0"] {
            admit(&mut reg, &signed_as(&sk, "svc", v, vec![1u8; 10]));
        }
        let bad = signed_as(&sk, "svc", "0.9.0", vec![1u8; 10]);
        for _ in 0..2 {
            let (_, up) = reg_begin_unknown(&bad);
            reg.reject(Rejection::of(&up, Refusal::UnknownSigner, "ci", 1))
                .unwrap();
        }
        let mut seen = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let (page, next) = reg.page(cursor.as_deref(), 2).unwrap();
            assert!(page.len() <= 2);
            for item in page {
                seen.push(match item {
                    Listed::Held(r) => format!("held {}", r.manifest.version),
                    Listed::Rejected(r) => format!("refused {}", r.version),
                });
            }
            match next {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        assert_eq!(
            seen,
            [
                "held 0.1.0",
                "held 0.2.0",
                "held 0.3.0",
                "refused 0.9.0",
                "refused 0.9.0"
            ]
        );
        assert_eq!(reg.page(None, 10).unwrap().0.len(), 5);
        assert!(reg.page(None, 10).unwrap().1.is_none());
        for bad in ["x1", "hzz", "r-1", "h41", ""] {
            assert!(reg.page(Some(bad), 2).is_err(), "{bad}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tampered_store_fails_the_open_and_a_removed_signer_is_never_served() {
        let dir = scratch("at-rest");
        let (vk, sk) = keys();
        let a = signed(&sk, vec![3u8; 1000]);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        admit(&mut reg, &a);
        drop(reg);

        // The signer's anchor is withdrawn: listed, not served.
        let mut reg = Registry::open(&dir, Box::new(TestSealer), TrustAnchors::new()).unwrap();
        assert!(reg.record("forge-worker", "0.1.0").unwrap().state.is_err());
        assert!(matches!(
            reg.fetch("forge-worker", "0.1.0", Recheck::Seal),
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
    fn a_copy_that_fails_while_running_is_marked_and_never_served_again() {
        let dir = scratch("runtime-fail");
        let (vk, sk) = keys();
        let a = signed(&sk, vec![3u8; 1000]);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        admit(&mut reg, &a);
        let path = fs::read_dir(dir.join("artefacts"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut bytes = fs::read(&path).unwrap();
        bytes[40] ^= 1;
        fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            reg.fetch("forge-worker", "0.1.0", Recheck::Seal),
            Err(StoreError::VerifyFailed(_))
        ));
        assert!(reg.record("forge-worker", "0.1.0").unwrap().state.is_err());
        bytes[40] ^= 1;
        fs::write(&path, &bytes).unwrap();
        // Even restored, and even from the cache, it is not served until a restart
        // re-verifies it.
        for check in [Recheck::Full, Recheck::Seal, Recheck::Cached] {
            assert!(matches!(
                reg.fetch("forge-worker", "0.1.0", check),
                Err(StoreError::Unverifiable(_))
            ));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    fn only_file(dir: &Path) -> PathBuf {
        fs::read_dir(dir.join("artefacts"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
    }

    #[test]
    fn a_download_rechecks_the_seal_and_verifies_in_full_only_bytes_not_yet_verified() {
        let dir = scratch("recheck");
        let (vk, sk) = keys();
        let a = signed(&sk, vec![6u8; 3000]);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        admit(&mut reg, &a);
        let n = reg.full_verifications();

        // The admitted copy passed the gate: a download unseals it, and that is all.
        let (_, got) = reg.fetch("forge-worker", "0.1.0", Recheck::Seal).unwrap();
        assert_eq!(got.payload, a.payload);
        reg.fetch("forge-worker", "0.1.0", Recheck::Cached).unwrap();
        assert_eq!(reg.full_verifications(), n);
        // VerifyArtefact always runs the whole gate.
        reg.fetch("forge-worker", "0.1.0", Recheck::Full).unwrap();
        assert_eq!(reg.full_verifications(), n + 1);

        // Different sealed bytes — still a valid seal over the same artefact — are not
        // the bytes that were verified: the next download verifies in full, once.
        let path = only_file(&dir);
        let (key, mut stored, _) = read_artefact(&TestSealer, &path).unwrap();
        stored.at += 1;
        let resealed = TestSealer
            .seal(&artefact_aad(&name_hash(&key)), &encode_stored(&stored))
            .unwrap();
        fs::write(&path, &resealed).unwrap();
        reg.fetch("forge-worker", "0.1.0", Recheck::Seal).unwrap();
        assert_eq!(reg.full_verifications(), n + 2);
        reg.fetch("forge-worker", "0.1.0", Recheck::Seal).unwrap();
        assert_eq!(reg.full_verifications(), n + 2);

        // The open verifies every file, so after a restart a download needs only the seal.
        drop(reg);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        let n = reg.full_verifications();
        assert_eq!(n, 1, "the open verified the one artefact");
        reg.fetch("forge-worker", "0.1.0", Recheck::Seal).unwrap();
        assert_eq!(reg.full_verifications(), n);

        // The seal is checked whatever is cached: a flipped byte is refused, and from
        // then on the artefact is not served at all.
        let mut bytes = fs::read(&path).unwrap();
        bytes[40] ^= 1;
        fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            reg.fetch("forge-worker", "0.1.0", Recheck::Seal),
            Err(StoreError::VerifyFailed(_))
        ));
        assert!(matches!(
            reg.fetch("forge-worker", "0.1.0", Recheck::Cached),
            Err(StoreError::Unverifiable(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_later_slice_of_an_artefact_no_longer_cached_rechecks_the_seal_not_the_gate() {
        let dir = scratch("lru-registry");
        let (vk, sk) = keys();
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        let versions = ["0.1.0", "0.2.0", "0.3.0", "0.4.0", "0.5.0"];
        for v in versions {
            admit(&mut reg, &signed_as(&sk, "svc", v, vec![2u8; 100]));
        }
        for v in versions {
            reg.fetch("svc", v, Recheck::Seal).unwrap();
        }
        assert_eq!(
            reg.cached(),
            ["0.5.0", "0.4.0", "0.3.0", "0.2.0"]
                .map(|v| ArtefactRef::new("svc", v))
                .to_vec()
        );
        let n = reg.full_verifications();
        // 0.1.0 fell out: its later slice reads the sealed copy again, seal only.
        reg.fetch("svc", "0.1.0", Recheck::Cached).unwrap();
        assert_eq!(reg.full_verifications(), n);
        assert_eq!(reg.cached()[0], ArtefactRef::new("svc", "0.1.0"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_verified_copies_are_an_lru_bounded_in_entries_and_bytes() {
        let (_, sk) = keys();
        let art = |v: &str, n: usize| Arc::new(signed_as(&sk, "svc", v, vec![1u8; n]));
        let (a, b, c) = (art("a", 100), art("b", 100), art("c", 100));
        let unit = VerifiedCache::weight(&a);
        let key = |v: &str| ArtefactRef::new("svc", v);
        let mut cache = VerifiedCache::new(2, unit * 3);
        cache.insert(key("a"), [1; 32], a.clone());
        cache.insert(key("b"), [2; 32], b);
        // Only the copy of exactly the verified sealed bytes is served.
        assert!(cache.get(&key("a"), &[9; 32]).is_none());
        assert!(cache.get(&key("a"), &[1; 32]).is_some());
        // a was used last, so b goes first.
        cache.insert(key("c"), [3; 32], c);
        assert_eq!(cache.keys(), [key("c"), key("a")]);
        // Bounded by bytes too: a big copy pushes the others out.
        // Just under the bound (its SBOM names "big", a little longer than "a").
        let big = art("big", 90 + 2 * unit as usize);
        cache.insert(key("big"), [4; 32], big);
        assert_eq!(cache.keys(), [key("big")]);
        // One past the byte bound is not cached at all.
        let huge = art("huge", 4 * unit as usize);
        cache.insert(key("huge"), [5; 32], huge);
        assert_eq!(cache.keys(), [key("big")]);
        cache.remove(&key("big"));
        assert!(cache.keys().is_empty());
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn uploads_are_ordered_owned_bounded_and_unique() {
        let dir = scratch("uploads");
        let (vk, sk) = keys();
        let a = signed(&sk, vec![5u8; 10]);
        let mut reg = Registry::open(&dir, Box::new(TestSealer), anchors(&vk)).unwrap();
        let new = |version: &str| {
            let mut n = new_upload(&a, "release-1");
            n.manifest.version = version.into();
            n
        };
        reg.begin("ci", "u".into(), new_upload(&a, "release-1"), 0)
            .unwrap();
        assert!(matches!(
            reg.chunk("ci", "u", 4, &[5; 2], 0),
            Err(StoreError::Malformed(_))
        ));
        assert!(matches!(
            reg.chunk("someone-else", "u", 0, &[5; 2], 0),
            Err(StoreError::NotFound(_))
        ));
        reg.chunk("ci", "u", 0, &[5; 4], 0).unwrap();
        assert!(matches!(
            reg.take("ci", "u", 0),
            Err(StoreError::Malformed(_))
        ));
        assert!(matches!(
            reg.chunk("ci", "u", 4, &[5; 7], 0),
            Err(StoreError::Malformed(_))
        ));
        reg.chunk("ci", "u", 4, &[5; 6], 0).unwrap();
        let up = reg.take("ci", "u", 0).unwrap();
        reg.admit(up, "ci", 0).unwrap();
        assert!(matches!(
            reg.begin("ci", "v".into(), new_upload(&a, "release-1"), 0),
            Err((StoreError::Conflict(_), None))
        ));

        let mut big = new("0.2.0");
        big.size = MAX_ARTEFACT_BYTES + 1;
        assert!(matches!(
            reg.begin("ci", "w".into(), big, 0),
            Err((StoreError::Malformed(_), None))
        ));

        // Two in flight per identity.
        reg.begin("ci", "x".into(), new("0.3.0"), 100).unwrap();
        reg.begin("ci", "y".into(), new("0.4.0"), 100).unwrap();
        assert!(matches!(
            reg.begin("ci", "z".into(), new("0.5.0"), 100),
            Err((StoreError::Busy(_), None))
        ));
        reg.begin("other", "o".into(), new("0.6.0"), 100).unwrap();

        // An idle upload is dropped, whichever call notices first.
        assert!(matches!(
            reg.chunk("ci", "x", 0, &[5; 1], 100 + UPLOAD_IDLE_SECS),
            Err(StoreError::NotFound(_))
        ));
        assert!(matches!(
            reg.take("ci", "y", 100 + UPLOAD_IDLE_SECS),
            Err(StoreError::NotFound(_))
        ));
        reg.begin("ci", "z".into(), new("0.5.0"), 100 + UPLOAD_IDLE_SECS)
            .unwrap();
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
