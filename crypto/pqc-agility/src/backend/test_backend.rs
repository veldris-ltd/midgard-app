//! ⚠️ INSECURE deterministic backend for tests only. ⚠️
//!
//! These KEM and signature implementations are built from SHA-256 alone. They
//! round-trip correctly and detect tampering and wrong keys — enough to exercise
//! the agility shim, the audit chain, and the handshake — but they provide **no**
//! IND-CCA / EUF-CMA security (the "verifying key" is the seed; signatures are
//! forgeable). The `compile_error!` in `backend/mod.rs` prevents building this
//! alongside a real backend.

use crate::algorithm::{KemId, KemSuiteId, SigId};
use crate::error::CryptoError;
use crate::hash::{ct_eq, Digest, Sha256};
use crate::hybrid::combine_hybrid;
use crate::kem::{Ciphertext, Kem, KemPublicKey, KemSecretKey, KemSizes, SharedSecret};
use crate::registry::CryptoProvider;
use crate::rng::Rng;
use crate::secret::SecretBytes;
use crate::sign::{Signature, SignatureScheme, SigningKey, VerifyingKey};

fn h(parts: &[&[u8]]) -> [u8; 32] {
    let mut d = Sha256::new();
    for p in parts {
        d.update(p);
    }
    d.finalize()
}

fn xor32(a: &[u8], b: &[u8]) -> [u8; 32] {
    let mut o = [0u8; 32];
    for i in 0..32 {
        o[i] = a[i] ^ b[i];
    }
    o
}

fn sizes_for(suite: KemSuiteId) -> KemSizes {
    // Realistic ML-KEM lengths so downstream length checks are meaningful; only
    // the first 32 bytes of each artefact are cryptographically meaningful here.
    let (pk, sk, ct) = match suite.pq_component() {
        KemId::MlKem768 => (1184usize, 2400usize, 1088usize),
        KemId::MlKem1024 => (1568, 3168, 1568),
        KemId::X25519 => (32, 32, 32),
    };
    let extra = if suite.is_hybrid() { 32 } else { 0 };
    KemSizes {
        public_key: pk + extra,
        secret_key: sk + extra,
        ciphertext: ct + extra,
    }
}

/// INSECURE deterministic KEM.
pub struct TestKem {
    suite: KemSuiteId,
    sizes: KemSizes,
}

impl TestKem {
    /// Build a test KEM for a suite.
    pub fn new(suite: KemSuiteId) -> Self {
        Self {
            suite,
            sizes: sizes_for(suite),
        }
    }

    fn shared(&self, pk_core: &[u8; 32], e: &[u8; 32]) -> SharedSecret {
        if self.suite.is_hybrid() {
            let pq = h(&[&b"PQC-TEST-SSQ"[..], &pk_core[..], &e[..]]);
            let cl = h(&[&b"PQC-TEST-SSC"[..], &pk_core[..], &e[..]]);
            combine_hybrid(self.suite, &pq, &cl, e, pk_core)
        } else {
            SharedSecret::from_bytes(h(&[&b"PQC-TEST-SS"[..], &pk_core[..], &e[..]]))
        }
    }
}

impl Kem for TestKem {
    fn suite(&self) -> KemSuiteId {
        self.suite
    }

    fn sizes(&self) -> KemSizes {
        self.sizes
    }

    fn keypair(&self, rng: &mut dyn Rng) -> Result<(KemPublicKey, KemSecretKey), CryptoError> {
        let mut sk_core = [0u8; 32];
        rng.fill(&mut sk_core);
        let pk_core = h(&[&b"PQC-TEST-PK"[..], &sk_core[..]]);
        let mut pk = vec![0u8; self.sizes.public_key];
        pk[..32].copy_from_slice(&pk_core);
        let mut sk = vec![0u8; self.sizes.secret_key];
        sk[..32].copy_from_slice(&sk_core);
        Ok((
            KemPublicKey::new(self.suite, pk),
            KemSecretKey::new(self.suite, SecretBytes::new(sk)),
        ))
    }

    fn encapsulate(
        &self,
        pk: &KemPublicKey,
        rng: &mut dyn Rng,
    ) -> Result<(Ciphertext, SharedSecret), CryptoError> {
        if pk.suite() != self.suite {
            return Err(CryptoError::AlgorithmMismatch {
                expected: self.suite.as_str(),
                got: pk.suite().as_str(),
            });
        }
        if pk.as_bytes().len() != self.sizes.public_key {
            return Err(CryptoError::LengthMismatch {
                expected: self.sizes.public_key,
                got: pk.as_bytes().len(),
            });
        }
        let mut pk_core = [0u8; 32];
        pk_core.copy_from_slice(&pk.as_bytes()[..32]);
        let mut e = [0u8; 32];
        rng.fill(&mut e);
        let mask = h(&[&b"PQC-TEST-MASK"[..], &pk_core[..]]);
        let ct_core = xor32(&e, &mask);
        let mut ct = vec![0u8; self.sizes.ciphertext];
        ct[..32].copy_from_slice(&ct_core);
        let ss = self.shared(&pk_core, &e);
        Ok((Ciphertext::new(self.suite, ct), ss))
    }

