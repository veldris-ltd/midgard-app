#![no_std]
#![deny(clippy::pedantic, warnings, missing_docs, unsafe_code)]
// Most of the 'allow' category...
#![deny(absolute_paths_not_starting_with_crate, dead_code)]
#![deny(elided_lifetimes_in_paths, explicit_outlives_requirements, keyword_idents)]
#![deny(let_underscore_drop, macro_use_extern_crate, meta_variable_misuse, missing_abi)]
#![deny(non_ascii_idents, rust_2021_incompatible_closure_captures)]
#![deny(rust_2021_incompatible_or_patterns, rust_2021_prefixes_incompatible_syntax)]
#![deny(rust_2021_prelude_collisions, single_use_lifetimes, trivial_casts)]
#![deny(trivial_numeric_casts, unreachable_pub, unsafe_op_in_unsafe_fn, unstable_features)]
#![deny(unused_extern_crates, unused_import_braces, unused_lifetimes, unused_macro_rules)]
#![deny(unused_qualifications, unused_results, variant_size_differences)]
//
#![doc = include_str!("../README.md")]

// Implements FIPS 205 Stateless Hash-Based Digital Signature Standard.
// See <https://csrc.nist.gov/pubs/fips/205/final>
//
// Algorithm 1 gen_len2 (n, lgw)                                           --> precomputed
// Algorithm 2 toInt(X, n)                                                 --> helpers.rs
// Algorithm 3 toByte(x, n)                                                --> helpers.rs
// Algorithm 4 base_2b(X, b, out_len)                                      --> helpers.rs
// Algorithm 5 chain(X, i, s, PK.seed, ADRS)                               --> wots.rs
// Algorithm 6 wots_PKgen(SK.seed, PK.seed, ADRS)                          --> wots.rs
// Algorithm 7 wots_sign(M, SK.seed, PK.seed, ADRS)                        --> wots.rs
// Algorithm 8 wots_PKFromSig(sig, M, PK.seed, ADRS)                       --> wots.rs
// Algorithm 9 xmss_node(SK.seed, i, z, PK.seed, ADRS)                     --> xmss.rs
// Algorithm 10 xmss_sign(M, SK.seed, idx, PK.seed, ADRS)                  --> xmss.rs
// Algorithm 11 xmss_PKFromSig(idx, SIGXMSS, M, PK.seed, ADRS)             --> xmss.rs
// Algorithm 12 ht_sign(M, SK.seed, PK.seed, idxtree, idxleaf)             --> hypertree.rs
// Algorithm 13 ht_verify(M, SIGHT, PK.seed, idxtree, idxleaf, PK.root)    --> hypertree.rs
// Algorithm 14 fors_SKgen(SK.seed, PK.seed, ADRS, idx)                    --> fors.rs
// Algorithm 15 fors_node(SK.seed, i, z, PK.seed, ADRS)                    --> fors.rs
// Algorithm 16 fors_sign(md, SK.seed, PK.seed, ADRS)                      --> fors.rs
// Algorithm 17 fors_pkFromSig(SIGFORS, md, PK.seed, ADRS)                 --> fors.rs
// Algorithm 18 slh_keygen_internal(SK.seed, SK.prf, PK.seed)              --> slh.rs
// Algorithm 19 slh_sign_internal(M, SK, addrnd)                           --> slh.rs
// Algorithm 20 slh_verify_internal(M, SIG, PK)                            --> slh.rs
// Algorithm 21 slh_keygen()                                               --> slh.rs
// Algorithm 22 slh_sign(M, ctx, SK)                                       --> slh.rs
// Algorithm 23 hash_slh_sign(M, ctx, PH, SK)                              --> slh.rs
// Algorithm 24 slh_verify(M, SIG, ctx, PK)                                --> slh.rs
// Algorithm 25 hash_slh_verify(M, SIG, ctx, PH, PK)                       --> slh.rs
// Fairly elaborate hashing is found in hashers.rs
// Signature serialize/deserialize and Adrs support can be found in helpers.rs
// types are in types.rs, traits are in traits.rs, and lib.rs provides wrappers into slh.rs


// TODO: Roadmap
// 1. Additional (external) top-level test vectors, particularly for hash variants (!!)
// 2. Implement fuzz harness, embedded target, revise WASM test
// 3. Experiment with struct alignment for performance uplift? (and fixed size 'slices')


/// All functionality is covered by traits, such that consumers can utilize trait objects as desired.
pub mod traits;
pub use types::Ph;

mod fors;
mod hashers;
mod helpers;
mod hypertree;
mod slh;
mod types;
mod wots;
mod xmss;


// Per eqns 5.1-4 on page 17, LGW=4, W=16 and LEN2=3 are constant across all security parameter sets.
const LGW: u32 = 4;
const W: u32 = 16;
const LEN2: u32 = 3;


