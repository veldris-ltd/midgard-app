//! At-rest custody for a software signing key.
//!
//! A keystore file holds an ML-KEM-1024 **root** and the ML-DSA **signing key
//! sealed under it** using the suite's existing KEM-DEM envelope
//! ([`kms::Kms::seal`]). The signing key is therefore never at rest in
//! plaintext: an attacker who obtains the sealed blob alone — from a backup, a
//! build cache, a leaked artefact directory — obtains nothing. `[NFR-PQ-2]`
//!
//! ## Format version 3: the root is wrapped, and the header is bound to it
//! The root secret is never in the file in the clear either. It is wrapped under a
//! [`RootWrap`] the caller supplies when writing and reading:
//!
//! - **kind 1, passphrase**: a key-encryption key is derived from an operator
//!   passphrase with PBKDF2-HMAC-SHA256 ([`KDF_ITERATIONS`] iterations, a fresh
//!   32-byte salt on every write) and the root secret is sealed under it with
//!   AES-256-GCM. A wrong passphrase is [`SignError::KeystoreLocked`], never
//!   "corrupt".
//! - **kind 2, external trust root**: the root secret is a [`kms::SealedBlob`]
//!   sealed to a [`kms::TrustRoot`] held elsewhere, an HSM or TPM behind
//!   [`kms::HardwareTrustRoot`]. This is the GA custody target (RF-SC-8).
//!
//! Either way the AAD of the wrapped root is its purpose followed by the **whole
//! header**, magic to `root_public`, exactly as stored. Whoever can write the file but
//! does not hold the passphrase cannot change the verifying key a reader trusts, nor
//! the root public key data is sealed to: any change to the header and the root no
//! longer unwraps. The signing key unsealed from the file must also sign under the
//! header's verifying key (a probe in [`kms::SoftwareSigningRoot::from_key`]), so a
//! signing key sealed afresh to the public root is refused too.
//!
//! ```text
//! magic          11  b"MGKEYSTORE1"
//! version         1  u8 = 3
//! key_id          4+n  the signing key's id
//! sig_alg         1  u8, SigId tag
//! verifying_key   4+n  public, exportable as a trust anchor
//! root_key_id     4+n
//! root_suite      1  u8, KemSuiteId tag
//! root_public     4+n  ML-KEM-1024 public key
//! wrap_kind       1  u8: 1 = passphrase, 2 = external trust root
//!  -- kind 1 --
//! kdf_salt       32
//! kdf_iterations  4  u32 LE, from KDF_ITERATIONS to KDF_ITERATIONS_MAX
//! root_nonce     12
//! root_cipher     4+n  AES-256-GCM(root secret), AAD = HEADER_AAD || header
//!  -- kind 2 --
//! root_blob       4+n  kms::SealedBlob::encode(), sealed to the external root
//!  --
//! sealed_suite    1  u8
//! sealed_root_id  4+n
//! sealed_wrapped  4+n  the ML-KEM encapsulation (the wrapped data key)
//! sealed_nonce   12
//! sealed_cipher   4+n  AES-256-GCM(signing key)
//! ```
//!
//! Version 1 kept the root secret in the clear between `root_public` and the sealed
//! blob, so whoever read the file held every key the root protects. Version 2 wrapped
//! the root as version 3 does, but its AAD bound only the key id, so the rest of the
//! header could be swapped without the passphrase. Every reader refuses both, as it
//! always refused version 1; [`migrate`] (run by `mg-admin keystore migrate`)
//! converts a file once, keeping the same root. `[VLD-ANB-MIDGARD-0007]`
//!
//! Key files are created owner-only (0600 on Unix) under a temporary name with
//! `create_new` and renamed into place, an existing keystore is never overwritten,
//! and key directories are created 0700 ([`create_private_dir`]).
//!
//! ## What the passphrase tier is
//! Software-tier custody: the root is as safe as the passphrase and the file are
//! kept apart. The passphrase is read from a file ([`RootWrap::passphrase_from_file`])
//! named by `--keystore-passphrase-file` or `MIDGARD_KEYSTORE_PASSPHRASE_FILE`,
//! never from the command line, and must not live beside the keystore or in its
//! backups. Library crates take a [`RootWrap`] and never read the environment. Key
//! hierarchy, rotation and revocation are RF-SC-8; a TPM/HSM root implements
//! [`kms::TrustRoot`] and slots in as kind 2 without changing anything above.

use std::path::{Path, PathBuf};

use kms::{Kms, SealedBlob, TrustRoot, WRAP_SUITE};
use pqc_agility::hash::pbkdf2_hmac_sha256;
use pqc_agility::{
    Aes256GcmAead, Ciphertext, Kem, KemPublicKey, KemSecretKey, KemSuiteId, Rng, SecretArray,
    SecretBytes, SharedSecret, SigId, SigningKey, VerifyingKey, GCM_NONCE_LEN,
};

use crate::error::SignError;

/// Keystore file magic.
pub const MAGIC: &[u8; 11] = b"MGKEYSTORE1";
/// Keystore format version: 3, the root secret wrapped under an AAD that binds the
/// whole header. `[VLD-ANB-MIDGARD-0007]`
pub const VERSION: u8 = 3;
/// The version that stored the root secret in the clear; refused by every reader.
const LEGACY_VERSION: u8 = 1;
/// The version whose wrapped root bound only the key id, not the rest of the header;
/// refused by every reader, read only by [`migrate`].
const UNBOUND_HEADER_VERSION: u8 = 2;
/// Plaintext dev-key file magic.
pub const KEY_MAGIC: &[u8; 6] = b"MGKEY1";
/// Plaintext dev-key file version (unchanged; not a keystore).
const KEY_VERSION: u8 = 1;

/// PBKDF2-HMAC-SHA256 iterations for the passphrase wrap, and the least a reader
/// accepts (OWASP's 2023 figure for PBKDF2-HMAC-SHA256).
pub const KDF_ITERATIONS: u32 = 600_000;
/// The most PBKDF2 iterations a reader accepts, so a file cannot make opening it
/// take hours: some sixteen times what is written.
pub const KDF_ITERATIONS_MAX: u32 = 10_000_000;
/// The shortest passphrase [`RootWrap::passphrase_from_file`] accepts.
pub const MIN_PASSPHRASE_LEN: usize = 16;
/// The environment variable that names the passphrase file, for binaries. Library
/// crates never read it.
pub const PASSPHRASE_ENV: &str = "MIDGARD_KEYSTORE_PASSPHRASE_FILE";
/// The command-line option that names the passphrase file.
pub const PASSPHRASE_FLAG: &str = "keystore-passphrase-file";

const KDF_SALT_LEN: usize = 32;
const WRAP_PASSPHRASE: u8 = 1;
const WRAP_EXTERNAL: u8 = 2;

/// AAD binding the sealed blob to its purpose, so a blob sealed for some other
/// use cannot be presented here as a signing key.
const SEAL_AAD: &[u8] = b"midgard-keystore/signing-key/v1";
/// AAD prefix of a version 2 wrapped root; the key id follows it.
const ROOT_AAD: &[u8] = b"midgard-keystore/root-secret/v2";
/// AAD prefix binding the wrapped root to its purpose; the whole encoded header
/// follows it (version 3).
const HEADER_AAD: &[u8] = b"midgard-keystore/root-secret/v3";

