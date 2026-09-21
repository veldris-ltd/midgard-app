use rand_core::CryptoRngCore;
use rand_core::RngCore;
use rand_core::CryptoRng;

use crate::Ph;
#[cfg(feature = "default-rng")]
use rand_core::OsRng;


/// The `KeyGen` trait is defined to allow trait objects.
pub trait KeyGen {
    /// A public key specific to the chosen security parameter set, e.g., `slh_dsa_shake_128s`, `slh_dsa_sha2_128s` etc
    type PublicKey;
    /// A private (secret) key specific to the chosen security parameter set, e.g., `slh_dsa_shake_128s`, `slh_dsa_sha2_128s` etc
    type PrivateKey;


    /// Generates a public and private key pair specific to this security parameter set.
    /// This function utilizes the **OS default** random number generator. This function operates
    /// in constant-time relative to secret data.
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
    fn try_keygen() -> Result<(Self::PublicKey, Self::PrivateKey), &'static str> {
        Self::try_keygen_with_rng(&mut OsRng)
    }


    /// Generates a public and private key pair specific to this security parameter set.
    /// This function utilizes the **provided** random number generator. This function operates
    /// in constant-time relative to secret data.
    /// # Errors
    /// Returns an error when the random number generator fails.
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
    /// // Generate both public and secret keys. This only fails when the provided rng fails.
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
    fn try_keygen_with_rng(
        rng: &mut impl CryptoRngCore,
    ) -> Result<(Self::PublicKey, Self::PrivateKey), &'static str>;

    /// Generates a public and private key pair specific to this security parameter set.
    /// This function utilizes **three provided seeds** rather than a random number
    /// generator in order to deterministically generate keys. This function operates
    /// in constant-time relative to secret data.
    /// # Errors
    /// Returns an error when the random number generator fails.
    /// # Examples
    /// ```rust
    /// use fips205::slh_dsa_shake_128s; // Could use any of the twelve security parameter sets.
    /// use fips205::traits::{KeyGen, SerDes, Signer, Verifier};
    /// # use std::error::Error;
    /// # use rand_core::OsRng;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    ///
    /// let msg_bytes = [0u8, 1, 2, 3, 4, 5, 6, 7];
    /// let mut rng = OsRng;
    ///
    /// // Generate both public and secret keys. This only fails when the provided rng fails.
    /// let (pk1, sk) = slh_dsa_shake_128s::KG::keygen_with_seeds(&[0u8; slh_dsa_shake_128s::N],
    ///                 &[1u8; slh_dsa_shake_128s::N], &[2u8; slh_dsa_shake_128s::N]);
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
    #[must_use]
    fn keygen_with_seeds<const N: usize>(
        sk_seed: &[u8; N], sk_prf: &[u8; N], pk_seed: &[u8; N]
    ) -> (Self::PublicKey, Self::PrivateKey) {
        Self::try_keygen_with_rng(&mut DummyRng {data: [*sk_seed, *sk_prf, *pk_seed], i: 0 }).expect("rng will not fail")
    }
}

// This is for the deterministic keygen functions; will be refactored more nicely
struct DummyRng<const N: usize> { data: [[u8; N]; 3], i: usize }

impl<const N: usize> RngCore for DummyRng<N> {
    fn next_u32(&mut self) -> u32 { unimplemented!() }

    fn next_u64(&mut self) -> u64 { unimplemented!() }

    fn fill_bytes(&mut self, _out: &mut [u8]) { unimplemented!() }

    fn try_fill_bytes(&mut self, out: &mut [u8]) -> Result<(), rand_core::Error> {
        out.copy_from_slice(&self.data[self.i]);
        self.i += 1;
        Ok(())
    }
}

impl<const N: usize> CryptoRng for DummyRng<N> {}


/// The Signer trait is implemented for the `PrivateKey` struct on each of the security parameter sets
pub trait Signer {
    /// The signature is specific to the chosen security parameter set, e.g., `slh_dsa_shake_128s`, `slh_dsa_sha2_128s` etc
    type Signature;
    /// The public key that corresponds to the private/secret key
    type PublicKey;