// This common functionality is injected into each parameter set module
macro_rules! functionality {
    () => {
        use crate::hashers::hash_message;
        use crate::traits::{KeyGen, SerDes, Signer, Verifier};
        use crate::types::{Ph, SlhDsaSig, SlhPrivateKey, SlhPublicKey};
        use rand_core::CryptoRngCore;
        use zeroize::{Zeroize, ZeroizeOnDrop};


        // ----- 'EXTERNAL' DATA TYPES -----

        /// Correctly sized private key specific to the target security parameter set. <br>
        /// Implements the [`crate::traits::Signer`] and [`crate::traits::SerDes`] traits.
        #[derive(Clone, Zeroize, ZeroizeOnDrop)]
        pub struct PrivateKey(SlhPrivateKey<N>);

        /// Correctly sized public key specific to the target security parameter set. <br>
        /// Implements the [`crate::traits::Verifier`] and [`crate::traits::SerDes`] traits.
        #[derive(Clone, Zeroize, ZeroizeOnDrop)]
        pub struct PublicKey(SlhPublicKey<N>);

        /// Empty struct to enable `KeyGen` trait objects across security parameter sets. <br>
        /// Implements the [`crate::traits::KeyGen`] trait.
        #[derive(Zeroize, ZeroizeOnDrop)]
        pub struct KG(); // Arguable how useful an empty struct+trait is...


        // ----- PRIMARY FUNCTIONS ---

        /// Generates a public and private key pair specific to this security parameter set. <br>
        /// This function utilizes the OS default random number generator, and operates in constant
        /// timing.
        /// # Errors
        /// Returns an error when the random number generator fails.
        /// # Examples
        /// ```rust
        /// use fips205::slh_dsa_shake_128s; // Could use any of the twelve security parameter sets.
        /// use fips205::traits::{SerDes, Signer, Verifier};
        /// # use std::error::Error;
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        ///
        /// let msg_bytes = [0u8, 1, 2, 3, 4, 5, 6, 7];
        ///
        ///
        /// // Generate both public and secret keys. This only fails when the OS rng fails.
        /// let (pk1, sk) = slh_dsa_shake_128s::try_keygen()?;
        /// // Use the secret key to generate a signature. The second parameter is the
        /// // context string (often just an empty &[]), and the last parameter selects
        /// // the preferred hedged variant. This only fails when the OS rng fails.
        /// let sig_bytes = sk.try_sign(&msg_bytes, b"context", true)?;
        ///
        ///
        /// // Serialize the public key, and send with message and signature bytes. These
        /// // statements model sending byte arrays over the wire.
        /// let (pk_send, msg_send, sig_send) = (pk1.into_bytes(), msg_bytes, sig_bytes);
        /// let (pk_recv, msg_recv, sig_recv) = (pk_send, msg_send, sig_send);
        ///
        ///
        /// // Deserialize the public key. This only fails on a malformed key.
        /// let pk2 = slh_dsa_shake_128s::PublicKey::try_from_bytes(&pk_recv)?;
        /// // Use the public key to verify the msg signature
        /// let v = pk2.verify(&msg_recv, &sig_recv, b"context");
        /// assert!(v);
        /// # Ok(())
        /// # }
        /// ```
        #[cfg(feature = "default-rng")]
        pub fn try_keygen() -> Result<(PublicKey, PrivateKey), &'static str> { KG::try_keygen() }


        /// Generates a public and private key pair specific to this security parameter set. <br>
        /// This function utilizes a supplied random number generator, and makes no (constant)
        /// timing assurances.
        /// # Errors
        /// Returns an error when the random number generator fails; propagates internal errors.
        /// # Examples
        /// ```rust
        /// use fips205::slh_dsa_shake_128s; // Could use any of the twelve security parameter sets.
        /// use fips205::traits::{SerDes, Signer, Verifier};
        /// # use std::error::Error;
        /// # use rand_core::OsRng;
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        ///
        /// let msg_bytes = [0u8, 1, 2, 3, 4, 5, 6, 7];
        /// let mut rng = OsRng;
        ///
        /// // Generate both public and secret keys. This only fails when the OS rng fails.
        /// let (pk1, sk) = slh_dsa_shake_128s::try_keygen_with_rng(&mut rng)?;
        /// // Use the secret key to generate a signature. The second parameter is the
        /// // context string (often just an empty &[]), and the last parameter selects
        /// // the preferred hedged variant. This only fails when the OS rng fails.
        /// let sig_bytes = sk.try_sign(&msg_bytes, b"context", true)?;
        ///
        ///
        /// // Serialize the public key, and send with message and signature bytes. These
        /// // statements model sending byte arrays over the wire.
        /// let (pk_send, msg_send, sig_send) = (pk1.into_bytes(), msg_bytes, sig_bytes);
        /// let (pk_recv, msg_recv, sig_recv) = (pk_send, msg_send, sig_send);
        ///
        ///
        /// // Deserialize the public key. This only fails on a malformed key.
        /// let pk2 = slh_dsa_shake_128s::PublicKey::try_from_bytes(&pk_recv)?;
        /// // Use the public key to verify the msg signature
        /// let v = pk2.verify(&msg_recv, &sig_recv, b"context");
        /// assert!(v);
        /// # Ok(())
        /// # }
        /// ```
        pub fn try_keygen_with_rng(
            rng: &mut impl CryptoRngCore,
        ) -> Result<(PublicKey, PrivateKey), &'static str> {
            KG::try_keygen_with_rng(rng)
        }


        impl KeyGen for KG {
            type PrivateKey = PrivateKey;
            type PublicKey = PublicKey;

            // Documented in traits.rs
            fn try_keygen_with_rng(
                rng: &mut impl CryptoRngCore,
            ) -> Result<(PublicKey, PrivateKey), &'static str> {
                let res = crate::slh::slh_keygen_with_rng::<D, H, HP, K, LEN, M, N>(rng, &HASHERS);
                res.map(|(sk, pk)| (PublicKey(pk), PrivateKey(sk)))
            }
        }


        impl Signer for PrivateKey {
            type Signature = [u8; SIG_LEN];
            type PublicKey = PublicKey;

            // Documented in traits.rs
            fn try_sign_with_rng(
                &self, rng: &mut impl CryptoRngCore, m: &[u8], ctx: &[u8], hedged: bool,
            ) -> Result<[u8; SIG_LEN], &'static str> {
                if ctx.len() > 255 {
                    return Err("ctx must be less than 256 bytes");
                };
                let mp: &[&[u8]] = &[&[0u8], &[ctx.len().to_le_bytes()[0]], ctx, m];
                let sig = crate::slh::slh_sign_with_rng::<A, D, H, HP, K, LEN, M, N>(
                    rng, &HASHERS, &mp, &self.0, hedged,
                );
                sig.map(|s| s.serialize())
            }

            // Documented in traits.rs
            fn try_hash_sign_with_rng(
                &self, rng: &mut impl CryptoRngCore, message: &[u8], ctx: &[u8], ph: &Ph,
                hedged: bool,
            ) -> Result<Self::Signature, &'static str> {
                if ctx.len() > 255 {
                    return Err("ctx must be less than 256 bytes");
                };
                let mut phm = [0u8; 64]; // hashers don't all play well with each other (varying output size)
                let (oid, phm_len) = hash_message(message, ph, &mut phm);
                let mp: &[&[u8]] = &[
                    &[1u8],
                    &[ctx.len().to_le_bytes()[0]],
                    ctx,
                    &oid,
                    &phm[0..phm_len],
                ];
                let sig = crate::slh::slh_sign_with_rng::<A, D, H, HP, K, LEN, M, N>(
                    rng, &HASHERS, &mp, &self.0, hedged, // BAD
                );
                sig.map(|s| s.serialize())
            }

            // Documented in traits.rs
            fn get_public_key(&self) -> Self::PublicKey {
                PublicKey(SlhPublicKey{pk_seed: self.0.pk_seed, pk_root: self.0.pk_root})
            }

            // Documented in traits.rs
            fn _test_only_raw_sign(
                &self, rng: &mut impl CryptoRngCore, m: &[u8], hedged: bool,
            ) -> Result<[u8; SIG_LEN], &'static str> {
                let mut opt_rand = (self.0).pk_seed;

                // 4: if (hedged) then    ▷ or to a random n-byte string
                if hedged {
                    // 5: opt_rand ←$ Bn
                    rng.try_fill_bytes(&mut opt_rand).map_err(|_| "Alg17: rng failed")?;

                    // 6: end if
                }
                let sig = crate::slh::slh_sign_internal::<A, D, H, HP, K, LEN, M, N>(
                    &HASHERS,
                    &[m],
                    &self.0,
                    opt_rand,
                );
                sig.map(|s| s.serialize())
            }
        }


        impl Verifier for PublicKey {
            type Signature = [u8; SIG_LEN];

            // Documented in traits.rs
            fn verify(&self, m: &[u8], sig_bytes: &[u8; SIG_LEN], ctx: &[u8]) -> bool {
                if ctx.len() > 255 {
                    return false;
                };
                let sig = SlhDsaSig::<A, D, HP, K, LEN, N>::deserialize(sig_bytes);
                let mp: &[&[u8]] = &[&[0u8], &[ctx.len().to_le_bytes()[0]], ctx, m];
                let res = crate::slh::slh_verify::<A, D, H, HP, K, LEN, M, N>(
                    &HASHERS, &mp, &sig, &self.0,
                );
                res
            }

            // Documented in traits.rs
            fn hash_verify(
                &self, m: &[u8], sig_bytes: &[u8; SIG_LEN], ctx: &[u8], ph: &Ph,
            ) -> bool {
                if ctx.len() > 255 {
                    return false;
                };
                let sig = SlhDsaSig::<A, D, HP, K, LEN, N>::deserialize(sig_bytes);
                let mut phm = [0u8; 64]; // hashers don't all play well with each other (varying output size)
                let (oid, phm_len) = hash_message(m, ph, &mut phm);
                let mp: &[&[u8]] = &[
                    &[1u8],
                    &[ctx.len().to_le_bytes()[0]],
                    ctx,
                    &oid,
                    &phm[0..phm_len],
                ];
                let res = crate::slh::slh_verify::<A, D, H, HP, K, LEN, M, N>(
                    &HASHERS, &mp, &sig, &self.0,
                );
                res
            }

            // Documented in traits.rs
            fn _test_only_raw_verify(
                &self, m: &[u8], sig_bytes: &[u8; SIG_LEN],
            ) -> Result<bool, &'static str> {
                let sig = SlhDsaSig::<A, D, HP, K, LEN, N>::deserialize(sig_bytes);
                let res = crate::slh::slh_verify_internal::<A, D, H, HP, K, LEN, M, N>(
                    &HASHERS,
                    &[m],
                    &sig,
                    &self.0,
                );
                Ok(res)
            }
        }


        // ----- SERIALIZATION AND DESERIALIZATION ---

        impl SerDes for PublicKey {
            type ByteArray = [u8; PK_LEN];

            // Documented in traits.rs
            fn into_bytes(self) -> Self::ByteArray {
                let mut out = [0u8; PK_LEN];
                out[0..(PK_LEN / 2)].copy_from_slice(&self.0.pk_seed);
                out[(PK_LEN / 2)..].copy_from_slice(&self.0.pk_root);
                out
            }

            // Documented in traits.rs
            fn try_from_bytes(bytes: &Self::ByteArray) -> Result<Self, &'static str> {
                let mut pk = SlhPublicKey { pk_seed: [0u8; N], pk_root: [0u8; N] };
                pk.pk_seed.copy_from_slice(&bytes[..(PK_LEN / 2)]);
                pk.pk_root.copy_from_slice(&bytes[(PK_LEN / 2)..]);
                Ok(PublicKey(pk))
            }
        }


        impl SerDes for PrivateKey {
            type ByteArray = [u8; SK_LEN];

            // Documented in traits.rs
            fn into_bytes(self) -> Self::ByteArray {
                let mut bytes = [0u8; SK_LEN];
                bytes[0..(SK_LEN / 4)].copy_from_slice(&self.0.sk_seed);
                bytes[(SK_LEN / 4)..(SK_LEN / 2)].copy_from_slice(&self.0.sk_prf);
                bytes[(SK_LEN / 2)..(3 * SK_LEN / 4)].copy_from_slice(&self.0.pk_seed);
                bytes[(3 * SK_LEN / 4)..].copy_from_slice(&self.0.pk_root);
                bytes
            }

            // Documented in traits.rs
            fn try_from_bytes(bytes: &Self::ByteArray) -> Result<Self, &'static str> {
                let mut sk = SlhPrivateKey {
                    sk_seed: [0u8; N],
                    sk_prf: [0u8; N],
                    pk_seed: [0u8; N],
                    pk_root: [0u8; N],
                };
                sk.sk_seed.copy_from_slice(&bytes[0..(SK_LEN / 4)]);
                sk.sk_prf.copy_from_slice(&bytes[(SK_LEN / 4)..(SK_LEN / 2)]);
                sk.pk_seed.copy_from_slice(&bytes[(SK_LEN / 2)..(3 * SK_LEN / 4)]);
                sk.pk_root.copy_from_slice(&bytes[(3 * SK_LEN / 4)..]);
                let (sk_test, _) = crate::slh::slh_keygen_internal::<D, H, HP, K, LEN, M, N>(&HASHERS, sk.sk_seed, sk.sk_prf, sk.pk_seed);
                if sk_test.pk_root != sk.pk_root { return Err("Corrupted key")}
                Ok(PrivateKey(sk))
            }
        }


        #[cfg(test)]
        mod tests {
            use super::*;
            use rand_chacha::rand_core::SeedableRng;

            // Test keygen, sign, serDes everything, verify true/false
            #[test]
            fn simple_round_trips() {
                let message = [0u8, 1, 2, 3];
                let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(123);
                let (pk1, sk1) = KG::try_keygen_with_rng(&mut rng).unwrap();
                let pk1_bytes = pk1.into_bytes();
                let sk1_bytes = sk1.into_bytes();
                let pk2 = PublicKey::try_from_bytes(&pk1_bytes).unwrap();
                let sk2 = PrivateKey::try_from_bytes(&sk1_bytes).unwrap();

                let sig = sk2.try_sign_with_rng(&mut rng, &message, b"context", true).unwrap();
                let result = pk2.verify(&message, &sig, b"context");
                assert!(result, "Signature failed to verify");

                let (pk3, sk3) = KG::keygen_with_seeds(&[0u8; N], &[1u8; N], &[2u8; N]);
                let sig = sk3.try_sign_with_rng(&mut rng, &message, b"context", true).unwrap();
                let result = pk3.verify(&message, &sig, b"context");

                assert!(result, "Signature failed to verify");
                let result = pk2.verify(&message, &sig, b"some other context");
                assert!(!result, "Signature should not have verified");
                for ph in [Ph::SHA256, Ph::SHA512, Ph::SHAKE128, Ph::SHAKE256] {
                    let sig = sk2
                        .try_hash_sign_with_rng(&mut rng, &message, b"context", &ph, true)
                        .unwrap();
                    let result = pk2.hash_verify(&message, &sig, b"context", &ph);
                    assert!(result, "Signature failed to verify");
                    let result = pk2.hash_verify(&message, &sig, b"some other context", &ph);
                    assert!(!result, "Signature should not have verified");
                }
            }
        }
    };
}