/// How a keystore's custody root is wrapped at rest: the value callers pass to
/// [`Keystore::write`] and [`Keystore::read`]. `[VLD-ANB-MIDGARD-0007]`
pub enum RootWrap<'a> {
    /// Under a key derived from an operator passphrase (kind 1).
    Passphrase(&'a SecretBytes),
    /// Sealed to an external trust root, an HSM or TPM (kind 2).
    External(&'a dyn TrustRoot),
}

impl RootWrap<'_> {
    /// Read a keystore passphrase from `path`: one trailing newline is stripped; an
    /// empty or shorter than [`MIN_PASSPHRASE_LEN`]-byte passphrase is refused, and on
    /// Unix so is a file any group or other user can read or write (wider than 0600).
    pub fn passphrase_from_file(path: &Path) -> Result<SecretBytes, SignError> {
        let refuse = |reason: String| SignError::KeyUnreadable {
            path: path.to_path_buf(),
            reason,
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path)
                .map_err(|e| refuse(e.to_string()))?
                .permissions()
                .mode();
            if mode & 0o077 != 0 {
                return Err(refuse(format!(
                    "the passphrase file is mode {:o}; it must be readable by its owner only \
                     (0600 or 0400)",
                    mode & 0o777
                )));
            }
        }
        let mut bytes = std::fs::read(path).map_err(|e| refuse(e.to_string()))?;
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        let passphrase = SecretBytes::new(bytes);
        if passphrase.is_empty() {
            return Err(refuse("the passphrase file is empty".into()));
        }
        if passphrase.len() < MIN_PASSPHRASE_LEN {
            return Err(refuse(format!(
                "the passphrase is shorter than {MIN_PASSPHRASE_LEN} bytes"
            )));
        }
        Ok(passphrase)
    }
}

/// The passphrase file a binary should use: `flag` (the value of
/// `--keystore-passphrase-file`) if given, else `env` (the value of
/// [`PASSPHRASE_ENV`], which the binary reads), else a fail-closed error naming
/// both. Taking the environment as a parameter keeps library crates from reading it.
pub fn passphrase_file(flag: Option<&str>, env: Option<String>) -> Result<PathBuf, String> {
    match (flag, env) {
        (Some(path), _) => Ok(PathBuf::from(path)),
        (None, Some(path)) if !path.is_empty() => Ok(PathBuf::from(path)),
        _ => Err(format!(
            "no keystore passphrase: pass --{PASSPHRASE_FLAG} PATH or set {PASSPHRASE_ENV}; \
             keystores wrap their custody root under it"
        )),
    }
}

/// [`passphrase_file`], read with [`RootWrap::passphrase_from_file`].
pub fn load_passphrase(flag: Option<&str>, env: Option<String>) -> Result<SecretBytes, String> {
    let path = passphrase_file(flag, env)?;
    RootWrap::passphrase_from_file(&path).map_err(|e| e.to_string())
}

/// A trust root reconstructed from stored ML-KEM key material.
///
/// `kms::SoftwareTrustRoot` can only be generated fresh, so this is the loading
/// counterpart. It implements the same public [`kms::TrustRoot`] trait, which is
/// the whole point of that trait being public.
pub struct KeystoreRoot {
    key_id: String,
    public_key: KemPublicKey,
    secret_key: KemSecretKey,
}

impl TrustRoot for KeystoreRoot {
    fn key_id(&self) -> &str {
        &self.key_id
    }
    fn public_key(&self) -> &KemPublicKey {
        &self.public_key
    }
    fn unwrap(&self, kem: &dyn Kem, wrapped: &Ciphertext) -> Result<SharedSecret, kms::KmsError> {
        kem.decapsulate(&self.secret_key, wrapped)
            .map_err(|e| kms::KmsError::Kem(e.to_string()))
    }
}

fn put(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

struct Cur<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .ok_or_else(|| format!("{what}: length overflows"))?;
        if end > self.b.len() {
            return Err(format!("{what}: truncated"));
        }
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self, what: &str) -> Result<u8, String> {
        Ok(self.take(1, what)?[0])
    }
    fn u32(&mut self, what: &str) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            self.take(4, what)?.try_into().expect("4 bytes"),
        ))
    }
    fn lp(&mut self, what: &str) -> Result<&'a [u8], String> {
        let n = self.u32(what)? as usize;
        self.take(n, what)
    }
    fn end(&self) -> Result<(), String> {
        if self.at != self.b.len() {
            return Err(format!("{} trailing byte(s)", self.b.len() - self.at));
        }
        Ok(())
    }
}

/// The fields before the root, common to both versions.
struct Header {
    key_id: String,
    alg: SigId,
    verifying_key: VerifyingKey,
    root_key_id: String,
    root_suite: KemSuiteId,
    root_public: Vec<u8>,
}

fn put_header(o: &mut Vec<u8>, version: u8, h: &Header) {
    o.extend_from_slice(MAGIC);
    o.push(version);
    put(o, h.key_id.as_bytes());
    o.push(h.alg.as_u8());
    put(o, h.verifying_key.as_bytes());
    put(o, h.root_key_id.as_bytes());
    o.push(h.root_suite.as_u8());
    put(o, &h.root_public);
}

fn take_header(c: &mut Cur) -> Result<Header, String> {
    let key_id = String::from_utf8(c.lp("key id")?.to_vec()).map_err(|_| "key id not UTF-8")?;
    let alg_tag = c.u8("algorithm")?;
    let alg = SigId::from_u8(alg_tag).ok_or(format!("unknown algorithm tag {alg_tag}"))?;
    let verifying_key = VerifyingKey::new(alg, c.lp("verifying key")?.to_vec());
    let root_key_id =
        String::from_utf8(c.lp("root key id")?.to_vec()).map_err(|_| "root key id not UTF-8")?;
    let rs_tag = c.u8("root suite")?;
    let root_suite = KemSuiteId::from_u8(rs_tag).ok_or(format!("unknown KEM suite {rs_tag}"))?;
    let root_public = c.lp("root public key")?.to_vec();
    Ok(Header {
        key_id,
        alg,
        verifying_key,
        root_key_id,
        root_suite,
        root_public,
    })
}

fn put_sealed(o: &mut Vec<u8>, sealed: &SealedBlob) {
    o.push(sealed.suite.as_u8());
    put(o, sealed.root_key_id.as_bytes());
    put(o, sealed.wrapped_key.as_bytes());
    o.extend_from_slice(&sealed.nonce);
    put(o, &sealed.ciphertext);
}