    /// Attempt to sign the given message, returning a digital signature on success, or an error if
    /// something went wrong. This function utilizes the **OS default** random number generator.
    /// This function operates in constant-time relative to secret data (excluding the random number
    /// generator internals). Uses a FIPS 205 context string (default: an empty string).
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
    fn try_sign(
        &self, message: &[u8], ctx: &[u8], hedged: bool,
    ) -> Result<Self::Signature, &'static str> {
        self.try_sign_with_rng(&mut OsRng, message, ctx, hedged)
    }


    /// Attempt to sign the hash of a given message, returning a digital signature on success, or an
    /// error if something went wrong. This function utilizes the **OS default** random number
    /// generator. This function operates in constant-time relative to secret data (excluding the
    /// random number generator internals). Uses a FIPS 205 context string (default: an empty string).
    /// # Errors
    /// Returns an error when the random number generator fails.
    /// # Examples
    /// ```rust
    /// use fips205::slh_dsa_shake_128s; // Could use any of the twelve security parameter sets.
    /// use fips205::traits::{SerDes, Signer, Verifier};
    /// # use std::error::Error;
    /// # use fips205::Ph;
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
    /// let sig_bytes = sk.try_hash_sign(&msg_bytes, b"context", &Ph::SHA256, true)?;
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
    /// // Use the public key to verify the signature on the message hash
    /// let v = pk2.hash_verify(&msg_recv, &sig_recv, b"context", &Ph::SHA256);
    /// assert!(v);
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "default-rng")]
    fn try_hash_sign(
        &self, message: &[u8], ctx: &[u8], ph: &Ph, hedged: bool,
    ) -> Result<Self::Signature, &'static str> {
        self.try_hash_sign_with_rng(&mut OsRng, message, ctx, ph, hedged)
    }


    /// Attempt to sign a given message, returning a digital signature on success, or an
    /// error if something went wrong. This function utilizes a **provided** random number generator.
    /// This function operates in constant-time relative to secret data (excluding the random number
    /// generator internals). Uses a FIPS 205 context string (default: an empty string).
    ///
    /// # Errors
    /// Returns an error when the random number generator fails.
    /// # Examples
    /// ```rust
    /// use fips205::slh_dsa_shake_128s; // Could use any of the twelve security parameter sets.
    /// use fips205::traits::{SerDes, Signer, Verifier};
    /// # use std::error::Error;
    /// # use rand_core::OsRng;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    ///
    /// let msg_bytes = [0u8, 1, 2, 3, 4, 5, 6, 7];
    /// let mut rng = OsRng;
    ///
    /// // Generate both public and secret keys. This only fails when the OS rng fails.
    /// let (pk1, sk) = slh_dsa_shake_128s::try_keygen()?;
    /// // Use the secret key to generate a signature. The third parameter is the
    /// // context string (often just an empty &[]), and the last parameter selects
    /// // the preferred hedged variant. This only fails when the provided rng fails.
    /// let sig_bytes = sk.try_sign_with_rng(&mut rng, &msg_bytes, b"context", true)?;
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
    fn try_sign_with_rng(
        &self, rng: &mut impl CryptoRngCore, message: &[u8], ctx: &[u8], hedged: bool,
    ) -> Result<Self::Signature, &'static str>;


    /// Attempt to sign the hash of a given message, returning a digital signature on success, or an
    /// error if something went wrong. This function utilizes a **provided** random number generator.
    /// This function operates in constant-time relative to secret data (excluding the random number
    /// generator internals). Uses a FIPS 205 context string (default: an empty string).
    ///
    /// # Errors
    /// Returns an error when the random number generator fails.
    /// # Examples
    /// ```rust
    /// use fips205::slh_dsa_shake_128s; // Could use any of the twelve security parameter sets.
    /// use fips205::traits::{SerDes, Signer, Verifier};
    /// # use std::error::Error;
    /// # use rand_core::OsRng;
    /// # use fips205::Ph;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    ///
    /// let msg_bytes = [0u8, 1, 2, 3, 4, 5, 6, 7];
    /// let mut rng = OsRng;
    ///
    ///
    /// // Generate both public and secret keys. This only fails when the OS rng fails.
    /// let (pk1, sk) = slh_dsa_shake_128s::try_keygen()?;
    /// // Use the secret key to generate a signature. The third parameter is the
    /// // context string (often just an empty &[]), and the last parameter selects
    /// // the preferred hedged variant. This only fails when the provided rng fails.
    /// let sig_bytes =
    ///     sk.try_hash_sign_with_rng(&mut rng, &msg_bytes, b"context", &Ph::SHA512, true)?;
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
    /// let v = pk2.hash_verify(&msg_recv, &sig_recv, b"context", &Ph::SHA512);
    /// assert!(v);
    /// # Ok(())
    /// # }
    /// ```
    fn try_hash_sign_with_rng(
        &self, rng: &mut impl CryptoRngCore, message: &[u8], ctx: &[u8], ph: &Ph, hedged: bool,
    ) -> Result<Self::Signature, &'static str>;


    /// Retrieves the public key associated with this private/secret key
    /// # Examples
    /// ```rust
    /// use fips205::slh_dsa_shake_128s; // Could use any of the twelve security parameter sets.
    /// use fips205::traits::{SerDes, Signer, Verifier};
    /// # use std::error::Error;
    /// # use rand_core::OsRng;
    /// # use fips205::Ph;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    ///
    /// let msg_bytes = [0u8, 1, 2, 3, 4, 5, 6, 7];
    /// let mut rng = OsRng;
    ///
    ///
    /// // Generate both public and secret keys, but only hang onto the secret key.
    /// let (_, sk) = slh_dsa_shake_128s::try_keygen()?;
    ///
    /// // The public key can be derived from the secret key
    /// let pk = sk.get_public_key();
    /// # Ok(())
    /// # }
    /// ```
    fn get_public_key(&self) -> Self::PublicKey;


    /// As of October 4 2024, the available NIST test vectors are applied to the **internal** functions
    /// rather than the external API. This function should not be used outside of this scenario.
    /// # Errors
    #[deprecated = "Temporary function to allow application of internal nist vectors; will be removed"]
    fn _test_only_raw_sign(
        &self, rng: &mut impl CryptoRngCore, m: &[u8], hedged: bool,
    ) -> Result<Self::Signature, &'static str>;
}