/// Functionality for the **SLH-DSA-SHA2-128s** security parameter set per FIPS 205 section 11.
///
/// This includes specific sizes for the public key, secret key, and signature along with a number of internal
/// constants. The SLH-DSA-SHA2-128s parameter set is claimed to be in security strength category 1.
///
/// **1)** The basic usage is for an originator to start with the [`slh_dsa_sha2_128s::try_keygen`] function below
/// to generate both [`slh_dsa_sha2_128s::PublicKey`] and [`slh_dsa_sha2_128s::PrivateKey`] structs. The resulting
/// [`slh_dsa_sha2_128s::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies several functions
/// to sign byte-array messages, such as [`traits::Signer::try_sign()`], resulting in a Signature byte-array.
///
/// **2)** Both the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait. The originator
/// utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the `PublicKey` struct into a byte-array for
/// distribution. The remote party utilizes the [`traits::SerDes::try_from_bytes()`] function to deserialize the
/// `PublicKey` byte-array into its struct.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`slh_dsa_sha2_128s::PublicKey`] struct to verify the message byte-array with the Signature byte-array..
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "slh_dsa_sha2_128s")]
pub mod slh_dsa_sha2_128s {
    use crate::hashers::sha2_cat_1::{f, h, h_msg, prf, prf_msg, t_l};
    use crate::hashers::Hashers;

