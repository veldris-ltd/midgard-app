//! At-rest custody for a software signing key.
//!
//! A keystore file holds an ML-KEM-1024 **root** and the ML-DSA **signing key
//! sealed under it** using the suite's existing KEM-DEM envelope
//! ([`kms::Kms::seal`]). The signing key is therefore never at rest in
//! plaintext: an attacker who obtains the sealed blob alone — from a backup, a
//! build cache, a leaked artefact directory — obtains nothing. `[NFR-PQ-2]`
//!
//! ```text
//! magic          11  b"MGKEYSTORE1"
//! version         1  u8
//! key_id          4+n  the signing key's id
//! sig_alg         1  u8, SigId tag
//! verifying_key   4+n  public, exportable as a trust anchor
//! root_key_id     4+n
//! root_suite      1  u8, KemSuiteId tag
//! root_public     4+n  ML-KEM-1024 public key
//! root_secret     4+n  ML-KEM-1024 secret key  ← the software custody device
//! sealed_suite    1  u8
//! sealed_root_id  4+n
//! sealed_wrapped  4+n  the ML-KEM encapsulation (the wrapped data key)
//! sealed_nonce   12
//! sealed_cipher   4+n  AES-256-GCM(signing key)
//! ```
//!
//! ## Honest limitation
//! The root secret sits in this file in the clear, so the file *is* the custody
//! device and must be protected as one — this is software-tier custody, the
//! equivalent of a software HSM. Its value is that it makes the signing key
//! non-ambient and gives the production path (`KmsSigner` over
//! [`kms::SigningRoot`]) something real to exercise. Hardware custody,
//! key hierarchy, rotation and revocation are RF-SC-8; a TPM/HSM root implements
//! [`kms::TrustRoot`] and slots in without changing anything above.

use std::path::Path;

use kms::{Kms, SealedBlob, TrustRoot, WRAP_SUITE};
use pqc_agility::{
    Ciphertext, Kem, KemPublicKey, KemSecretKey, KemSuiteId, Rng, SecretBytes, SharedSecret, SigId,
    SigningKey, VerifyingKey, GCM_NONCE_LEN,
};

use crate::error::SignError;

/// Keystore file magic.
pub const MAGIC: &[u8; 11] = b"MGKEYSTORE1";
/// Keystore format version.
pub const VERSION: u8 = 1;
/// Plaintext dev-key file magic.
pub const KEY_MAGIC: &[u8; 6] = b"MGKEY1";

/// AAD binding the sealed blob to its purpose, so a blob sealed for some other
/// use cannot be presented here as a signing key.
const SEAL_AAD: &[u8] = b"midgard-keystore/signing-key/v1";

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
    fn lp(&mut self, what: &str) -> Result<&'a [u8], String> {
        let n = u32::from_le_bytes(self.take(4, what)?.try_into().expect("4 bytes")) as usize;
        self.take(n, what)
    }
}

/// A keystore, in memory.
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
    root_secret: Vec<u8>,
    sealed: SealedBlob,
}

impl Keystore {
    /// Generate a fresh root and signing key, sealing the latter under the former.
    pub fn generate(key_id: &str, alg: SigId, rng: &mut dyn Rng) -> Result<Self, SignError> {
        let provider = pqc_agility::backend::fips::provider();

        // 1. The custody root (ML-KEM-1024).
        let kem = provider
            .kem_for(WRAP_SUITE)
            .map_err(|e| SignError::Custodian(e.to_string()))?;
        let (root_public, root_secret) = kem
            .keypair(rng)
            .map_err(|e| SignError::Custodian(e.to_string()))?;
        let root_key_id = format!("{key_id}-root");

        // 2. The signing key.
        let (verifying_key, signing_key) = provider
            .sig_for(alg)
            .map_err(|e| SignError::Custodian(e.to_string()))?
            .keypair(rng)
            .map_err(|e| SignError::Custodian(e.to_string()))?;

        // 3. Seal the signing key under the root — it is never at rest in the clear.
        let root = KeystoreRoot {
            key_id: root_key_id.clone(),
            public_key: KemPublicKey::new(WRAP_SUITE, root_public.as_bytes().to_vec()),
            secret_key: KemSecretKey::new(
                WRAP_SUITE,
                SecretBytes::new(root_secret.expose().to_vec()),
            ),
        };
        let sealed = Kms::new(pqc_agility::backend::fips::provider(), root)
            .seal(SEAL_AAD, signing_key.expose(), rng)
            .map_err(|e| SignError::Custodian(e.to_string()))?;

        Ok(Self {
            key_id: key_id.to_string(),
            alg,
            verifying_key,
            root_key_id,
            root_suite: WRAP_SUITE,
            root_public: root_public.as_bytes().to_vec(),
            root_secret: root_secret.expose().to_vec(),
            sealed,
        })
    }

