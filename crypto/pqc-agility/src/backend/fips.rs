//! Production backend: pure-Rust, ACVP-validated **FIPS 203 / 204 / 205**
//! (ML-KEM, ML-DSA, SLH-DSA) plus **X25519** for the hybrid transit suites.
//!
//! Chosen over liboqs in Phase 1 because it is dependency-light pure Rust: no C
//! toolchain, trivially vendored for offline builds, and deterministic enough for
//! bit-reproducible releases (AC-M-6). The upstream crates carry the NIST ACVP
//! known-answer-test suites; this module's own tests exercise the *integration*
//! (round-trips, hybrid mixing, tamper/wrong-key rejection, determinism).
//!
//! ## Artefact layout (stable, tagged by [`KemSuiteId`])
//! - Pure ML-KEM suite: `pk = ek`, `sk = dk`, `ct = ct`.
//! - Hybrid suite: the 32-byte X25519 half is appended to each artefact —
//!   `pk = ek ‖ x25519_pub`, `sk = dk ‖ x25519_static`, `ct = ct ‖ x25519_eph_pub`
//!   (X-Wing's public-key and ciphertext order). The shared secret is X-Wing's
//!   [`crate::hybrid::combine_hybrid`] over both secrets and the X25519 messages, and
//!   an all-zero X25519 result is refused. [NFR-PQ-1] [NFR-PQ-4] [MG-SEC-09]
//!
//! ## Secret hygiene [MG-SEC-08]
//! `x25519-dalek`, `aes`, `aes-gcm`, `ghash` and `polyval` are built with their
//! `zeroize` features, so their key schedules and DH scalars are wiped on drop. Every
//! intermediate secret this module holds — ML-KEM shared secrets, serialised key
//! bytes, the X25519 DH output — sits in a [`Zeroizing`] wrapper, and a hybrid secret
//! key is allocated at its final size before it is filled, so it is never reallocated
//! (which would leave a copy in freed memory and defeat the `mlock`).

use fips203::{ml_kem_1024, ml_kem_768};
use rand_core::{CryptoRng, RngCore};
use x25519_dalek::{EphemeralSecret, PublicKey as XPublicKey, StaticSecret as XStaticSecret};
use zeroize::Zeroizing;

// Trait methods only — imported anonymously so the three crates' identically
// named traits (KeyGen, SerDes, Signer, Verifier) don't collide and never warn
// "unused" when a concrete type also exposes the method inherently.
use fips203::traits::{Decaps as _, Encaps as _, KeyGen as _, SerDes as _};
use fips204::traits::{KeyGen as _, SerDes as _, Signer as _, Verifier as _};
use fips205::traits::{KeyGen as _, SerDes as _, Signer as _, Verifier as _};

use crate::algorithm::{KemId, KemSuiteId, SigId};
use crate::error::CryptoError;
use crate::hybrid::{combine_hybrid, reject_noncontributory_x25519};
use crate::kem::{Ciphertext, Kem, KemPublicKey, KemSecretKey, KemSizes, SharedSecret};
use crate::registry::CryptoProvider;
use crate::rng::Rng;
use crate::secret::SecretBytes;
use crate::sign::{Signature, SignatureScheme, SigningKey, VerifyingKey};

/// Length of the X25519 public key / secret scalar / DH output.
const X25519_LEN: usize = 32;

// ---------------------------------------------------------------------------
// RNG bridge: adapt the crate's object-safe `Rng` to `rand_core` (0.6), which is
// what the FIPS and dalek crates require. Marked `CryptoRng` because callers
// inject a cryptographic source (`OsRng` in production).
// ---------------------------------------------------------------------------

struct RngAdapter<'a> {
    inner: &'a mut dyn Rng,
}

impl RngCore for RngAdapter<'_> {
    fn next_u32(&mut self) -> u32 {
        let mut b = [0u8; 4];
        self.inner.fill(&mut b);
        u32::from_le_bytes(b)
    }
    fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        self.inner.fill(&mut b);
        u64::from_le_bytes(b)
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.inner.fill(dest);
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.inner.fill(dest);
        Ok(())
    }
}

impl CryptoRng for RngAdapter<'_> {}

// ---------------------------------------------------------------------------
// ML-KEM (FIPS 203) per-parameter operations, in terms of raw bytes so the
// distinct concrete types per module don't leak into the `Kem` impl. Each helper
// scopes its own trait imports to avoid the same-named traits across fips crates
// colliding.
// ---------------------------------------------------------------------------