    /// Seed size
    pub const N: usize = 16;
    const H: usize = 63;
    const D: usize = 7;
    const HP: usize = 9;
    const A: usize = 12;
    const K: usize = 14;
    const M: usize = 30;
    const LEN: usize = 2 * N + 3;

    /// Length of public key
    pub const PK_LEN: usize = 32;

    /// Length of signature byte-array
    pub const SIG_LEN: usize = 7856;

    /// Length of private/secret key
    pub const SK_LEN: usize = PK_LEN * 2;

    static HASHERS: Hashers<K, LEN, M, N> =
        Hashers::<K, LEN, M, N> { h_msg, prf, prf_msg, f, h, t_l, t_len: t_l };

    functionality!();
}


/// Functionality for the **SLH-DSA-SHAKE-128s** security parameter set per FIPS 205 section 11.
///
/// This includes specific sizes for the public key, secret key, and signature along with a number of internal
/// constants. The SLH-DSA-SHAKE-128s parameter set is claimed to be in security strength category 1.
///
/// **1)** The basic usage is for an originator to start with the [`slh_dsa_shake_128s::try_keygen`] function below
/// to generate both [`slh_dsa_shake_128s::PublicKey`] and [`slh_dsa_shake_128s::PrivateKey`] structs. The resulting
/// [`slh_dsa_shake_128s::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies several functions
/// to sign byte-array messages, such as [`traits::Signer::try_sign()`], resulting in a Signature byte-array.
///
/// **2)** Both the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait. The originator
/// utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the `PublicKey` struct into a byte-array for
/// distribution. The remote party utilizes the [`traits::SerDes::try_from_bytes()`] function to deserialize the
/// `PublicKey` byte-array into its struct.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`slh_dsa_shake_128s::PublicKey`] struct to verify the message byte-array with the Signature byte-array..
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "slh_dsa_shake_128s")]
pub mod slh_dsa_shake_128s {
    use crate::hashers::shake::{f, h, h_msg, prf, prf_msg, t_l};
    use crate::hashers::Hashers;

