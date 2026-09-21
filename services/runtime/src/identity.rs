//! A service's mesh identity, loaded from the directory `mg-admin mesh issue` wrote:
//! its certificate, the mesh trust anchor, its identity key, and its own durable,
//! PQ-signed audit ledger. Loading fails closed. [NFR-SEC-1] [RC-35] [AC-M-4]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use attest::keystore::KeystoreRoot;
use attest::Keystore;
use audit_writer::{verify_chain, AuditWriter, FileLedger, LedgerId, Sealer};
use kms::{Kms, KmsSealer};
use pq_mtls::{decode_anchor, decode_cert, verify_cert, IdentityCert, TrustAnchor};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{SecretBytes, SigId, SigningKey, VerifyingKey};

/// Everything a service presents and verifies on the mesh.
pub struct ServiceIdentity {
    /// The service's name, as its certificate states it.
    pub service: String,
    /// Its certificate.
    pub cert: IdentityCert,
    /// The key the certificate binds.
    pub signing_key: Arc<SigningKey>,
    /// The mesh trust anchor peers are verified against.
    pub anchor: TrustAnchor,
    dir: PathBuf,
}

impl ServiceIdentity {
    /// Load `service`'s identity from `dir`. Refuses a certificate that names another
    /// service or does not verify against the anchor, and an identity keystore that
    /// does not hold the certified key.
    pub fn load(dir: impl AsRef<Path>, service: &str) -> Result<Self, String> {
        let dir = dir.as_ref().to_path_buf();
        let cert_path = dir.join(format!("{service}.cert"));
        let cert = decode_cert(&read(&cert_path)?)
            .map_err(|e| format!("{} is not a certificate: {e:?}", cert_path.display()))?;
        let anchor_path = dir.join("mesh.anchor");
        let anchor = decode_anchor(&read(&anchor_path)?)
            .map_err(|e| format!("{} is not a trust anchor: {e:?}", anchor_path.display()))?;

        if cert.identity.id.0 != service {
            return Err(format!(
                "{} is issued to '{}', not '{service}'",
                cert_path.display(),
                cert.identity.id.0
            ));
        }
        verify_cert(&cert, &anchor, &fips_provider()).map_err(|e| {
            format!(
                "{} does not verify against {}: {e:?}",
                cert_path.display(),
                anchor_path.display()
            )
        })?;

        let keystore_path = dir.join(format!("{service}.keystore"));
        let (verifying_key, signing_key, _) = unseal(&keystore_path)?;
        if verifying_key != cert.identity.verifying_key {
            return Err(format!(
                "{} does not hold the key {} certifies",
                keystore_path.display(),
                cert_path.display()
            ));
        }

        Ok(Self {
            service: service.to_string(),
            cert,
            signing_key: Arc::new(signing_key),
            anchor,
            dir,
        })
    }

    /// The service's roles, as its certificate states them.
    pub fn roles(&self) -> Vec<&str> {
        self.cert
            .identity
            .roles
            .iter()
            .map(|r| r.0.as_str())
            .collect()
    }

    /// A sealer for the service's own durable state — its journals and stores — under
    /// the custody root of its audit keystore, the root its ledger is sealed under.
    /// [RC-12]
    pub fn data_sealer(&self) -> Result<Box<dyn Sealer>, String> {
        Ok(Box::new(KmsSealer::new(self.data_kms()?)))
    }

    /// The KMS over the same custody root, for stores that seal through a KMS directly
    /// (versioned dataset stores). [RC-11]
    pub fn data_kms(&self) -> Result<Kms<KeystoreRoot>, String> {
        let (keystore, _) = self.audit_keystore()?;
        Ok(Kms::new(fips_provider(), keystore.trust_root()))
    }

    /// Open the service's own durable audit ledger at `path` — sealed under, and signed
    /// with, its audit keystore — and re-verify every recovered entry before returning
    /// it, so nothing is appended to a chain that does not verify.
    pub fn audit_writer(
        &self,
        path: impl AsRef<Path>,
        ledger_id: &str,
    ) -> Result<AuditWriter<FileLedger>, String> {
        let (keystore, keystore_path) = self.audit_keystore()?;
        let sealer = KmsSealer::new(Kms::new(fips_provider(), keystore.trust_root()));
        let (verifying_key, signing_key, alg) = unseal_keystore(keystore, &keystore_path)?;

        let ledger = FileLedger::open(path.as_ref(), Box::new(sealer), LedgerId(ledger_id.into()))
            .map_err(|e| format!("the audit ledger does not open: {e}"))?;
        let writer = AuditWriter::new(
            ledger,
            Box::new(FipsSig::new(alg)),
            signing_key,
            verifying_key.clone(),
            LedgerId(ledger_id.into()),
        );
        let resolver =
            move |a: SigId| -> Option<VerifyingKey> { (a == alg).then(|| verifying_key.clone()) };
        verify_chain(
            writer.entries(),
            &fips_provider(),
            &LedgerId(ledger_id.into()),
            &resolver,
        )
        .map_err(|e| format!("the audit ledger does not re-verify: {e:?}"))?;
        Ok(writer)
    }

    fn audit_keystore(&self) -> Result<(Keystore, PathBuf), String> {
        let path = self.dir.join(format!("{}.audit.keystore", self.service));
        let keystore = Keystore::read(&path)
            .map_err(|e| format!("cannot open the audit keystore {}: {e}", path.display()))?;
        Ok((keystore, path))
    }
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn unseal(path: &Path) -> Result<(VerifyingKey, SigningKey, SigId), String> {
    let keystore = Keystore::read(path)
        .map_err(|e| format!("cannot open the keystore {}: {e}", path.display()))?;
    unseal_keystore(keystore, path)
}

/// A keystore's verifying key, its unsealed signing key, and their algorithm.
fn unseal_keystore(
    keystore: Keystore,
    path: &Path,
) -> Result<(VerifyingKey, SigningKey, SigId), String> {
    let verifying_key = keystore.verifying_key.clone();
    let alg = keystore.alg;
    let root = keystore
        .into_signing_root()
        .map_err(|e| format!("cannot unseal {}: {e}", path.display()))?;
    let signing_key = SigningKey::new(
        alg,
        SecretBytes::new(root.expose_signing_key().expose().to_vec()),
    );
    Ok((verifying_key, signing_key, alg))
}