macro_rules! mlkem_ops {
    ($module:ident, $keypair:ident, $encaps:ident, $decaps:ident) => {
        /// `(ek, dk)`, with `dk` written into a buffer of `dk_capacity` bytes so a
        /// hybrid suite can append its X25519 half without reallocating.
        fn $keypair(
            a: &mut RngAdapter,
            dk_capacity: usize,
        ) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
            let (ek, dk) = fips203::$module::KG::try_keygen_with_rng(a)
                .map_err(|_| CryptoError::RngFailure)?;
            let dk_bytes = Zeroizing::new(dk.into_bytes());
            let mut sk = Vec::with_capacity(dk_capacity.max(dk_bytes.len()));
            sk.extend_from_slice(dk_bytes.as_slice());
            Ok((ek.into_bytes().to_vec(), sk))
        }

        fn $encaps(
            ek_bytes: &[u8],
            a: &mut RngAdapter,
        ) -> Result<(Vec<u8>, Zeroizing<[u8; 32]>), CryptoError> {
            let ek_arr: [u8; fips203::$module::EK_LEN] =
                ek_bytes.try_into().map_err(|_| CryptoError::InvalidKey)?;
            let ek = fips203::$module::EncapsKey::try_from_bytes(ek_arr)
                .map_err(|_| CryptoError::InvalidKey)?;
            let (ssk, ct) = ek
                .try_encaps_with_rng(a)
                .map_err(|_| CryptoError::RngFailure)?;
            Ok((ct.into_bytes().to_vec(), Zeroizing::new(ssk.into_bytes())))
        }

        fn $decaps(dk_bytes: &[u8], ct_bytes: &[u8]) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
            let dk_arr: Zeroizing<[u8; fips203::$module::DK_LEN]> =
                Zeroizing::new(dk_bytes.try_into().map_err(|_| CryptoError::InvalidKey)?);
            let ct_arr: [u8; fips203::$module::CT_LEN] = ct_bytes
                .try_into()
                .map_err(|_| CryptoError::InvalidCiphertext)?;
            let dk = fips203::$module::DecapsKey::try_from_bytes(*dk_arr)
                .map_err(|_| CryptoError::InvalidKey)?;
            let ct = fips203::$module::CipherText::try_from_bytes(ct_arr)
                .map_err(|_| CryptoError::InvalidCiphertext)?;
            let ssk = dk
                .try_decaps(&ct)
                .map_err(|_| CryptoError::DecapsulationFailed)?;
            Ok(Zeroizing::new(ssk.into_bytes()))
        }
    };
}

mlkem_ops!(
    ml_kem_768,
    mlkem768_keypair,
    mlkem768_encaps,
    mlkem768_decaps
);
mlkem_ops!(
    ml_kem_1024,
    mlkem1024_keypair,
    mlkem1024_encaps,
    mlkem1024_decaps
);

/// Declared post-quantum artefact sizes `(ek, dk, ct)` for a parameter set.
fn pq_sizes(param: KemId) -> (usize, usize, usize) {
    match param {
        KemId::MlKem768 => (ml_kem_768::EK_LEN, ml_kem_768::DK_LEN, ml_kem_768::CT_LEN),
        KemId::MlKem1024 => (
            ml_kem_1024::EK_LEN,
            ml_kem_1024::DK_LEN,
            ml_kem_1024::CT_LEN,
        ),
        // X25519 is only ever a hybrid *component*, never a standalone KEM param.
        KemId::X25519 => (0, 0, 0),
    }
}

fn mlkem_keypair(
    param: KemId,
    a: &mut RngAdapter,
    dk_capacity: usize,
) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
    match param {
        KemId::MlKem768 => mlkem768_keypair(a, dk_capacity),
        KemId::MlKem1024 => mlkem1024_keypair(a, dk_capacity),
        KemId::X25519 => Err(CryptoError::UnknownAlgorithm("X25519")),
    }
}

fn mlkem_encaps(
    param: KemId,
    ek_bytes: &[u8],
    a: &mut RngAdapter,
) -> Result<(Vec<u8>, Zeroizing<[u8; 32]>), CryptoError> {
    match param {
        KemId::MlKem768 => mlkem768_encaps(ek_bytes, a),
        KemId::MlKem1024 => mlkem1024_encaps(ek_bytes, a),
        KemId::X25519 => Err(CryptoError::UnknownAlgorithm("X25519")),
    }
}