    /// Seed size
    pub const N: usize = 16;
    const H: usize = 63;
    const D: usize = 7;
    const HP: usize = 9;
    const A: usize = 12;
    const K: usize = 14;
    const M: usize = 30;
    const LEN: usize = 2 * N + 3;

    /// Length of public key
    pub const PK_LEN: usize = 32;

    /// Length of signature byte-array
    pub const SIG_LEN: usize = 7856;

    /// Length of private/secret key
    pub const SK_LEN: usize = PK_LEN * 2;

    static HASHERS: Hashers<K, LEN, M, N> =
        Hashers::<K, LEN, M, N> { h_msg, prf, prf_msg, f, h, t_l, t_len: t_l };

    functionality!();
}


/// Functionality for the **SLH-DSA-SHA2-128f** security parameter set per FIPS 205 section 11.
///
/// This includes specific sizes for the public key, secret key, and signature along with a number of internal
/// constants. The SLH-DSA-SHA2-128f parameter set is claimed to be in security strength category 1.
///
/// **1)** The basic usage is for an originator to start with the [`slh_dsa_sha2_128f::try_keygen`] function below
/// to generate both [`slh_dsa_sha2_128f::PublicKey`] and [`slh_dsa_sha2_128f::PrivateKey`] structs. The resulting
/// [`slh_dsa_sha2_128f::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies several functions
/// to sign byte-array messages, such as [`traits::Signer::try_sign()`], resulting in a Signature byte-array.
///
/// **2)** Both the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait. The originator
/// utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the `PublicKey` struct into a byte-array for
/// distribution. The remote party utilizes the [`traits::SerDes::try_from_bytes()`] function to deserialize the
/// `PublicKey` byte-array into its struct.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`slh_dsa_sha2_128f::PublicKey`] struct to verify the message byte-array with the Signature byte-array..
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "slh_dsa_sha2_128f")]
pub mod slh_dsa_sha2_128f {
    use crate::hashers::sha2_cat_1::{f, h, h_msg, prf, prf_msg, t_l};
    use crate::hashers::Hashers;

    /// Seed size
    pub const N: usize = 16;
    const H: usize = 66;
    const D: usize = 22;
    const HP: usize = 3;
    const A: usize = 6;
    const K: usize = 33;
    const M: usize = 34;
    const LEN: usize = 2 * N + 3;

    /// Length of public key
    pub const PK_LEN: usize = 32;

    /// Length of signature byte-array
    pub const SIG_LEN: usize = 17088;

    /// Length of private/secret key
    pub const SK_LEN: usize = PK_LEN * 2;

    static HASHERS: Hashers<K, LEN, M, N> =
        Hashers::<K, LEN, M, N> { h_msg, prf, prf_msg, f, h, t_l, t_len: t_l };

    functionality!();
}


/// Functionality for the **SLH-DSA-SHAKE-128f** security parameter set per FIPS 205 section 11.
///
/// This includes specific sizes for the public key, secret key, and signature along with a number of internal
/// constants. The SLH-DSA-SHAKE-128f parameter set is claimed to be in security strength category 1.
///
/// **1)** The basic usage is for an originator to start with the [`slh_dsa_shake_128f::try_keygen`] function below
/// to generate both [`slh_dsa_shake_128f::PublicKey`] and [`slh_dsa_shake_128f::PrivateKey`] structs. The resulting
/// [`slh_dsa_shake_128f::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies several functions
/// to sign byte-array messages, such as [`traits::Signer::try_sign()`], resulting in a Signature byte-array.
///
/// **2)** Both the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait. The originator
/// utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the `PublicKey` struct into a byte-array for
/// distribution. The remote party utilizes the [`traits::SerDes::try_from_bytes()`] function to deserialize the
/// `PublicKey` byte-array into its struct.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`slh_dsa_shake_128f::PublicKey`] struct to verify the message byte-array with the Signature byte-array..
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "slh_dsa_shake_128f")]
pub mod slh_dsa_shake_128f {
    use crate::hashers::shake::{f, h, h_msg, prf, prf_msg, t_l};
    use crate::hashers::Hashers;

    /// Seed size
    pub const N: usize = 16;
    const H: usize = 66;
    const D: usize = 22;
    const HP: usize = 3;
    const A: usize = 6;
    const K: usize = 33;
    const M: usize = 34;
    const LEN: usize = 2 * N + 3;