fn take_sealed(c: &mut Cur) -> Result<SealedBlob, String> {
    let ss_tag = c.u8("sealed suite")?;
    let suite = KemSuiteId::from_u8(ss_tag).ok_or(format!("unknown sealed suite {ss_tag}"))?;
    let root_key_id = String::from_utf8(c.lp("sealed root id")?.to_vec())
        .map_err(|_| "sealed root id not UTF-8")?;
    let wrapped = c.lp("wrapped key")?.to_vec();
    let nonce: [u8; GCM_NONCE_LEN] = c
        .take(GCM_NONCE_LEN, "nonce")?
        .try_into()
        .map_err(|_| "bad nonce length")?;
    let ciphertext = c.lp("sealed signing key")?.to_vec();
    Ok(SealedBlob {
        root_key_id,
        suite,
        wrapped_key: Ciphertext::new(suite, wrapped),
        nonce,
        ciphertext,
    })
}

/// A parse failure (the bytes are not a keystore) as opposed to a lock failure (they
/// are, and the wrap does not open them).
fn malformed(reason: impl Into<String>) -> SignError {
    SignError::KeyUnreadable {
        path: PathBuf::new(),
        reason: reason.into(),
    }
}

/// The message every reader gives for a version 1 file.
const V1_REFUSED: &str =
    "keystore version 1 stores the custody root in clear; run mg-admin keystore migrate";

/// The message every reader gives for a version 2 file.
const V2_REFUSED: &str = "keystore version 2 does not authenticate its header; run mg-admin \
                          keystore migrate";

/// The AAD for the wrapped root of a file of `version` whose encoded header is
/// `header`: its purpose, then the whole header (version 3), or only the key id
/// (version 2).
fn root_aad(version: u8, header: &[u8], key_id: &str) -> Vec<u8> {
    if version == UNBOUND_HEADER_VERSION {
        let mut aad = ROOT_AAD.to_vec();
        aad.extend_from_slice(key_id.as_bytes());
        return aad;
    }
    let mut aad = HEADER_AAD.to_vec();
    aad.extend_from_slice(header);
    aad
}

/// A fresh ML-KEM-1024 custody root.
fn new_root(rng: &mut dyn Rng) -> Result<(KemPublicKey, KemSecretKey), SignError> {
    pqc_agility::backend::fips::provider()
        .kem_for(WRAP_SUITE)
        .map_err(|e| SignError::Custodian(e.to_string()))?
        .keypair(rng)
        .map_err(|e| SignError::Custodian(e.to_string()))
}

/// The key-encryption key for a passphrase wrap. Derived straight into a zeroised,
/// mlocked buffer.
fn derive_kek(passphrase: &SecretBytes, salt: &[u8], iterations: u32) -> SecretArray<32> {
    let mut kek = SecretArray::<32>::zeroed();
    pbkdf2_hmac_sha256(passphrase.expose(), salt, iterations, kek.expose_mut());
    kek
}

/// A keystore, in memory. Its root is unlocked: [`Keystore::read`] has already
/// unwrapped it.
pub struct Keystore {
    /// The signing key's id.
    pub key_id: String,
    /// The signing algorithm.
    pub alg: SigId,
    /// The public verifying key.
    pub verifying_key: VerifyingKey,
    root_key_id: String,
    root_suite: KemSuiteId,
    root_public: Vec<u8>,
    /// Zeroised on drop and mlocked; never written out except wrapped.
    root_secret: SecretBytes,
    sealed: SealedBlob,
}

impl Keystore {
    /// Generate a fresh root and signing key, sealing the latter under the former.
    pub fn generate(key_id: &str, alg: SigId, rng: &mut dyn Rng) -> Result<Self, SignError> {
        let provider = pqc_agility::backend::fips::provider();

        // 1. The custody root (ML-KEM-1024).
        let root = new_root(rng)?;

        // 2. The signing key.
        let (verifying_key, signing_key) = provider
            .sig_for(alg)
            .map_err(|e| SignError::Custodian(e.to_string()))?
            .keypair(rng)
            .map_err(|e| SignError::Custodian(e.to_string()))?;

        // 3. Seal the signing key under the root.
        Self::sealed_under(
            key_id,
            format!("{key_id}-root"),
            root,
            verifying_key,
            &signing_key,
            rng,
        )
    }

    /// A keystore for `signing_key` under the custody root `root`, called
    /// `root_key_id`, the signing key sealed under it.
    fn sealed_under(
        key_id: &str,
        root_key_id: String,
        (root_public, root_secret): (KemPublicKey, KemSecretKey),
        verifying_key: VerifyingKey,
        signing_key: &SigningKey,
        rng: &mut dyn Rng,
    ) -> Result<Self, SignError> {
        let provider = pqc_agility::backend::fips::provider();
        let mut keystore = Self {
            key_id: key_id.to_string(),
            alg: signing_key.alg(),
            verifying_key,
            root_key_id,
            root_suite: WRAP_SUITE,
            root_public: root_public.as_bytes().to_vec(),
            root_secret: SecretBytes::new(root_secret.expose().to_vec()),
            sealed: SealedBlob {
                root_key_id: String::new(),
                suite: WRAP_SUITE,
                wrapped_key: Ciphertext::new(WRAP_SUITE, Vec::new()),
                nonce: [0; GCM_NONCE_LEN],
                ciphertext: Vec::new(),
            },
        };

        // The signing key is never at rest in the clear.
        keystore.sealed = Kms::new(provider, keystore.trust_root())
            .seal(SEAL_AAD, signing_key.expose(), rng)
            .map_err(|e| SignError::Custodian(e.to_string()))?;
        Ok(keystore)
    }

    /// The same signing key under a fresh custody root: the signing key is unsealed
    /// (and checked against the verifying key), a new ML-KEM-1024 root is generated as
    /// [`Keystore::generate`] does, with a new root key id, and the key is resealed
    /// under it. The key id, algorithm and verifying key are unchanged, so trust
    /// anchors stay valid; nothing sealed under the old root opens under the new one,
    /// so only a keystore whose root seals nothing but its own signing key may be
    /// rotated (`mg-admin keystore rotate-root` decides which). `[VLD-ANB-MIDGARD-0007]`
    pub fn rotate_root(&self, rng: &mut dyn Rng) -> Result<Self, SignError> {
        let signing = self.signing_root()?;
        let root = new_root(rng)?;
        let mut tag = [0u8; 8];
        rng.fill(&mut tag);
        Self::sealed_under(
            &self.key_id,
            format!("{}-root-{}", self.key_id, crate::hex::hex(&tag)),
            root,
            self.verifying_key.clone(),
            signing.expose_signing_key(),
            rng,
        )
    }

    /// Refuse a root public key that does not belong to the root secret: a probe
    /// sealed to the public key must unseal under the secret. Versions 1 and 2 did not
    /// authenticate the public key, and data sealed to a substituted one would be
    /// readable by whoever substituted it.
    fn check_root_pair(&self, rng: &mut dyn Rng) -> Result<(), SignError> {
        let kms = Kms::new(pqc_agility::backend::fips::provider(), self.trust_root());
        let aad = b"midgard-keystore/root-pair-probe/v1";
        let probe = kms
            .seal(aad, aad, rng)
            .map_err(|e| SignError::Custodian(e.to_string()))?;
        match kms.unseal(aad, &probe) {
            Ok(opened) if opened == aad => Ok(()),
            _ => Err(SignError::Custodian(
                "the keystore's root public key does not belong to its root secret".into(),
            )),
        }
    }