    /// Serialise for storage.
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        o.extend_from_slice(MAGIC);
        o.push(VERSION);
        put(&mut o, self.key_id.as_bytes());
        o.push(self.alg.as_u8());
        put(&mut o, self.verifying_key.as_bytes());
        put(&mut o, self.root_key_id.as_bytes());
        o.push(self.root_suite.as_u8());
        put(&mut o, &self.root_public);
        put(&mut o, &self.root_secret);
        o.push(self.sealed.suite.as_u8());
        put(&mut o, self.sealed.root_key_id.as_bytes());
        put(&mut o, self.sealed.wrapped_key.as_bytes());
        o.extend_from_slice(&self.sealed.nonce);
        put(&mut o, &self.sealed.ciphertext);
        o
    }

    /// Parse a keystore. Every malformed field is a rejection.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut c = Cur { b: bytes, at: 0 };
        if c.take(MAGIC.len(), "magic")? != MAGIC {
            return Err("bad magic — not an MGKEYSTORE1 file".into());
        }
        let v = c.u8("version")?;
        if v != VERSION {
            return Err(format!("unsupported keystore version {v}"));
        }
        let key_id = String::from_utf8(c.lp("key id")?.to_vec()).map_err(|_| "key id not UTF-8")?;
        let alg_tag = c.u8("algorithm")?;
        let alg = SigId::from_u8(alg_tag).ok_or(format!("unknown algorithm tag {alg_tag}"))?;
        let verifying_key = VerifyingKey::new(alg, c.lp("verifying key")?.to_vec());
        let root_key_id = String::from_utf8(c.lp("root key id")?.to_vec())
            .map_err(|_| "root key id not UTF-8")?;
        let rs_tag = c.u8("root suite")?;
        let root_suite =
            KemSuiteId::from_u8(rs_tag).ok_or(format!("unknown KEM suite {rs_tag}"))?;
        let root_public = c.lp("root public key")?.to_vec();
        let root_secret = c.lp("root secret key")?.to_vec();

        let ss_tag = c.u8("sealed suite")?;
        let sealed_suite =
            KemSuiteId::from_u8(ss_tag).ok_or(format!("unknown sealed suite {ss_tag}"))?;
        let sealed_root_id = String::from_utf8(c.lp("sealed root id")?.to_vec())
            .map_err(|_| "sealed root id not UTF-8")?;
        let wrapped = c.lp("wrapped key")?.to_vec();
        let nonce: [u8; GCM_NONCE_LEN] = c
            .take(GCM_NONCE_LEN, "nonce")?
            .try_into()
            .map_err(|_| "bad nonce length")?;
        let ciphertext = c.lp("sealed signing key")?.to_vec();

        if c.at != bytes.len() {
            return Err(format!("{} trailing byte(s)", bytes.len() - c.at));
        }

        Ok(Self {
            key_id,
            alg,
            verifying_key,
            root_key_id,
            root_suite,
            root_public,
            root_secret,
            sealed: SealedBlob {
                root_key_id: sealed_root_id,
                suite: sealed_suite,
                wrapped_key: Ciphertext::new(sealed_suite, wrapped),
                nonce,
                ciphertext,
            },
        })
    }

    /// Unseal the signing key and build a live custodian.
    ///
    /// A tampered sealed blob, a mismatched root, or a disabled algorithm all
    /// fail here rather than producing a custodian that signs wrongly.
    pub fn into_signing_root(self) -> Result<kms::SoftwareSigningRoot, SignError> {
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

    /// The custody root this keystore seals its signing key under, as a live
    /// [`kms::TrustRoot`].
    ///
    /// A service's keystore is then its one custody device: the same root that keeps
    /// its signing key sealed can seal its data at rest — object store, audit ledger,
    /// identity directory — through `kms::KmsSealer`. It inherits the limitation in
    /// the module docs: software-tier custody, protected only as well as the file is.
    pub fn trust_root(&self) -> KeystoreRoot {
        KeystoreRoot {
            key_id: self.root_key_id.clone(),
            public_key: KemPublicKey::new(self.root_suite, self.root_public.clone()),
            secret_key: KemSecretKey::new(
                self.root_suite,
                SecretBytes::new(self.root_secret.clone()),
            ),
        }
    }

    /// Read a keystore from disk.
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

    /// Write a keystore to disk.
    pub fn write(&self, path: &Path) -> Result<(), SignError> {
        write_restricted(path, &self.encode())
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

/// Write a file containing key material, owner-readable only where the platform
/// supports it. On Windows the ACL is inherited; the keystore's real protection
/// is that the signing key inside it is sealed.
fn write_restricted(path: &Path, bytes: &[u8]) -> Result<(), SignError> {
    std::fs::write(path, bytes).map_err(|e| SignError::Write {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    // On Windows this is skipped and the file inherits its folder's ACL: the app's working copy, %LOCALAPPDATA%\Midgard, is private to the user (the W2 shell checks it).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
            SignError::Write {
                path: path.to_path_buf(),
                reason: format!("cannot restrict permissions: {e}"),
            }
        })?;
    }
    Ok(())
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
        o.push(VERSION);
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
        if v != VERSION {
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
    use super::*;
    use kms::SigningRoot;
    // For `verify` on the concrete `FipsSig`; production paths go through
    // `&dyn SignatureScheme` and need no import.
    use pqc_agility::SignatureScheme;

    fn ks() -> Keystore {
        let mut rng = pqc_agility::OsRng;
        Keystore::generate("release-key-1", kms::RELEASE_SIG_ALG, &mut rng).expect("generate")
    }

    #[test]
    fn round_trips_and_unseals_a_working_custodian() {
        let k = ks();
        let vk = k.verifying_key.clone();
        let decoded = Keystore::decode(&k.encode()).expect("decode");
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
        let mut rng = pqc_agility::OsRng;
        let k = Keystore::generate("k", kms::RELEASE_SIG_ALG, &mut rng).expect("generate");
        let secret = {
            let root = Keystore::decode(&k.encode())
                .unwrap()
                .into_signing_root()
                .unwrap();
            root.expose_signing_key().expose().to_vec()
        };
        let file = k.encode();
        assert!(
            !file.windows(secret.len()).any(|w| w == secret.as_slice()),
            "the signing key must never appear in the keystore in plaintext"
        );
    }

    #[test]
    fn a_tampered_sealed_blob_fails_to_unseal() {
        let k = ks();
        let mut bytes = k.encode();
        // Corrupt the last byte, which lies inside the AES-GCM ciphertext.
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        let decoded = Keystore::decode(&bytes).expect("still parses");
        assert!(
            decoded.into_signing_root().is_err(),
            "AEAD must reject a tampered sealed signing key"
        );
    }

    #[test]
    fn rejects_bad_magic_version_and_trailing_bytes() {
        let k = ks();
        let good = k.encode();

        let mut bad = good.clone();
        bad[0] = b'X';
        assert!(Keystore::decode(&bad).is_err());

        let mut bad = good.clone();
        bad[MAGIC.len()] = 9;
        assert!(Keystore::decode(&bad).is_err());

        let mut bad = good.clone();
        bad.push(0);
        assert!(Keystore::decode(&bad).is_err());

        assert!(Keystore::decode(&good[..good.len() - 1]).is_err());
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

        let reloaded = Keystore::decode(&k.encode()).expect("decode");
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
}