    /// Length of public key
    pub const PK_LEN: usize = 32;

    /// Length of signature byte-array
    pub const SIG_LEN: usize = 17088;

    /// Length of private/secret key
    pub const SK_LEN: usize = PK_LEN * 2;

    static HASHERS: Hashers<K, LEN, M, N> =
        Hashers::<K, LEN, M, N> { h_msg, prf, prf_msg, f, h, t_l, t_len: t_l };

    functionality!();
}


/// Functionality for the **SLH-DSA-SHA2-192s** security parameter set per FIPS 205 section 11.
///
/// This includes specific sizes for the public key, secret key, and signature along with a number of internal
/// constants. The SLH-DSA-SHA2-192s parameter set is claimed to be in security strength category 3.
///
/// **1)** The basic usage is for an originator to start with the [`slh_dsa_sha2_192s::try_keygen`] function below
/// to generate both [`slh_dsa_sha2_192s::PublicKey`] and [`slh_dsa_sha2_192s::PrivateKey`] structs. The resulting
/// [`slh_dsa_sha2_192s::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies several functions
/// to sign byte-array messages, such as [`traits::Signer::try_sign()`], resulting in a Signature byte-array.
///
/// **2)** Both the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait. The originator
/// utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the `PublicKey` struct into a byte-array for
/// distribution. The remote party utilizes the [`traits::SerDes::try_from_bytes()`] function to deserialize the
/// `PublicKey` byte-array into its struct.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`slh_dsa_sha2_192s::PublicKey`] struct to verify the message byte-array with the Signature byte-array..
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "slh_dsa_sha2_192s")]
pub mod slh_dsa_sha2_192s {
    use crate::hashers::sha2_cat_3_5::{f, h, h_msg, prf, prf_msg, t_l};
    use crate::hashers::Hashers;

    /// Seed size
    pub const N: usize = 24;
    const H: usize = 63;
    const D: usize = 7;
    const HP: usize = 9;
    const A: usize = 14;
    const K: usize = 17;
    const M: usize = 39;
    const LEN: usize = 2 * N + 3;

    /// Length of public key
    pub const PK_LEN: usize = 48;

    /// Length of signature byte-array
    pub const SIG_LEN: usize = 16224;

    /// Length of private/secret key
    pub const SK_LEN: usize = PK_LEN * 2;

    static HASHERS: Hashers<K, LEN, M, N> =
        Hashers::<K, LEN, M, N> { h_msg, prf, prf_msg, f, h, t_l, t_len: t_l };

    functionality!();
}


/// Functionality for the **SLH-DSA-SHAKE-192s** security parameter set per FIPS 205 section 11.
///
/// This includes specific sizes for the public key, secret key, and signature along with a number of internal
/// constants. The SLH-DSA-SHAKE-192s parameter set is claimed to be in security strength category 3.
///
/// **1)** The basic usage is for an originator to start with the [`slh_dsa_shake_192s::try_keygen`] function below
/// to generate both [`slh_dsa_shake_192s::PublicKey`] and [`slh_dsa_shake_192s::PrivateKey`] structs. The resulting
/// [`slh_dsa_shake_192s::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies several functions
/// to sign byte-array messages, such as [`traits::Signer::try_sign()`], resulting in a Signature byte-array.
///
/// **2)** Both the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait. The originator
/// utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the `PublicKey` struct into a byte-array for
/// distribution. The remote party utilizes the [`traits::SerDes::try_from_bytes()`] function to deserialize the
/// `PublicKey` byte-array into its struct.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`slh_dsa_shake_192s::PublicKey`] struct to verify the message byte-array with the Signature byte-array..
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "slh_dsa_shake_192s")]
pub mod slh_dsa_shake_192s {
    use crate::hashers::shake::{f, h, h_msg, prf, prf_msg, t_l};
    use crate::hashers::Hashers;

    /// Seed size
    pub const N: usize = 24;
    const H: usize = 63;
    const D: usize = 7;
    const HP: usize = 9;
    const A: usize = 14;
    const K: usize = 17;
    const M: usize = 39;
    const LEN: usize = 2 * N + 3;

    /// Length of public key
    pub const PK_LEN: usize = 48;

    /// Length of signature byte-array
    pub const SIG_LEN: usize = 16224;

    /// Length of private/secret key
    pub const SK_LEN: usize = PK_LEN * 2;

    static HASHERS: Hashers<K, LEN, M, N> =
        Hashers::<K, LEN, M, N> { h_msg, prf, prf_msg, f, h, t_l, t_len: t_l };

    functionality!();
}


/// Functionality for the **SLH-DSA-SHA2-192f** security parameter set per FIPS 205 section 11.
///
/// This includes specific sizes for the public key, secret key, and signature along with a number of internal
/// constants. The SLH-DSA-SHA2-192f parameter set is claimed to be in security strength category 3.
///
/// **1)** The basic usage is for an originator to start with the [`slh_dsa_sha2_192f::try_keygen`] function below
/// to generate both [`slh_dsa_sha2_192f::PublicKey`] and [`slh_dsa_sha2_192f::PrivateKey`] structs. The resulting
/// [`slh_dsa_sha2_192f::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies several functions
/// to sign byte-array messages, such as [`traits::Signer::try_sign()`], resulting in a Signature byte-array.
///
/// **2)** Both the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait. The originator
/// utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the `PublicKey` struct into a byte-array for
/// distribution. The remote party utilizes the [`traits::SerDes::try_from_bytes()`] function to deserialize the
/// `PublicKey` byte-array into its struct.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`slh_dsa_sha2_192f::PublicKey`] struct to verify the message byte-array with the Signature byte-array..
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "slh_dsa_sha2_192f")]
pub mod slh_dsa_sha2_192f {
    use crate::hashers::sha2_cat_3_5::{f, h, h_msg, prf, prf_msg, t_l};
    use crate::hashers::Hashers;