/// The Verifier trait is implemented for `PublicKey` on each of the security parameter sets
pub trait Verifier {
    /// The signature is specific to the chosen security parameter set, e.g., `slh_dsa_shake_128s`, `slh_dsa_sha2_128s` etc
    type Signature;


    /// Verifies a digital signature with respect to a `PublicKey`. This function does not operates on
    /// secret data, so it need/does not provide constant-time assurances. Uses a FIPS 205 context string
    /// (default: an empty string).
    ///
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
    #[must_use]
    fn verify(&self, message: &[u8], signature: &Self::Signature, ctx: &[u8]) -> bool;


    /// Verifies a digital signature on the hash of a message with respect to a `PublicKey`. As this
    /// function operates on purely public data, it need/does not provide constant-time assurances.
    ///
    /// # Examples
    /// ```rust
    /// use fips205::slh_dsa_shake_128s; // Could use any of the twelve security parameter sets.
    /// use fips205::traits::{SerDes, Signer, Verifier};
    /// # use std::error::Error;
    /// # use fips205::Ph;
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
    /// let sig_bytes = sk.try_hash_sign(&msg_bytes, b"context", &Ph::SHA256, true)?;
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
    /// // Use the public key to verify the signature on the message hash
    /// let v = pk2.hash_verify(&msg_recv, &sig_recv, b"context", &Ph::SHA256);
    /// assert!(v);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    fn hash_verify(&self, message: &[u8], signature: &Self::Signature, ctx: &[u8], ph: &Ph)
        -> bool;


    /// As of October 4 2024, the available NIST test vectors are applied to the **internal** functions
    /// rather than the external API. This function should not be used outside of this scenario.
    /// # Errors
    #[deprecated = "Temporary function to allow application of internal nist vectors; will be removed"]
    fn _test_only_raw_verify(
        &self, m: &[u8], sig_bytes: &Self::Signature,
    ) -> Result<bool, &'static str>;
}


/// The `SerDes` trait provides for validated serialization and deserialization of fixed size elements
pub trait SerDes {
    /// The fixed-size byte array to be serialized or deserialized
    type ByteArray;


    /// Produces a byte array of fixed-size specific to the struct being serialized.
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
    /// // Generate public/private key pair and signature
    /// let (pk1, sk) = slh_dsa_shake_128s::try_keygen()?;  // Generate both public and secret keys
    /// let sig_bytes = sk.try_sign(&msg_bytes, b"context", true)?;  // Use the secret key to generate a msg signature
    ///
    /// // Serialize the public key, and send with message and signature bytes
    /// let (pk_send, msg_send, sig_send) = (pk1.into_bytes(), msg_bytes, sig_bytes);
    /// let (pk_recv, msg_recv, sig_recv) = (pk_send, msg_send, sig_send);
    ///
    /// // Deserialize the public key, then use it to verify the msg signature
    /// let pk2 = slh_dsa_shake_128s::PublicKey::try_from_bytes(&pk_recv)?;
    /// let v = pk2.verify(&msg_recv, &sig_recv, b"context");
    /// assert!(v);
    /// # Ok(())
    /// # }
    /// ```
    fn into_bytes(self) -> Self::ByteArray;


    /// Consumes a byte array of fixed-size specific to the struct being deserialized; performs validation
    /// # Errors
    /// Returns an error on malformed input.
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
    /// // Generate public/private key pair and signature
    /// let (pk1, sk) = slh_dsa_shake_128s::try_keygen()?;  // Generate both public and secret keys
    /// let sig_bytes = sk.try_sign(&msg_bytes, b"context", true)?;  // Use the secret key to generate a msg signature
    ///
    /// // Serialize the public key, and send with message and signature bytes
    /// let (pk_send, msg_send, sig_send) = (pk1.into_bytes(), msg_bytes, sig_bytes);
    /// let (pk_recv, msg_recv, sig_recv) = (pk_send, msg_send, sig_send);
    ///
    /// // Deserialize the public key, then use it to verify the msg signature
    /// let pk2 = slh_dsa_shake_128s::PublicKey::try_from_bytes(&pk_recv)?;
    /// let v = pk2.verify(&msg_recv, &sig_recv, b"context");
    /// assert!(v);
    /// # Ok(())
    /// # }
    /// ```
    fn try_from_bytes(bytes: &Self::ByteArray) -> Result<Self, &'static str>
    where
        Self: Sized;
}