fn mlkem_decaps(
    param: KemId,
    dk_bytes: &[u8],
    ct_bytes: &[u8],
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    match param {
        KemId::MlKem768 => mlkem768_decaps(dk_bytes, ct_bytes),
        KemId::MlKem1024 => mlkem1024_decaps(dk_bytes, ct_bytes),
        KemId::X25519 => Err(CryptoError::UnknownAlgorithm("X25519")),
    }
}

// ---------------------------------------------------------------------------
// The FIPS KEM: pure ML-KEM or ML-KEM ⊕ X25519 hybrid, keyed by suite.
// ---------------------------------------------------------------------------

/// A production KEM for one [`KemSuiteId`].
pub struct FipsKem {
    suite: KemSuiteId,
    sizes: KemSizes,
}

impl FipsKem {
    /// Build the KEM for a suite, precomputing declared artefact sizes.
    pub fn new(suite: KemSuiteId) -> Self {
        let (ek, dk, ct) = pq_sizes(suite.pq_component());
        let extra = if suite.is_hybrid() { X25519_LEN } else { 0 };
        Self {
            suite,
            sizes: KemSizes {
                public_key: ek + extra,
                secret_key: dk + extra,
                ciphertext: ct + extra,
            },
        }
    }
}

fn to_x25519_arr(b: &[u8]) -> Result<[u8; X25519_LEN], CryptoError> {
    b.try_into().map_err(|_| CryptoError::InvalidKey)
}

impl Kem for FipsKem {
    fn suite(&self) -> KemSuiteId {
        self.suite
    }

    fn sizes(&self) -> KemSizes {
        self.sizes
    }

    fn keypair(&self, rng: &mut dyn Rng) -> Result<(KemPublicKey, KemSecretKey), CryptoError> {
        let mut a = RngAdapter { inner: rng };
        // Sized up front: appending the X25519 half must not reallocate. [MG-SEC-08]
        let (mut pk, mut sk) =
            mlkem_keypair(self.suite.pq_component(), &mut a, self.sizes.secret_key)?;
        if self.suite.is_hybrid() {
            let x_sk = XStaticSecret::random_from_rng(&mut a);
            let x_pk = XPublicKey::from(&x_sk);
            pk.extend_from_slice(x_pk.as_bytes());
            sk.extend_from_slice(Zeroizing::new(x_sk.to_bytes()).as_slice());
        }
        debug_assert_eq!(sk.capacity(), self.sizes.secret_key);
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
        let mut a = RngAdapter { inner: rng };
        let (pq_ek_len, _, _) = pq_sizes(self.suite.pq_component());
        let (pq_ek, x_pk_bytes) = pk.as_bytes().split_at(pq_ek_len);
        let (mut ct, ss_pq) = mlkem_encaps(self.suite.pq_component(), pq_ek, &mut a)?;

        if self.suite.is_hybrid() {
            let pk_x = to_x25519_arr(x_pk_bytes)?;
            let eph = EphemeralSecret::random_from_rng(&mut a);
            let eph_pub = XPublicKey::from(&eph);
            let ss_cl = eph.diffie_hellman(&XPublicKey::from(pk_x));
            reject_noncontributory_x25519(ss_cl.as_bytes())?;
            ct.extend_from_slice(eph_pub.as_bytes());
            let ss = combine_hybrid(
                self.suite,
                &ss_pq,
                ss_cl.as_bytes(),
                eph_pub.as_bytes(),
                &pk_x,
            );
            Ok((Ciphertext::new(self.suite, ct), ss))
        } else {
            Ok((
                Ciphertext::new(self.suite, ct),
                SharedSecret::from_bytes(*ss_pq),
            ))
        }
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
        if sk.expose().len() != self.sizes.secret_key {
            return Err(CryptoError::InvalidKey);
        }
        if ct.as_bytes().len() != self.sizes.ciphertext {
            return Err(CryptoError::LengthMismatch {
                expected: self.sizes.ciphertext,
                got: ct.as_bytes().len(),
            });
        }
        let (_, pq_dk_len, pq_ct_len) = pq_sizes(self.suite.pq_component());
        let (pq_dk, x_sk_bytes) = sk.expose().split_at(pq_dk_len);
        let (pq_ct, x_eph_bytes) = ct.as_bytes().split_at(pq_ct_len);
        let ss_pq = mlkem_decaps(self.suite.pq_component(), pq_dk, pq_ct)?;

        if self.suite.is_hybrid() {
            let x_sk = XStaticSecret::from(*Zeroizing::new(to_x25519_arr(x_sk_bytes)?));
            // X-Wing binds the recipient's own X25519 public key, recomputed here.
            let pk_x = XPublicKey::from(&x_sk);
            let ct_x = to_x25519_arr(x_eph_bytes)?;
            let ss_cl = x_sk.diffie_hellman(&XPublicKey::from(ct_x));
            reject_noncontributory_x25519(ss_cl.as_bytes())?;
            Ok(combine_hybrid(
                self.suite,
                &ss_pq,
                ss_cl.as_bytes(),
                &ct_x,
                pk_x.as_bytes(),
            ))
        } else {
            Ok(SharedSecret::from_bytes(*ss_pq))
        }
    }
}