    /// Seed size
    pub const N: usize = 24;
    const H: usize = 66;
    const D: usize = 22;
    const HP: usize = 3;
    const A: usize = 8;
    const K: usize = 33;
    const M: usize = 42;
    const LEN: usize = 2 * N + 3;

    /// Length of public key
    pub const PK_LEN: usize = 48;

    /// Length of signature byte-array
    pub const SIG_LEN: usize = 35664;

    /// Length of private/secret key
    pub const SK_LEN: usize = PK_LEN * 2;

    static HASHERS: Hashers<K, LEN, M, N> =
        Hashers::<K, LEN, M, N> { h_msg, prf, prf_msg, f, h, t_l, t_len: t_l };

    functionality!();
}


/// Functionality for the **SLH-DSA-SHAKE-192f** security parameter set per FIPS 205 section 11.
///
/// This includes specific sizes for the public key, secret key, and signature along with a number of internal
/// constants. The SLH-DSA-SHAKE-192f parameter set is claimed to be in security strength category 3.
///
/// **1)** The basic usage is for an originator to start with the [`slh_dsa_shake_192f::try_keygen`] function below
/// to generate both [`slh_dsa_shake_192f::PublicKey`] and [`slh_dsa_shake_192f::PrivateKey`] structs. The resulting
/// [`slh_dsa_shake_192f::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies several functions
/// to sign byte-array messages, such as [`traits::Signer::try_sign()`], resulting in a Signature byte-array.
///
/// **2)** Both the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait. The originator
/// utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the `PublicKey` struct into a byte-array for
/// distribution. The remote party utilizes the [`traits::SerDes::try_from_bytes()`] function to deserialize the
/// `PublicKey` byte-array into its struct.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`slh_dsa_shake_192f::PublicKey`] struct to verify the message byte-array with the Signature byte-array..
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "slh_dsa_shake_192f")]
pub mod slh_dsa_shake_192f {
    use crate::hashers::shake::{f, h, h_msg, prf, prf_msg, t_l};
    use crate::hashers::Hashers;

    /// Seed size
    pub const N: usize = 24;
    const H: usize = 66;
    const D: usize = 22;
    const HP: usize = 3;
    const A: usize = 8;
    const K: usize = 33;
    const M: usize = 42;
    const LEN: usize = 2 * N + 3;

    /// Length of public key
    pub const PK_LEN: usize = 48;

    /// Length of signature byte-array
    pub const SIG_LEN: usize = 35664;

    /// Length of private/secret key
    pub const SK_LEN: usize = PK_LEN * 2;

    static HASHERS: Hashers<K, LEN, M, N> =
        Hashers::<K, LEN, M, N> { h_msg, prf, prf_msg, f, h, t_l, t_len: t_l };

    functionality!();
}


/// Functionality for the **SLH-DSA-SHA2-256s** security parameter set per FIPS 205 section 11.
///
/// This includes specific sizes for the public key, secret key, and signature along with a number of internal
/// constants. The SLH-DSA-SHA2-256s parameter set is claimed to be in security strength category 5.
///
/// **1)** The basic usage is for an originator to start with the [`slh_dsa_sha2_256s::try_keygen`] function below
/// to generate both [`slh_dsa_sha2_256s::PublicKey`] and [`slh_dsa_sha2_256s::PrivateKey`] structs. The resulting
/// [`slh_dsa_sha2_256s::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies several functions
/// to sign byte-array messages, such as [`traits::Signer::try_sign()`], resulting in a Signature byte-array.
///
/// **2)** Both the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait. The originator
/// utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the `PublicKey` struct into a byte-array for
/// distribution. The remote party utilizes the [`traits::SerDes::try_from_bytes()`] function to deserialize the
/// `PublicKey` byte-array into its struct.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`slh_dsa_sha2_256s::PublicKey`] struct to verify the message byte-array with the Signature byte-array..
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "slh_dsa_sha2_256s")]
pub mod slh_dsa_sha2_256s {
    use crate::hashers::sha2_cat_3_5::{f, h, h_msg, prf, prf_msg, t_l};
    use crate::hashers::Hashers;

    /// Seed size
    pub const N: usize = 32;
    const H: usize = 64;
    const D: usize = 8;
    const HP: usize = 8;
    const A: usize = 14;
    const K: usize = 22;
    const M: usize = 47;
    const LEN: usize = 2 * N + 3;

    /// Length of public key
    pub const PK_LEN: usize = 64;

    /// Length of signature byte-array
    pub const SIG_LEN: usize = 29792;

    /// Length of private/secret key
    pub const SK_LEN: usize = PK_LEN * 2;

    static HASHERS: Hashers<K, LEN, M, N> =
        Hashers::<K, LEN, M, N> { h_msg, prf, prf_msg, f, h, t_l, t_len: t_l };

    functionality!();
}