    fn header(&self) -> Header {
        Header {
            key_id: self.key_id.clone(),
            alg: self.alg,
            verifying_key: self.verifying_key.clone(),
            root_key_id: self.root_key_id.clone(),
            root_suite: self.root_suite,
            root_public: self.root_public.clone(),
        }
    }

    /// Serialise for storage as version 3, wrapping the root under `wrap` with the
    /// whole header as its AAD. A fresh salt and nonce (or a fresh encapsulation) are
    /// drawn from `rng` on every call, so re-wrapping never reuses a nonce.
    pub fn encode(&self, wrap: &RootWrap, rng: &mut dyn Rng) -> Result<Vec<u8>, SignError> {
        self.encode_as(VERSION, wrap, rng)
    }

    /// [`Keystore::encode`] as `version`: 3, or 2 for the retained test encoder.
    fn encode_as(
        &self,
        version: u8,
        wrap: &RootWrap,
        rng: &mut dyn Rng,
    ) -> Result<Vec<u8>, SignError> {
        let mut o = Vec::new();
        put_header(&mut o, version, &self.header());
        let aad = root_aad(version, &o, &self.key_id);
        match wrap {
            RootWrap::Passphrase(passphrase) => {
                let mut salt = [0u8; KDF_SALT_LEN];
                rng.fill(&mut salt);
                let mut nonce = [0u8; GCM_NONCE_LEN];
                rng.fill(&mut nonce);
                let kek = derive_kek(passphrase, &salt, KDF_ITERATIONS);
                let cipher = Aes256GcmAead::new(kek.expose())
                    .seal(&nonce, &aad, self.root_secret.expose())
                    .map_err(|e| SignError::Custodian(format!("cannot wrap the root: {e}")))?;
                o.push(WRAP_PASSPHRASE);
                o.extend_from_slice(&salt);
                o.extend_from_slice(&KDF_ITERATIONS.to_le_bytes());
                o.extend_from_slice(&nonce);
                put(&mut o, &cipher);
            }
            RootWrap::External(root) => {
                let blob = kms::seal_to_root(
                    &pqc_agility::backend::fips::provider(),
                    *root,
                    &aad,
                    self.root_secret.expose(),
                    rng,
                )
                .map_err(|e| SignError::Custodian(format!("cannot wrap the root: {e}")))?;
                o.push(WRAP_EXTERNAL);
                put(&mut o, &blob.encode());
            }
        }
        put_sealed(&mut o, &self.sealed);
        Ok(o)
    }

    /// Parse a keystore and unwrap its root under `wrap`. Every malformed field is
    /// a rejection ([`SignError::KeyUnreadable`]); so are version 1, which kept the
    /// root in the clear, and version 2, whose header is not authenticated: both name
    /// `mg-admin keystore migrate`. A wrap that does not open the root (a wrong
    /// passphrase, another trust root, the other wrap kind, or a header altered since
    /// the file was written) is [`SignError::KeystoreLocked`].
    pub fn decode(bytes: &[u8], wrap: &RootWrap) -> Result<Self, SignError> {
        Self::decode_versions(bytes, wrap, &[VERSION])
    }

    /// [`Keystore::decode`], accepting each version in `accept` (3, and 2 for
    /// [`migrate`] alone).
    fn decode_versions(bytes: &[u8], wrap: &RootWrap, accept: &[u8]) -> Result<Self, SignError> {
        let mut c = Cur { b: bytes, at: 0 };
        if c.take(MAGIC.len(), "magic").map_err(malformed)? != MAGIC {
            return Err(malformed("bad magic — not an MGKEYSTORE1 file"));
        }
        let version = c.u8("version").map_err(malformed)?;
        match version {
            v if accept.contains(&v) => {}
            LEGACY_VERSION => return Err(malformed(V1_REFUSED)),
            UNBOUND_HEADER_VERSION => return Err(malformed(V2_REFUSED)),
            v => return Err(malformed(format!("unsupported keystore version {v}"))),
        }
        let header = take_header(&mut c).map_err(malformed)?;
        let aad = root_aad(version, &bytes[..c.at], &header.key_id);
        let kind = c.u8("wrap kind").map_err(malformed)?;
        let (root_secret, sealed) = match kind {
            WRAP_PASSPHRASE => {
                let salt = c.take(KDF_SALT_LEN, "kdf salt").map_err(malformed)?;
                let iterations = c.u32("kdf iterations").map_err(malformed)?;
                if iterations < KDF_ITERATIONS {
                    return Err(malformed(format!(
                        "kdf iterations {iterations} is below the minimum {KDF_ITERATIONS}"
                    )));
                }
                if iterations > KDF_ITERATIONS_MAX {
                    return Err(malformed(format!(
                        "kdf iterations {iterations} is above the maximum {KDF_ITERATIONS_MAX}"
                    )));
                }
                let nonce: [u8; GCM_NONCE_LEN] = c
                    .take(GCM_NONCE_LEN, "root nonce")
                    .map_err(malformed)?
                    .try_into()
                    .expect("nonce length");
                let cipher = c.lp("wrapped root").map_err(malformed)?;
                let sealed = take_sealed(&mut c).map_err(malformed)?;
                c.end().map_err(malformed)?;
                let RootWrap::Passphrase(passphrase) = wrap else {
                    return Err(SignError::KeystoreLocked(
                        "the root is wrapped under a passphrase, and none was given".into(),
                    ));
                };
                let kek = derive_kek(passphrase, salt, iterations);
                let root = Aes256GcmAead::new(kek.expose())
                    .open(&nonce, &aad, cipher)
                    .map_err(|_| {
                        SignError::KeystoreLocked(
                            "the passphrase does not unlock the custody root, or the header \
                             has been altered"
                                .into(),
                        )
                    })?;
                (SecretBytes::new(root), sealed)
            }
            WRAP_EXTERNAL => {
                let blob = SealedBlob::decode(c.lp("root blob").map_err(malformed)?)
                    .map_err(|e| malformed(format!("root blob: {e}")))?;
                let sealed = take_sealed(&mut c).map_err(malformed)?;
                c.end().map_err(malformed)?;
                let RootWrap::External(root) = wrap else {
                    return Err(SignError::KeystoreLocked(
                        "the root is sealed to an external trust root, and none was given".into(),
                    ));
                };
                if blob.root_key_id != root.key_id() {
                    return Err(SignError::KeystoreLocked(format!(
                        "the root is sealed to trust root '{}', not '{}'",
                        blob.root_key_id,
                        root.key_id()
                    )));
                }
                let secret = kms::unseal_from_root(
                    &pqc_agility::backend::fips::provider(),
                    *root,
                    &aad,
                    &blob,
                )
                .map_err(|e| {
                    SignError::KeystoreLocked(format!(
                        "the trust root does not unlock the custody root: {e}"
                    ))
                })?;
                (SecretBytes::new(secret), sealed)
            }
            k => return Err(malformed(format!("unknown root wrap kind {k}"))),
        };
        Ok(Self {
            key_id: header.key_id,
            alg: header.alg,
            verifying_key: header.verifying_key,
            root_key_id: header.root_key_id,
            root_suite: header.root_suite,
            root_public: header.root_public,
            root_secret,
            sealed,
        })
    }

