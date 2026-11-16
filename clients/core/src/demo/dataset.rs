//! The signed demo dataset file. `[NFR-PQ-3]` `[AC-C-13]`
//!
//! ```text
//! magic       7   b"MGDEMO1"
//! version     1   u8 = 1
//! release_len 4   u32 LE ┐ the release this dataset is for, e.g. "0.1.0-rc.3"
//! release     n          ┘
//! body_len    4   u32 LE ┐ canonical JSON (see [`super::canon`])
//! body        n          ┘
//! env_len     4   u32 LE ┐ an attest MGSIG1 envelope
//! envelope    n          ┘
//! ```
//!
//! The signature covers `preimage(DEMO_DATASET, magic ‖ version ‖ release_len ‖
//! release ‖ body_len ‖ body)`, so the release label cannot be swapped under a valid
//! signature and a demo-dataset signature never stands for any other construct.
//! Verification order: strict framing (truncation and trailing bytes refused), domain
//! label, pinned key, `attest::accept`, release label — and only then is the body
//! parsed, and refused unless it is canonical. There is no warning tier: every
//! finding is an `Err`.

use attest::accept::{accept, preimage, AcceptError, DEMO_DATASET};
use attest::envelope::SigEnvelope;
use attest::json::{parse, Json};
use attest::{SignError, SigningBackend, TrustAnchors};
use pqc_agility::{SigId, Signature, VerifyingKey};

use super::canon;

/// File magic.
pub const MAGIC: &[u8; 7] = b"MGDEMO1";
/// Format version.
pub const VERSION: u8 = 1;
/// Algorithms a demo dataset may be signed with (a subset of
/// `attest::APPROVED_RELEASE_ALGS`, so a ceremony key can re-sign it later).
pub const APPROVED: [SigId; 2] = [SigId::MlDsa65, SigId::MlDsa87];

/// How many characters of an unpinned key id an error message echoes.
pub const KEY_ID_SHOWN: usize = 64;

/// Why a dataset was refused.
#[derive(Debug)]
pub enum DatasetError {
    /// Truncated, trailing bytes, bad magic or version, or a malformed envelope.
    Malformed(String),
    /// The envelope names another signing domain.
    WrongDomain,
    /// The signing key is not pinned. The id comes from an unverified file, so it
    /// holds at most [`KEY_ID_SHOWN`] characters of it, followed by `…` when cut
    /// (see [`DatasetError::unknown_key`]); `Debug` and `Display` are both bounded.
    UnknownKey(String),
    /// The signature does not verify under the pinned key.
    Signature(AcceptError),
    /// The dataset is for another release.
    WrongRelease {
        /// What the file says.
        found: String,
        /// What this build expects.
        expected: String,
    },
    /// The body is not strict, canonical JSON.
    Body(String),
    /// Signing failed (build step only).
    Sign(String),
}

impl std::fmt::Display for DatasetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatasetError::Malformed(d) => write!(f, "malformed demo dataset: {d}"),
            DatasetError::WrongDomain => write!(f, "not signed as a demo dataset"),
            DatasetError::UnknownKey(k) => {
                // Already bounded where it was made (see `unknown_key`).
                match k.strip_suffix('…') {
                    Some(shown) => write!(f, "signing key {shown:?}… is not pinned"),
                    None => write!(f, "signing key {k:?} is not pinned"),
                }
            }
            DatasetError::Signature(e) => write!(f, "demo dataset signature refused: {e}"),
            DatasetError::WrongRelease { found, expected } => {
                write!(f, "demo dataset is for {found}, this build is {expected}")
            }
            DatasetError::Body(d) => write!(f, "demo dataset body: {d}"),
            DatasetError::Sign(d) => write!(f, "signing failed: {d}"),
        }
    }
}

impl std::error::Error for DatasetError {}

impl DatasetError {
    /// An [`DatasetError::UnknownKey`] for `key_id`, cut to [`KEY_ID_SHOWN`]
    /// characters (and `…`) so no error representation carries the whole id.
    pub fn unknown_key(key_id: &str) -> Self {
        let mut shown: String = key_id.chars().take(KEY_ID_SHOWN).collect();
        if key_id.chars().count() > KEY_ID_SHOWN {
            shown.push('…');
        }
        DatasetError::UnknownKey(shown)
    }
}

/// A verified dataset.
#[derive(Debug)]
pub struct Dataset {
    /// The release it is for.
    pub release: String,
    /// The signing key id.
    pub key_id: String,
    /// The canonical body bytes, as signed.
    pub body_bytes: Vec<u8>,
    /// The parsed body.
    pub body: Json,
}

fn put(o: &mut Vec<u8>, b: &[u8], what: &str) -> Result<(), DatasetError> {
    let n = u32::try_from(b.len())
        .map_err(|_| DatasetError::Sign(format!("{what} is longer than 4 GiB")))?;
    o.extend_from_slice(&n.to_le_bytes());
    o.extend_from_slice(b);
    Ok(())
}

/// The bytes the signature covers (before the domain preimage).
pub fn signed_bytes(release: &str, body: &[u8]) -> Result<Vec<u8>, DatasetError> {
    let mut o = Vec::with_capacity(16 + release.len() + body.len());
    o.extend_from_slice(MAGIC);
    o.push(VERSION);
    put(&mut o, release.as_bytes(), "release")?;
    put(&mut o, body, "body")?;
    Ok(o)
}