// ---------------------------------------------------------------------------
// Signatures: ML-DSA (FIPS 204) and SLH-DSA (FIPS 205). Empty context string.
// ---------------------------------------------------------------------------

macro_rules! mldsa_ops {
    ($module:ident, $keypair:ident, $sign:ident, $verify:ident) => {
        fn $keypair(a: &mut RngAdapter) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
            let (pk, sk) = fips204::$module::KG::try_keygen_with_rng(a)
                .map_err(|_| CryptoError::RngFailure)?;
            Ok((
                pk.into_bytes().to_vec(),
                Zeroizing::new(sk.into_bytes()).to_vec(),
            ))
        }

        fn $sign(sk_bytes: &[u8], msg: &[u8], a: &mut RngAdapter) -> Result<Vec<u8>, CryptoError> {
            let sk_arr: Zeroizing<[u8; fips204::$module::SK_LEN]> =
                Zeroizing::new(sk_bytes.try_into().map_err(|_| CryptoError::InvalidKey)?);
            let sk = fips204::$module::PrivateKey::try_from_bytes(*sk_arr)
                .map_err(|_| CryptoError::InvalidKey)?;
            let sig = sk
                .try_sign_with_rng(a, msg, &[])
                .map_err(|_| CryptoError::RngFailure)?;
            Ok(sig.to_vec())
        }

        fn $verify(vk_bytes: &[u8], msg: &[u8], sig_bytes: &[u8]) -> Result<(), CryptoError> {
            let vk_arr: [u8; fips204::$module::PK_LEN] =
                vk_bytes.try_into().map_err(|_| CryptoError::InvalidKey)?;
            let vk = fips204::$module::PublicKey::try_from_bytes(vk_arr)
                .map_err(|_| CryptoError::InvalidKey)?;
            let sig_arr: [u8; fips204::$module::SIG_LEN] = sig_bytes
                .try_into()
                .map_err(|_| CryptoError::InvalidSignature)?;
            if vk.verify(msg, &sig_arr, &[]) {
                Ok(())
            } else {
                Err(CryptoError::InvalidSignature)
            }
        }
    };
}

mldsa_ops!(ml_dsa_44, mldsa44_keypair, mldsa44_sign, mldsa44_verify);
mldsa_ops!(ml_dsa_65, mldsa65_keypair, mldsa65_sign, mldsa65_verify);
mldsa_ops!(ml_dsa_87, mldsa87_keypair, mldsa87_sign, mldsa87_verify);

// SLH-DSA has a `hedged` flag on signing and only the one parameter set we offer.
fn slh128s_keypair(a: &mut RngAdapter) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
    use fips205::slh_dsa_sha2_128s::KG;
    let (pk, sk) = KG::try_keygen_with_rng(a).map_err(|_| CryptoError::RngFailure)?;
    Ok((
        pk.into_bytes().to_vec(),
        Zeroizing::new(sk.into_bytes()).to_vec(),
    ))
}

fn slh128s_sign(sk_bytes: &[u8], msg: &[u8], a: &mut RngAdapter) -> Result<Vec<u8>, CryptoError> {
    use fips205::slh_dsa_sha2_128s::{PrivateKey, SK_LEN};
    let sk_arr: Zeroizing<[u8; SK_LEN]> =
        Zeroizing::new(sk_bytes.try_into().map_err(|_| CryptoError::InvalidKey)?);
    // fips205's SerDes takes the byte array by reference (unlike fips203/204).
    let sk = PrivateKey::try_from_bytes(&sk_arr).map_err(|_| CryptoError::InvalidKey)?;
    // hedged = true: mix fresh randomness into signing (the recommended mode).
    let sig = sk
        .try_sign_with_rng(a, msg, &[], true)
        .map_err(|_| CryptoError::RngFailure)?;
    Ok(sig.to_vec())
}