    /// Unseal the signing key and build a live custodian, keeping the keystore.
    ///
    /// A tampered sealed blob, a mismatched root, or a disabled algorithm all
    /// fail here rather than producing a custodian that signs wrongly.
    pub fn signing_root(&self) -> Result<kms::SoftwareSigningRoot, SignError> {
        let key_bytes = Kms::new(pqc_agility::backend::fips::provider(), self.trust_root())
            .unseal(SEAL_AAD, &self.sealed)
            .map_err(|e| SignError::Custodian(format!("cannot unseal signing key: {e}")))?;

        kms::SoftwareSigningRoot::from_key(
            pqc_agility::backend::fips::provider(),
            &self.key_id,
            self.verifying_key.clone(),
            SigningKey::new(self.alg, SecretBytes::new(key_bytes)),
        )
        .map_err(|e| SignError::Custodian(e.to_string()))
    }

    /// Unseal the signing key and build a live custodian. See
    /// [`Keystore::signing_root`].
    pub fn into_signing_root(self) -> Result<kms::SoftwareSigningRoot, SignError> {
        self.signing_root()
    }

    /// The custody root this keystore seals its signing key under, as a live
    /// [`kms::TrustRoot`].
    ///
    /// A service's keystore is then its one custody device: the same root that keeps
    /// its signing key sealed can seal its data at rest — object store, audit ledger,
    /// identity directory — through `kms::KmsSealer`. The root is copied once, into
    /// the returned value's zeroised buffer.
    pub fn trust_root(&self) -> KeystoreRoot {
        KeystoreRoot {
            key_id: self.root_key_id.clone(),
            public_key: KemPublicKey::new(self.root_suite, self.root_public.clone()),
            secret_key: KemSecretKey::new(
                self.root_suite,
                SecretBytes::new(self.root_secret.expose().to_vec()),
            ),
        }
    }

    /// Read a keystore from disk and unwrap its root under `wrap`.
    pub fn read(path: &Path, wrap: &RootWrap) -> Result<Self, SignError> {
        let bytes = std::fs::read(path).map_err(|e| SignError::KeyUnreadable {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
        Self::decode(&bytes, wrap).map_err(|e| at_path(e, path))
    }

    /// Write a keystore to disk, its root wrapped under `wrap`: created owner-only
    /// under a temporary name and renamed into place. An existing file is an error.
    pub fn write(&self, path: &Path, wrap: &RootWrap, rng: &mut dyn Rng) -> Result<(), SignError> {
        write_restricted(path, &self.encode(wrap, rng)?)
    }

    /// The id of the custody root the signing key is sealed under.
    pub fn root_key_id(&self) -> &str {
        &self.root_key_id
    }

    /// Export the verifying key as a trust anchor: `<alg> <key-id> <hex>`.
    pub fn export_verifying_key(&self) -> String {
        format!(
            "{} {} {}\n",
            self.alg.as_str(),
            self.key_id,
            crate::hex::hex(self.verifying_key.as_bytes())
        )
    }
}

/// Put the file's path into an error from [`Keystore::decode`].
fn at_path(e: SignError, path: &Path) -> SignError {
    match e {
        SignError::KeyUnreadable { reason, .. } => SignError::KeyUnreadable {
            path: path.to_path_buf(),
            reason,
        },
        SignError::KeystoreLocked(reason) => {
            SignError::KeystoreLocked(format!("{}: {reason}", path.display()))
        }
        other => other,
    }
}

/// The version of a keystore file's bytes, if they start like one.
pub fn format_version(bytes: &[u8]) -> Option<u8> {
    (bytes.len() > MAGIC.len() && &bytes[..MAGIC.len()] == MAGIC).then(|| bytes[MAGIC.len()])
}

/// Parse a version 1 keystore, root in the clear, into an unlocked keystore. Kept
/// only for [`migrate`] (and the legacy test encoder); readers never call it.
fn decode_v1(bytes: &[u8]) -> Result<Keystore, String> {
    let mut c = Cur { b: bytes, at: 0 };
    if c.take(MAGIC.len(), "magic")? != MAGIC {
        return Err("bad magic — not an MGKEYSTORE1 file".into());
    }
    let v = c.u8("version")?;
    if v != LEGACY_VERSION {
        return Err(format!("not a version 1 keystore (version {v})"));
    }
    let header = take_header(&mut c)?;
    let root_secret = SecretBytes::new(c.lp("root secret key")?.to_vec());
    let sealed = take_sealed(&mut c)?;
    c.end()?;
    Ok(Keystore {
        key_id: header.key_id,
        alg: header.alg,
        verifying_key: header.verifying_key,
        root_key_id: header.root_key_id,
        root_suite: header.root_suite,
        root_public: header.root_public,
        root_secret,
        sealed,
    })
}

/// Convert a version 1 or version 2 keystore to version 3, its root wrapped under
/// `wrap` (a version 2 file must also unlock under it). Neither earlier version
/// authenticates its header, so before anything is written the header's root public
/// key must match the root secret and its verifying key the sealed signing key; then
/// the new bytes are decoded under `wrap` and must unseal the same signing key, under
/// the same verifying key, as the old file. Any other version is an error. The root
/// bytes are kept: where an earlier copy may have been exposed, rotate the root
/// ([`Keystore::rotate_root`]) where that is allowed, or re-initialise.
/// `[VLD-ANB-MIDGARD-0007]`
pub fn migrate(bytes: &[u8], wrap: &RootWrap, rng: &mut dyn Rng) -> Result<Vec<u8>, SignError> {
    let legacy = match format_version(bytes) {
        Some(LEGACY_VERSION) => decode_v1(bytes).map_err(malformed)?,
        Some(UNBOUND_HEADER_VERSION) => {
            Keystore::decode_versions(bytes, wrap, &[UNBOUND_HEADER_VERSION])?
        }
        Some(v) => {
            return Err(malformed(format!(
                "keystore version {v} is not one migration converts (1 or 2)"
            )))
        }
        None => return Err(malformed("bad magic — not an MGKEYSTORE1 file")),
    };
    legacy.check_root_pair(rng)?;
    let before = legacy.signing_root()?;
    let migrated = legacy.encode(wrap, rng)?;

    let after = Keystore::decode(&migrated, wrap)?;
    let after_root = after.signing_root()?;
    use kms::SigningRoot as _;
    if after.verifying_key != legacy.verifying_key
        || after_root.verifying_key() != before.verifying_key()
        || !pqc_agility::ct_eq(
            after_root.expose_signing_key().expose(),
            before.expose_signing_key().expose(),
        )
    {
        return Err(SignError::Custodian(
            "the migrated keystore does not unseal the same signing key".into(),
        ));
    }
    Ok(migrated)
}

/// Create a key directory (and any missing parents) owner-only: 0700 on Unix, and
/// an existing directory is narrowed to 0700. Elsewhere the ACL is inherited.
pub fn create_private_dir(path: &Path) -> Result<(), SignError> {
    let fail = |e: std::io::Error| SignError::Write {
        path: path.to_path_buf(),
        reason: e.to_string(),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(fail)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(fail)?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(path).map_err(fail)?;
    Ok(())
}

/// Write a file containing key material: created owner-only (0600 on Unix) under
/// `<path>.tmp` with `create_new`, flushed, then renamed to `path`, so a partial file
/// never carries the name and the mode is never wider, even for an instant. An
/// existing `path` is an error. On Windows the ACL is inherited; the keystore's real
/// protection there is the wrap.
pub fn write_restricted(path: &Path, bytes: &[u8]) -> Result<(), SignError> {
    use std::io::Write as _;
    let fail = |reason: String| SignError::Write {
        path: path.to_path_buf(),
        reason,
    };
    if path.exists() {
        return Err(fail(
            "the file already exists; refusing to overwrite key material".into(),
        ));
    }
    let mut tmp_name = path.as_os_str().to_os_string();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp).map_err(|e| {
        fail(format!(
            "cannot create {}: {e} (a leftover from an interrupted write may be removed)",
            tmp.display()
        ))
    })?;
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            drop(file);
            std::fs::rename(&tmp, path)
        });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(fail(e.to_string()));
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        // Make the rename durable; best effort, the file itself is already synced.
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

/// Fixtures for tests that write and read keystores, in this crate and downstream
/// (the `test-support` feature). Nothing here is a secret.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::*;