    fn decapsulate(&self, sk: &KemSecretKey, ct: &Ciphertext) -> Result<SharedSecret, CryptoError> {
        if sk.suite() != self.suite {
            return Err(CryptoError::AlgorithmMismatch {
                expected: self.suite.as_str(),
                got: sk.suite().as_str(),
            });
        }
        if ct.suite() != self.suite {
            return Err(CryptoError::AlgorithmMismatch {
                expected: self.suite.as_str(),
                got: ct.suite().as_str(),
            });
        }
        if ct.as_bytes().len() != self.sizes.ciphertext {
            return Err(CryptoError::LengthMismatch {
                expected: self.sizes.ciphertext,
                got: ct.as_bytes().len(),
            });
        }
        if sk.expose().len() != self.sizes.secret_key {
            return Err(CryptoError::InvalidKey);
        }
        let mut sk_core = [0u8; 32];
        sk_core.copy_from_slice(&sk.expose()[..32]);
        let pk_core = h(&[&b"PQC-TEST-PK"[..], &sk_core[..]]);
        let mask = h(&[&b"PQC-TEST-MASK"[..], &pk_core[..]]);
        let mut ct_core = [0u8; 32];
        ct_core.copy_from_slice(&ct.as_bytes()[..32]);
        let e = xor32(&ct_core, &mask);
        Ok(self.shared(&pk_core, &e))
    }
}

/// INSECURE deterministic signature scheme. The verifying key IS the seed, so
/// signatures are forgeable — this only detects tampering and wrong-key use.
pub struct TestSig {
    alg: SigId,
}

impl TestSig {
    /// Build a test signature scheme for an algorithm.
    pub fn new(alg: SigId) -> Self {
        Self { alg }
    }
}

impl SignatureScheme for TestSig {
    fn id(&self) -> SigId {
        self.alg
    }

    fn keypair(&self, rng: &mut dyn Rng) -> Result<(VerifyingKey, SigningKey), CryptoError> {
        let mut seed = [0u8; 32];
        rng.fill(&mut seed);
        let vk = VerifyingKey::new(self.alg, seed.to_vec());
        let sk = SigningKey::new(self.alg, SecretBytes::new(seed.to_vec()));
        Ok((vk, sk))
    }

    fn sign(&self, sk: &SigningKey, msg: &[u8]) -> Result<Signature, CryptoError> {
        if sk.alg() != self.alg {
            return Err(CryptoError::AlgorithmMismatch {
                expected: self.alg.as_str(),
                got: sk.alg().as_str(),
            });
        }
        // For the test backend the verifying key equals the signing seed.
        let sig = h(&[&b"PQC-TEST-SIG"[..], sk.expose(), msg]);
        Ok(Signature::new(self.alg, sig.to_vec()))
    }

    fn verify(&self, vk: &VerifyingKey, msg: &[u8], sig: &Signature) -> Result<(), CryptoError> {
        if vk.alg() != self.alg {
            return Err(CryptoError::AlgorithmMismatch {
                expected: self.alg.as_str(),
                got: vk.alg().as_str(),
            });
        }
        if sig.alg() != self.alg {
            return Err(CryptoError::AlgorithmMismatch {
                expected: self.alg.as_str(),
                got: sig.alg().as_str(),
            });
        }
        let expect = h(&[&b"PQC-TEST-SIG"[..], vk.as_bytes(), msg]);
        if ct_eq(&expect, sig.as_bytes()) {
            Ok(())
        } else {
            Err(CryptoError::InvalidSignature)
        }
    }
}

/// A [`CryptoProvider`] populated with every test KEM suite and signature
/// algorithm. INSECURE — tests only.
pub fn provider() -> CryptoProvider {
    let mut p = CryptoProvider::new();
    for suite in [
        KemSuiteId::MlKem768,
        KemSuiteId::MlKem1024,
        KemSuiteId::HybridX25519MlKem768,
        KemSuiteId::HybridX25519MlKem1024,
    ] {
        p.register_kem(Box::new(TestKem::new(suite)));
    }
    for alg in [
        SigId::MlDsa44,
        SigId::MlDsa65,
        SigId::MlDsa87,
        SigId::SlhDsaSha2_128s,
    ] {
        p.register_sig(Box::new(TestSig::new(alg)));
    }
    p
}