fn slh128s_verify(vk_bytes: &[u8], msg: &[u8], sig_bytes: &[u8]) -> Result<(), CryptoError> {
    use fips205::slh_dsa_sha2_128s::{PublicKey, PK_LEN, SIG_LEN};
    let vk_arr: [u8; PK_LEN] = vk_bytes.try_into().map_err(|_| CryptoError::InvalidKey)?;
    let vk = PublicKey::try_from_bytes(&vk_arr).map_err(|_| CryptoError::InvalidKey)?;
    let sig_arr: [u8; SIG_LEN] = sig_bytes
        .try_into()
        .map_err(|_| CryptoError::InvalidSignature)?;
    if vk.verify(msg, &sig_arr, &[]) {
        Ok(())
    } else {
        Err(CryptoError::InvalidSignature)
    }
}

/// A production signature scheme for one [`SigId`].
pub struct FipsSig {
    alg: SigId,
}

impl FipsSig {
    /// Build the signature scheme for an algorithm.
    pub fn new(alg: SigId) -> Self {
        Self { alg }
    }
}

impl SignatureScheme for FipsSig {
    fn id(&self) -> SigId {
        self.alg
    }

    fn keypair(&self, rng: &mut dyn Rng) -> Result<(VerifyingKey, SigningKey), CryptoError> {
        let mut a = RngAdapter { inner: rng };
        let (vk, sk) = match self.alg {
            SigId::MlDsa44 => mldsa44_keypair(&mut a)?,
            SigId::MlDsa65 => mldsa65_keypair(&mut a)?,
            SigId::MlDsa87 => mldsa87_keypair(&mut a)?,
            SigId::SlhDsaSha2_128s => slh128s_keypair(&mut a)?,
        };
        Ok((
            VerifyingKey::new(self.alg, vk),
            SigningKey::new(self.alg, SecretBytes::new(sk)),
        ))
    }

    fn sign(&self, sk: &SigningKey, msg: &[u8]) -> Result<Signature, CryptoError> {
        if sk.alg() != self.alg {
            return Err(CryptoError::AlgorithmMismatch {
                expected: self.alg.as_str(),
                got: sk.alg().as_str(),
            });
        }
        // ML-DSA / SLH-DSA signing draws fresh randomness; the trait doesn't
        // thread an RNG, so we pull from the production OS CSPRNG. [NFR-PRV-1]
        let mut os = crate::rng::OsRng;
        let mut a = RngAdapter { inner: &mut os };
        let bytes = match self.alg {
            SigId::MlDsa44 => mldsa44_sign(sk.expose(), msg, &mut a)?,
            SigId::MlDsa65 => mldsa65_sign(sk.expose(), msg, &mut a)?,
            SigId::MlDsa87 => mldsa87_sign(sk.expose(), msg, &mut a)?,
            SigId::SlhDsaSha2_128s => slh128s_sign(sk.expose(), msg, &mut a)?,
        };
        Ok(Signature::new(self.alg, bytes))
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
        match self.alg {
            SigId::MlDsa44 => mldsa44_verify(vk.as_bytes(), msg, sig.as_bytes()),
            SigId::MlDsa65 => mldsa65_verify(vk.as_bytes(), msg, sig.as_bytes()),
            SigId::MlDsa87 => mldsa87_verify(vk.as_bytes(), msg, sig.as_bytes()),
            SigId::SlhDsaSha2_128s => slh128s_verify(vk.as_bytes(), msg, sig.as_bytes()),
        }
    }
}

/// A [`CryptoProvider`] populated with the production FIPS suites and signatures.
pub fn provider() -> CryptoProvider {
    let mut p = CryptoProvider::new();
    for suite in [
        KemSuiteId::MlKem768,
        KemSuiteId::MlKem1024,
        KemSuiteId::HybridX25519MlKem768,
        KemSuiteId::HybridX25519MlKem1024,
    ] {
        p.register_kem(Box::new(FipsKem::new(suite)));
    }
    for alg in [
        SigId::MlDsa44,
        SigId::MlDsa65,
        SigId::MlDsa87,
        SigId::SlhDsaSha2_128s,
    ] {
        p.register_sig(Box::new(FipsSig::new(alg)));
    }
    p
}