    /// The passphrase every test keystore is wrapped under. Inert by construction.
    pub const TEST_PASSPHRASE: &[u8] = b"anubis-test-passphrase-not-a-secret";

    /// [`TEST_PASSPHRASE`] as a [`SecretBytes`], for [`RootWrap::Passphrase`].
    pub fn test_passphrase() -> SecretBytes {
        SecretBytes::new(TEST_PASSPHRASE.to_vec())
    }

    /// Write [`TEST_PASSPHRASE`] to `<dir>/keystore.passphrase`, owner-only, for
    /// binaries under test (pass it as `MIDGARD_KEYSTORE_PASSPHRASE_FILE`). Returns
    /// the path; an existing file is reused, and tests racing to create it are safe
    /// (each writes its own temporary file and renames it into place).
    pub fn test_passphrase_file(dir: &Path) -> PathBuf {
        let path = dir.join("keystore.passphrase");
        if !path.exists() {
            std::fs::create_dir_all(dir).expect("passphrase directory");
            let mut nonce = [0u8; 8];
            pqc_agility::Rng::fill(&mut pqc_agility::OsRng, &mut nonce);
            let tmp = dir.join(format!(
                "keystore.passphrase.{}.{}",
                std::process::id(),
                u64::from_le_bytes(nonce)
            ));
            write_restricted(&tmp, TEST_PASSPHRASE).expect("write the test passphrase");
            std::fs::rename(&tmp, &path).expect("place the test passphrase");
        }
        path
    }

    /// [`test_passphrase_file`] in one directory under the system temporary
    /// directory, shared by every test binary.
    pub fn shared_test_passphrase_file() -> PathBuf {
        test_passphrase_file(&std::env::temp_dir().join("midgard-test-keystore-passphrase"))
    }

    /// Generate and write a keystore wrapped under [`TEST_PASSPHRASE`].
    pub fn write_test_keystore(keystore: &Keystore, path: &Path) {
        keystore
            .write(
                path,
                &RootWrap::Passphrase(&test_passphrase()),
                &mut pqc_agility::OsRng,
            )
            .expect("write test keystore");
    }

    /// Read a keystore wrapped under [`TEST_PASSPHRASE`].
    pub fn read_test_keystore(path: &Path) -> Keystore {
        Keystore::read(path, &RootWrap::Passphrase(&test_passphrase())).expect("read test keystore")
    }

    /// The retained version 2 encoder: the root wrapped, its AAD binding only the key
    /// id. Only for proving that readers refuse it and that [`migrate`] converts it.
    pub fn encode_v2(keystore: &Keystore) -> Vec<u8> {
        keystore
            .encode_as(
                UNBOUND_HEADER_VERSION,
                &RootWrap::Passphrase(&test_passphrase()),
                &mut pqc_agility::OsRng,
            )
            .expect("encode version 2")
    }

    /// The retained version 1 encoder: the root secret in the clear. Only for
    /// proving that readers refuse it and that [`migrate`] converts it.
    pub fn encode_v1(keystore: &Keystore) -> Vec<u8> {
        let mut o = Vec::new();
        put_header(&mut o, LEGACY_VERSION, &keystore.header());
        put(&mut o, keystore.root_secret.expose());
        put_sealed(&mut o, &keystore.sealed);
        o
    }

    /// The root secret's bytes, for tests that search a file for them.
    pub fn root_secret_bytes(keystore: &Keystore) -> Vec<u8> {
        keystore.root_secret.expose().to_vec()
    }
}

/// A plaintext development key file. Only [`crate::backend::FileSigner`] consumes
/// it, and only with the environment opt-in.
pub struct DevKeyFile {
    /// Key id.
    pub key_id: String,
    /// Algorithm.
    pub alg: SigId,
    /// Public key.
    pub verifying_key: VerifyingKey,
    /// Secret key, in the clear — which is the entire reason this mode is gated.
    pub signing_key: SigningKey,
}