/// Functionality for the **SLH-DSA-SHAKE-256s** security parameter set per FIPS 205 section 11.
///
/// This includes specific sizes for the public key, secret key, and signature along with a number of internal
/// constants. The SLH-DSA-SHAKE_256s parameter set is claimed to be in security strength category 5.
///
/// **1)** The basic usage is for an originator to start with the [`slh_dsa_shake_256s::try_keygen`] function below
/// to generate both [`slh_dsa_shake_256s::PublicKey`] and [`slh_dsa_shake_256s::PrivateKey`] structs. The resulting
/// [`slh_dsa_shake_256s::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies several functions
/// to sign byte-array messages, such as [`traits::Signer::try_sign()`], resulting in a Signature byte-array.
///
/// **2)** Both the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait. The originator
/// utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the `PublicKey` struct into a byte-array for
/// distribution. The remote party utilizes the [`traits::SerDes::try_from_bytes()`] function to deserialize the
/// `PublicKey` byte-array into its struct.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`slh_dsa_shake_256s::PublicKey`] struct to verify the message byte-array with the Signature byte-array..
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "slh_dsa_shake_256s")]
pub mod slh_dsa_shake_256s {
    use crate::hashers::shake::{f, h, h_msg, prf, prf_msg, t_l};
    use crate::hashers::Hashers;

    /// Seed size
    pub const N: usize = 32;
    const H: usize = 64;
    const D: usize = 8;
    const HP: usize = 8;
    const A: usize = 14;
    const K: usize = 22;
    const M: usize = 47;
    const LEN: usize = 2 * N + 3;

    /// Length of public key
    pub const PK_LEN: usize = 64;

    /// Length of signature byte-array
    pub const SIG_LEN: usize = 29792;

    /// Length of private/secret key
    pub const SK_LEN: usize = PK_LEN * 2;

    static HASHERS: Hashers<K, LEN, M, N> =
        Hashers::<K, LEN, M, N> { h_msg, prf, prf_msg, f, h, t_l, t_len: t_l };

    functionality!();
}


/// Functionality for the **SLH-DSA-SHA2-256f** security parameter set per FIPS 205 section 11.
///
/// This includes specific sizes for the public key, secret key, and signature along with a number of internal
/// constants. The SLH-DSA-SHA2-256f parameter set is claimed to be in security strength category 5.
///
/// **1)** The basic usage is for an originator to start with the [`slh_dsa_sha2_256f::try_keygen`] function below
/// to generate both [`slh_dsa_sha2_256f::PublicKey`] and [`slh_dsa_sha2_256f::PrivateKey`] structs. The resulting
/// [`slh_dsa_sha2_256f::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies several functions
/// to sign byte-array messages, such as [`traits::Signer::try_sign()`], resulting in a Signature byte-array.
///
/// **2)** Both the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait. The originator
/// utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the `PublicKey` struct into a byte-array for
/// distribution. The remote party utilizes the [`traits::SerDes::try_from_bytes()`] function to deserialize the
/// `PublicKey` byte-array into its struct.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`slh_dsa_sha2_256f::PublicKey`] struct to verify the message byte-array with the Signature byte-array..
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "slh_dsa_sha2_256f")]
pub mod slh_dsa_sha2_256f {
    use crate::hashers::sha2_cat_3_5::{f, h, h_msg, prf, prf_msg, t_l};
    use crate::hashers::Hashers;

    /// Seed size
    pub const N: usize = 32;
    const H: usize = 68;
    const D: usize = 17;
    const HP: usize = 4;
    const A: usize = 9;
    const K: usize = 35;
    const M: usize = 49;
    const LEN: usize = 2 * N + 3;

    /// Length of public key
    pub const PK_LEN: usize = 64;

    /// Length of signature byte-array
    pub const SIG_LEN: usize = 49856;

    /// Length of private/secret key
    pub const SK_LEN: usize = PK_LEN * 2;

    static HASHERS: Hashers<K, LEN, M, N> =
        Hashers::<K, LEN, M, N> { h_msg, prf, prf_msg, f, h, t_l, t_len: t_l };

    functionality!();
}


/// Functionality for the **SLH-DSA-SHAKE-256f** security parameter set per FIPS 205 section 11.
///
/// This includes specific sizes for the public key, secret key, and signature along with a number of internal
/// constants. The SLH-DSA-SHAKE-256f parameter set is claimed to be in security strength category 5.
///
/// **1)** The basic usage is for an originator to start with the [`slh_dsa_shake_256f::try_keygen`] function below
/// to generate both [`slh_dsa_shake_256f::PublicKey`] and [`slh_dsa_shake_256f::PrivateKey`] structs. The resulting
/// [`slh_dsa_shake_256f::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies several functions
/// to sign byte-array messages, such as [`traits::Signer::try_sign()`], resulting in a Signature byte-array.
///
/// **2)** Both the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait. The originator
/// utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the `PublicKey` struct into a byte-array for
/// distribution. The remote party utilizes the [`traits::SerDes::try_from_bytes()`] function to deserialize the
/// `PublicKey` byte-array into its struct.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`slh_dsa_shake_256f::PublicKey`] struct to verify the message byte-array with the Signature byte-array..
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "slh_dsa_shake_256f")]
pub mod slh_dsa_shake_256f {
    use crate::hashers::shake::{f, h, h_msg, prf, prf_msg, t_l};
    use crate::hashers::Hashers;

    /// Seed size
    pub const N: usize = 32;
    const H: usize = 68;
    const D: usize = 17;
    const HP: usize = 4;
    const A: usize = 9;
    const K: usize = 35;
    const M: usize = 49;
    const LEN: usize = 2 * N + 3;

    /// Length of public key
    pub const PK_LEN: usize = 64;

    /// Length of signature byte-array
    pub const SIG_LEN: usize = 49856;

    /// Length of private/secret key
    pub const SK_LEN: usize = PK_LEN * 2;

    static HASHERS: Hashers<K, LEN, M, N> =
        Hashers::<K, LEN, M, N> { h_msg, prf, prf_msg, f, h, t_l, t_len: t_l };

    functionality!();
}