/// Parse `bytes` strictly and require them to be their own canonical encoding.
fn canonical_body(bytes: &[u8]) -> Result<Json, DatasetError> {
    let text = std::str::from_utf8(bytes).map_err(|_| DatasetError::Body("not UTF-8".into()))?;
    let body = parse(text).map_err(DatasetError::Body)?;
    if canon::encode(&body).as_bytes() != bytes {
        return Err(DatasetError::Body("not canonical JSON".into()));
    }
    Ok(body)
}

/// Sign `body` for `release` through `signer` and return the file bytes. The body
/// must already be canonical JSON and the key one of [`APPROVED`], so nothing is
/// signed that [`verify`] would refuse.
pub fn sign(
    release: &str,
    body: &[u8],
    signer: &dyn SigningBackend,
) -> Result<Vec<u8>, DatasetError> {
    let alg = signer.verifying_key().alg();
    if !APPROVED.contains(&alg) {
        return Err(DatasetError::Sign(format!(
            "{} is not approved for demo datasets",
            alg.as_str()
        )));
    }
    canonical_body(body)?;
    let signed = signed_bytes(release, body)?;
    let signature = signer
        .sign(&preimage(&DEMO_DATASET, &signed))
        .map_err(|e| DatasetError::Sign(e.to_string()))?;
    let env = SigEnvelope {
        domain: DEMO_DATASET.label().to_vec(),
        key_id: signer.key_id().to_string(),
        signature,
    };
    let mut file = signed;
    put(&mut file, &env.encode(), "envelope")?;
    Ok(file)
}

/// A signer that signs demo-dataset preimages and nothing else: every message must
/// begin `preimage(DEMO_DATASET, MAGIC ‖ VERSION …)`, the shape [`sign`] hands it.
///
/// The demo signing key is meant to sign through this path only. The build step
/// wraps its unsealed key in this type and never hands it to a general signer (a
/// KMS signing service, `mg-sign sign`), so the key cannot be made to sign an
/// arbitrary message in any domain.
pub struct DemoDatasetOnly<S> {
    inner: S,
}

impl<S: SigningBackend> DemoDatasetOnly<S> {
    /// Wrap `inner`.
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S: SigningBackend> SigningBackend for DemoDatasetOnly<S> {
    fn key_id(&self) -> &str {
        self.inner.key_id()
    }

    fn sign(&self, msg: &[u8]) -> Result<Signature, SignError> {
        if !is_dataset_preimage(msg) {
            return Err(SignError::Custodian(
                "the demo key signs demo-dataset preimages only".into(),
            ));
        }
        self.inner.sign(msg)
    }

    fn verifying_key(&self) -> &VerifyingKey {
        self.inner.verifying_key()
    }
}

/// Whether `msg` is exactly `preimage(DEMO_DATASET, signed_bytes(release, body))`
/// for a UTF-8 `release` and a canonical JSON `body`: the prefix, then each field
/// consumed exactly to the end, with nothing after the body.
fn is_dataset_preimage(msg: &[u8]) -> bool {
    let mut head = MAGIC.to_vec();
    head.push(VERSION);
    let Some(rest) = msg.strip_prefix(preimage(&DEMO_DATASET, &head).as_slice()) else {
        return false;
    };
    let mut c = Cursor { b: rest, at: 0 };
    let Ok(release) = c.field("release") else {
        return false;
    };
    let Ok(body) = c.field("body") else {
        return false;
    };
    c.at == rest.len() && std::str::from_utf8(release).is_ok() && canonical_body(body).is_ok()
}

/// A cursor that fails closed rather than panicking on a short read.
struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], DatasetError> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len());
        let end = end.ok_or_else(|| DatasetError::Malformed(format!("{what}: truncated")))?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn field(&mut self, what: &str) -> Result<&'a [u8], DatasetError> {
        let len: [u8; 4] = self
            .take(4, what)?
            .try_into()
            .expect("take(4) yields 4 bytes");
        self.take(u32::from_le_bytes(len) as usize, what)
    }
}

/// Verify `file` against `anchors` for `expected_release`; parse the body only after.
pub fn verify(
    file: &[u8],
    anchors: &TrustAnchors,
    expected_release: &str,
) -> Result<Dataset, DatasetError> {
    let mut c = Cursor { b: file, at: 0 };
    if c.take(MAGIC.len(), "magic")? != MAGIC {
        return Err(DatasetError::Malformed("bad magic".into()));
    }
    if c.take(1, "version")?[0] != VERSION {
        return Err(DatasetError::Malformed("unsupported version".into()));
    }
    let release = std::str::from_utf8(c.field("release")?)
        .map_err(|_| DatasetError::Malformed("release is not UTF-8".into()))?
        .to_string();
    let body_bytes = c.field("body")?.to_vec();
    let signed_len = c.at;
    let env = SigEnvelope::parse(c.field("envelope")?)
        .map_err(|e| DatasetError::Malformed(e.to_string()))?;
    if c.at != file.len() {
        return Err(DatasetError::Malformed("trailing bytes".into()));
    }
    if env.domain != DEMO_DATASET.label() {
        return Err(DatasetError::WrongDomain);
    }
    let anchor = anchors
        .get(&env.key_id)
        .ok_or_else(|| DatasetError::unknown_key(&env.key_id))?;
    accept(
        &DEMO_DATASET,
        &file[..signed_len],
        anchor,
        &env.signature,
        &APPROVED,
    )
    .map_err(DatasetError::Signature)?;
    if release != expected_release {
        return Err(DatasetError::WrongRelease {
            found: release,
            expected: expected_release.to_string(),
        });
    }
    let body = canonical_body(&body_bytes)?;
    Ok(Dataset {
        release,
        key_id: env.key_id,
        body_bytes,
        body,
    })
}