impl DevKeyFile {
    /// Serialise.
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        o.extend_from_slice(KEY_MAGIC);
        o.push(KEY_VERSION);
        put(&mut o, self.key_id.as_bytes());
        o.push(self.alg.as_u8());
        put(&mut o, self.verifying_key.as_bytes());
        put(&mut o, self.signing_key.expose());
        o
    }

    /// Parse.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut c = Cur { b: bytes, at: 0 };
        if c.take(KEY_MAGIC.len(), "magic")? != KEY_MAGIC {
            return Err("bad magic — not an MGKEY1 file".into());
        }
        let v = c.u8("version")?;
        if v != KEY_VERSION {
            return Err(format!("unsupported key version {v}"));
        }
        let key_id = String::from_utf8(c.lp("key id")?.to_vec()).map_err(|_| "key id not UTF-8")?;
        let alg_tag = c.u8("algorithm")?;
        let alg = SigId::from_u8(alg_tag).ok_or(format!("unknown algorithm tag {alg_tag}"))?;
        let verifying_key = VerifyingKey::new(alg, c.lp("verifying key")?.to_vec());
        let signing_key = SigningKey::new(alg, SecretBytes::new(c.lp("signing key")?.to_vec()));
        if c.at != bytes.len() {
            return Err(format!("{} trailing byte(s)", bytes.len() - c.at));
        }
        Ok(Self {
            key_id,
            alg,
            verifying_key,
            signing_key,
        })
    }

    /// Read from disk.
    pub fn read(path: &Path) -> Result<Self, SignError> {
        let bytes = std::fs::read(path).map_err(|e| SignError::KeyUnreadable {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
        Self::decode(&bytes).map_err(|reason| SignError::KeyUnreadable {
            path: path.to_path_buf(),
            reason,
        })
    }

    /// Write to disk.
    pub fn write(&self, path: &Path) -> Result<(), SignError> {
        write_restricted(path, &self.encode())
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{encode_v1, encode_v2, root_secret_bytes, test_passphrase};
    use super::*;
    use kms::SigningRoot;
    // For `verify` on the concrete `FipsSig`; production paths go through
    // `&dyn SignatureScheme` and need no import.
    use pqc_agility::SignatureScheme;

    fn ks() -> Keystore {
        let mut rng = pqc_agility::OsRng;
        Keystore::generate("release-key-1", kms::RELEASE_SIG_ALG, &mut rng).expect("generate")
    }

    fn encode(k: &Keystore) -> Vec<u8> {
        k.encode(
            &RootWrap::Passphrase(&test_passphrase()),
            &mut pqc_agility::OsRng,
        )
        .expect("encode")
    }

    fn decode(bytes: &[u8]) -> Result<Keystore, SignError> {
        Keystore::decode(bytes, &RootWrap::Passphrase(&test_passphrase()))
    }

    #[test]
    fn round_trips_and_unseals_a_working_custodian() {
        let k = ks();
        let vk = k.verifying_key.clone();
        let decoded = decode(&encode(&k)).expect("decode");
        assert_eq!(decoded.key_id, "release-key-1");
        assert_eq!(decoded.alg, SigId::MlDsa87);

        let root = decoded.into_signing_root().expect("unseal");
        let sig = root.sign(b"payload").expect("sign");
        pqc_agility::backend::fips::FipsSig::new(SigId::MlDsa87)
            .verify(&vk, b"payload", &sig)
            .expect("the unsealed key matches the exported verifying key");
    }

    #[test]
    fn signing_key_is_not_present_in_the_clear() {
        let k = ks();
        let secret = k
            .signing_root()
            .unwrap()
            .expose_signing_key()
            .expose()
            .to_vec();
        let file = encode(&k);
        assert!(
            !file.windows(secret.len()).any(|w| w == secret.as_slice()),
            "the signing key must never appear in the keystore in plaintext"
        );
    }

    // [AC-50.1] [VLD-ANB-MIDGARD-0007]
    #[test]
    fn root_secret_is_not_present_in_the_clear() {
        let k = ks();
        let secret = root_secret_bytes(&k);
        let file = encode(&k);
        assert_eq!(file[MAGIC.len()], 3, "the version byte is 3");
        assert!(
            !file.windows(secret.len()).any(|w| w == secret.as_slice()),
            "the custody root must never appear in the keystore in plaintext"
        );
    }

    // [AC-50.2] [VLD-ANB-MIDGARD-0007]
    #[test]
    fn a_wrong_passphrase_is_locked_not_corrupt() {
        let file = encode(&ks());
        let wrong = SecretBytes::new(b"wrong-passphrase-0000".to_vec());
        match Keystore::decode(&file, &RootWrap::Passphrase(&wrong)) {
            Err(SignError::KeystoreLocked(reason)) => assert!(!reason.contains("corrupt")),
            Err(e) => panic!("expected KeystoreLocked, got {e}"),
            Ok(_) => panic!("a wrong passphrase must not unlock the keystore"),
        }
    }

    #[test]
    fn rewrapping_draws_a_fresh_salt_and_nonce() {
        let k = ks();
        assert_ne!(
            encode(&k),
            encode(&k),
            "no salt or nonce reuse across writes"
        );
    }

    #[test]
    fn a_tampered_sealed_blob_fails_to_unseal() {
        let k = ks();
        let mut bytes = encode(&k);
        // Corrupt the last byte, which lies inside the AES-GCM ciphertext.
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        let decoded = decode(&bytes).expect("still parses");
        assert!(
            decoded.into_signing_root().is_err(),
            "AEAD must reject a tampered sealed signing key"
        );
    }

    /// Where the header's verifying key and root public key lie in an encoded file.
    fn header_ranges(bytes: &[u8]) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        let mut c = Cur { b: bytes, at: 0 };
        c.take(MAGIC.len() + 1, "").unwrap();
        c.lp("key id").unwrap();
        c.u8("alg").unwrap();
        let vk = c.lp("verifying key").unwrap().len();
        let vk = c.at - vk..c.at;
        c.lp("root key id").unwrap();
        c.u8("root suite").unwrap();
        let rp = c.lp("root public").unwrap().len();
        (vk, c.at - rp..c.at)
    }

    // A header field swapped in the file, with no passphrase, is refused at decode:
    // the whole header is the wrapped root's AAD. [AC-50.2] [VLD-ANB-MIDGARD-0007]
    #[test]
    fn a_swapped_verifying_key_or_root_public_fails_to_decode() {
        let k = ks();
        let other = encode(&ks());
        let good = encode(&k);
        let (vk, rp) = header_ranges(&good);
        let (other_vk, other_rp) = header_ranges(&other);
        assert!(decode(&good).is_ok());

        let mut bad = good.clone();
        bad[vk.clone()].copy_from_slice(&other[other_vk]);
        assert!(
            decode(&bad).is_err(),
            "a swapped verifying key must not decode"
        );

        let mut bad = good.clone();
        bad[rp.clone()].copy_from_slice(&other[other_rp]);
        assert!(
            decode(&bad).is_err(),
            "a swapped root public key must not decode"
        );
    }

    #[test]
    fn rejects_bad_magic_version_trailing_bytes_and_weak_kdf() {
        let k = ks();
        let good = encode(&k);

        let mut bad = good.clone();
        bad[0] = b'X';
        assert!(decode(&bad).is_err());

        let mut bad = good.clone();
        bad[MAGIC.len()] = 9;
        assert!(decode(&bad).is_err());

        let mut bad = good.clone();
        bad.push(0);
        assert!(decode(&bad).is_err());

        assert!(decode(&good[..good.len() - 1]).is_err());

        // A file claiming fewer KDF iterations than the floor is refused outright.
        let mut c = Cur { b: &good, at: 0 };
        c.take(MAGIC.len() + 1, "").unwrap();
        take_header(&mut c).unwrap();
        let iterations_at = c.at + 1 + KDF_SALT_LEN;
        let mut bad = good.clone();
        bad[iterations_at..iterations_at + 4].copy_from_slice(&1000u32.to_le_bytes());
        match decode(&bad) {
            Err(SignError::KeyUnreadable { reason, .. }) => assert!(reason.contains("minimum")),
            other => panic!("expected a refusal, got {:?}", other.err()),
        }

        // And so is one claiming more than the ceiling, before any key is derived: a
        // file must not be able to make its reader spin for hours.
        let mut bad = good.clone();
        bad[iterations_at..iterations_at + 4]
            .copy_from_slice(&(KDF_ITERATIONS_MAX + 1).to_le_bytes());
        match decode(&bad) {
            Err(SignError::KeyUnreadable { reason, .. }) => assert!(reason.contains("maximum")),
            other => panic!("expected a refusal, got {:?}", other.err()),
        }
    }

    // [AC-50.3] [VLD-ANB-MIDGARD-0007]
    #[test]
    fn version_1_is_refused_with_the_migrate_hint() {
        let v1 = encode_v1(&ks());
        assert_eq!(format_version(&v1), Some(1));
        match decode(&v1) {
            Err(SignError::KeyUnreadable { reason, .. }) => {
                assert!(reason.contains("keystore migrate"), "{reason}")
            }
            other => panic!("expected a refusal, got {:?}", other.err()),
        }
    }

    // Version 2, whose header is not authenticated, is refused as version 1 is, and
    // migrates to version 3 with the same root and key. [AC-50.3] [VLD-ANB-MIDGARD-0007]
    #[test]
    fn version_2_is_refused_with_the_migrate_hint_and_migrates() {
        let k = ks();
        let v2 = encode_v2(&k);
        assert_eq!(format_version(&v2), Some(2));
        match decode(&v2) {
            Err(SignError::KeyUnreadable { reason, .. }) => {
                assert!(reason.contains("keystore migrate"), "{reason}")
            }
            other => panic!("expected a refusal, got {:?}", other.err()),
        }
        let passphrase = test_passphrase();
        let wrap = RootWrap::Passphrase(&passphrase);
        let v3 = migrate(&v2, &wrap, &mut pqc_agility::OsRng).expect("migrate");
        assert_eq!(format_version(&v3), Some(VERSION));
        let migrated = decode(&v3).expect("decodes");
        assert_eq!(root_secret_bytes(&migrated), root_secret_bytes(&k));
        assert_eq!(migrated.verifying_key, k.verifying_key);
        assert!(
            migrate(&v3, &wrap, &mut pqc_agility::OsRng).is_err(),
            "a current file is not migrated"
        );
    }

    // A version 2 file whose unauthenticated header was swapped is not carried into
    // version 3, where the swap would become authenticated. [VLD-ANB-MIDGARD-0007]
    #[test]
    fn migration_refuses_a_version_2_file_with_a_swapped_header() {
        let passphrase = test_passphrase();
        let wrap = RootWrap::Passphrase(&passphrase);
        let good = encode_v2(&ks());
        let other = encode_v2(&ks());
        let (vk, rp) = header_ranges(&good);
        let (other_vk, other_rp) = header_ranges(&other);
        for (range, from) in [(vk, other_vk), (rp, other_rp)] {
            let mut bad = good.clone();
            bad[range].copy_from_slice(&other[from]);
            assert!(migrate(&bad, &wrap, &mut pqc_agility::OsRng).is_err());
        }
    }

    // A signing key sealed afresh to the (public) root, beside the original verifying
    // key, is refused by the probe rather than trusted. [NFR-PQ-3] [VLD-ANB-MIDGARD-0007]
    #[test]
    fn a_signing_key_resealed_to_the_public_root_is_refused() {
        let mut rng = pqc_agility::OsRng;
        let mut k = ks();
        let impostor = ks().signing_root().unwrap();
        k.sealed = Kms::new(crate::provider(), k.trust_root())
            .seal(SEAL_AAD, impostor.expose_signing_key().expose(), &mut rng)
            .unwrap();
        let reread = decode(&encode(&k)).expect("the header is intact, so it decodes");
        assert!(
            reread.signing_root().is_err(),
            "a signing key that is not the verifying key's own must not load"
        );
    }

    // Rotating the root keeps the signing key and verifying key and changes the root:
    // data sealed under the old root no longer opens. [VLD-ANB-MIDGARD-0007]
    #[test]
    fn rotating_the_root_keeps_the_key_and_replaces_the_root() {
        let mut rng = pqc_agility::OsRng;
        let k = ks();
        let sealed = Kms::new(crate::provider(), k.trust_root())
            .seal(b"store/0", b"object bytes", &mut rng)
            .unwrap();
        let rotated = decode(&encode(&k.rotate_root(&mut rng).expect("rotate"))).unwrap();
        assert_eq!(rotated.key_id, k.key_id);
        assert_eq!(rotated.verifying_key, k.verifying_key);
        assert_ne!(rotated.root_key_id(), k.root_key_id());
        assert_ne!(root_secret_bytes(&rotated), root_secret_bytes(&k));
        assert_ne!(rotated.root_public, k.root_public);
        assert!(ct_eq_keys(&rotated, &k), "the same signing key");
        assert!(Kms::new(crate::provider(), rotated.trust_root())
            .unseal(b"store/0", &sealed)
            .is_err());
    }

    fn ct_eq_keys(a: &Keystore, b: &Keystore) -> bool {
        pqc_agility::ct_eq(
            a.signing_root().unwrap().expose_signing_key().expose(),
            b.signing_root().unwrap().expose_signing_key().expose(),
        )
    }

    #[test]
    fn exports_a_parseable_trust_anchor_line() {
        let k = ks();
        let line = k.export_verifying_key();
        let parts: Vec<&str> = line.trim().split(' ').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "ML-DSA-87");
        assert_eq!(parts[1], "release-key-1");
        assert_eq!(
            crate::hex::unhex(parts[2]).unwrap(),
            k.verifying_key.as_bytes()
        );
    }

    #[test]
    fn dev_key_file_round_trips() {
        let mut rng = pqc_agility::OsRng;
        let p = crate::provider();
        let (vk, sk) = p
            .sig_for(SigId::MlDsa87)
            .unwrap()
            .keypair(&mut rng)
            .unwrap();
        let f = DevKeyFile {
            key_id: "dev".into(),
            alg: SigId::MlDsa87,
            verifying_key: vk,
            signing_key: sk,
        };
        let d = DevKeyFile::decode(&f.encode()).expect("decode");
        assert_eq!(d.key_id, "dev");
        assert_eq!(d.signing_key.expose(), f.signing_key.expose());
    }

    #[test]
    fn the_trust_root_seals_data_that_a_reloaded_keystore_unseals() {
        let mut rng = pqc_agility::OsRng;
        let k = ks();
        let sealed = Kms::new(crate::provider(), k.trust_root())
            .seal(b"store/0", b"object bytes", &mut rng)
            .expect("seal");

        let reloaded = decode(&encode(&k)).expect("decode");
        let opened = Kms::new(crate::provider(), reloaded.trust_root())
            .unseal(b"store/0", &sealed)
            .expect("the same root, reloaded from the file, unseals");
        assert_eq!(opened, b"object bytes");

        let other = Keystore::generate("other", kms::RELEASE_SIG_ALG, &mut rng).expect("generate");
        assert!(
            Kms::new(crate::provider(), other.trust_root())
                .unseal(b"store/0", &sealed)
                .is_err(),
            "another keystore's root cannot unseal it"
        );
    }

    #[test]
    fn the_passphrase_source_fails_closed_naming_both() {
        assert_eq!(
            passphrase_file(Some("/a"), Some("/b".into())).unwrap(),
            PathBuf::from("/a")
        );
        assert_eq!(
            passphrase_file(None, Some("/b".into())).unwrap(),
            PathBuf::from("/b")
        );
        for env in [None, Some(String::new())] {
            let e = passphrase_file(None, env).unwrap_err();
            assert!(e.contains("--keystore-passphrase-file"), "{e}");
            assert!(e.contains("MIDGARD_KEYSTORE_PASSPHRASE_FILE"), "{e}");
        }
    }
}
