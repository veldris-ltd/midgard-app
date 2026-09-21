use rand_core::{CryptoRng, CryptoRngCore, RngCore};

#[cfg(feature = "default-rng")]
use rand_core::OsRng;


/// The `KeyGen` trait is defined to allow trait objects.
pub trait KeyGen {
    /// The (public) encapsulation key sent from the originator to the remote party.
    type EncapsKey;
    /// The (private) decapsulation key used by the originator to generate the shared secret.
    type DecapsKey;
    /// A serialized (public) encapsulation key byte array of the correct length.
    type EncapsByteArray;
    /// A serialized (private) decapsulation key byte array of the correct length.
    type DecapsByteArray;


    /// Generates an encapsulation and decapsulation key pair specific to this security parameter set.
    ///
    /// This function uses the OS default random number generator and operates in constant time,
    /// except for the `rho` operation which crosses the trust boundary in the clear.
    ///
    /// # Errors
    /// Returns an error if the random number generator fails.
    ///
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(feature = "ml-kem-512")] {
    /// use fips203::ml_kem_512;  // Could also be ml_kem_768 or ml_kem_1024.
    /// use fips203::traits::{KeyGen, SerDes, Decaps, Encaps};
    ///
    /// let (ek1, dk1) = ml_kem_512::KG::try_keygen()?;  // Party 1 generates both encaps and decaps keys
    /// let ek1_bytes = ek1.into_bytes();  // Party 1 serializes the encaps key
    ///
    /// let ek2_bytes = ek1_bytes;  // Party 1 sends encaps bytes to party 2
    ///
    /// let ek2 = ml_kem_512::EncapsKey::try_from_bytes(ek2_bytes)?;  // Party 2 deserializes the encaps key
    /// let (ssk2, ct2) = ek2.try_encaps()?;  // Party 2 generates shared secret and ciphertext
    /// let ct2_bytes = ct2.into_bytes();  // Party 2 serializes the ciphertext
    ///
    /// let ct1_bytes = ct2_bytes;  // Party 2 sends the ciphertext to party 1
    ///
    /// let ct1 = ml_kem_512::CipherText::try_from_bytes(ct1_bytes)?;  // Party 1 deserializes the ciphertext
    /// let ssk1 = dk1.try_decaps(&ct1)?;  // Party 1 runs decaps to generate the shared secret
    ///
    /// assert_eq!(ssk1, ssk2);  // Each party has the same shared secret
    /// # }
    /// # Ok(())}
    /// ```
    #[cfg(feature = "default-rng")]
    fn try_keygen() -> Result<(Self::EncapsKey, Self::DecapsKey), &'static str> {
        Self::try_keygen_with_rng(&mut OsRng)
    }


    /// Generates an encapsulation and decapsulation key pair using a provided random number generator.
    ///
    /// This function operates in constant time, except for the `rho` operation which crosses
    /// the trust boundary in the clear.
    ///
    /// # Arguments
    /// * `rng` - A cryptographically secure random number generator implementing `CryptoRngCore`
    ///
    /// # Errors
    /// Returns an error if the random number generator fails.
    ///
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(feature = "ml-kem-512")] {
    /// use rand_core::OsRng;
    /// use fips203::ml_kem_512;  // Could also be ml_kem_768 or ml_kem_1024.
    /// use fips203::traits::{KeyGen, SerDes, Decaps, Encaps};
    ///
    /// let (ek1, dk1) = ml_kem_512::KG::try_keygen_with_rng(&mut OsRng)?;  // Party 1 generates both encaps and decaps keys
    /// let ek1_bytes = ek1.into_bytes();  // Party 1 serializes the encaps key
    ///
    /// let ek2_bytes = ek1_bytes;  // Party 1 sends encaps bytes to party 2
    ///
    /// let ek2 = ml_kem_512::EncapsKey::try_from_bytes(ek2_bytes)?;  // Party 2 deserializes the encaps key
    /// let (ssk2, ct2) = ek2.try_encaps_with_rng(&mut OsRng)?;  // Party 2 generates shared secret and ciphertext
    /// let ct2_bytes = ct2.into_bytes();  // Party 2 serializes the ciphertext
    ///
    /// let ct1_bytes = ct2_bytes;  // Party 2 sends the ciphertext to party 1
    ///
    /// let ct1 = ml_kem_512::CipherText::try_from_bytes(ct1_bytes)?;  // Party 1 deserializes the ciphertext
    /// let ssk1 = dk1.try_decaps(&ct1)?;  // Party 1 runs decaps to generate the shared secret
    ///
    /// assert_eq!(ssk1, ssk2);  // Each party has the same shared secret
    /// # }
    /// # Ok(())}
    /// ```
    fn try_keygen_with_rng(
        rng: &mut impl CryptoRngCore,
    ) -> Result<(Self::EncapsKey, Self::DecapsKey), &'static str>;


    /// Generates an encapsulation and decapsulation key key pair specific to this security parameter set
    /// based on a provided seed. <br>
    /// This function is intended to operate in constant time outside of `rho` which crosses the trust
    /// boundary in the clear.
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(feature = "ml-kem-512")] {
    /// use rand_core::OsRng;
    /// use crate::fips203::RngCore;
    /// use fips203::ml_kem_512;  // Could also be ml_kem_768 or ml_kem_1024.
    /// use fips203::traits::{KeyGen, SerDes, Decaps, Encaps};
    ///
    /// // Party 1 gets the d and z seeds from the OS random number generator
    /// let mut d_z = [0u8; 64];
    /// OsRng.fill_bytes(&mut d_z);
    ///
    /// // Party 1 supplies the d and z seeds to key generation to obtain both encaps and decaps keys
    /// let (ek1, dk1) = ml_kem_512::KG::keygen_from_seed(d_z[0..32].try_into().unwrap(), d_z[32..64].try_into().unwrap());
    /// let ek1_bytes = ek1.into_bytes();  // Party 1 serializes the encaps key
    ///
    /// let ek2_bytes = ek1_bytes;  // Party 1 sends encaps bytes to party 2
    ///
    /// let ek2 = ml_kem_512::EncapsKey::try_from_bytes(ek2_bytes)?;  // Party 2 deserializes the encaps key
    /// let (ssk2, ct2) = ek2.try_encaps_with_rng(&mut OsRng)?;  // Party 2 generates shared secret and ciphertext
    /// let ct2_bytes = ct2.into_bytes();  // Party 2 serializes the ciphertext
    ///
    /// let ct1_bytes = ct2_bytes;  // Party 2 sends the ciphertext to party 1
    ///
    /// let ct1 = ml_kem_512::CipherText::try_from_bytes(ct1_bytes)?; // Party 1 deserializes the ciphertext
    /// let ssk1 = dk1.try_decaps(&ct1)?;  // Party 1 runs decaps to generate the shared secret
    ///
    /// assert_eq!(ssk1, ssk2);  // Each party has the same shared secret
    /// # }
    /// # Ok(())}
    /// ```
    #[must_use]
    fn keygen_from_seed(d: [u8; 32], z: [u8; 32]) -> (Self::EncapsKey, Self::DecapsKey);


    /// Performs validation between an encapsulation key and a decapsulation key (both in byte arrays), perhaps in the
    /// scenario where both have been serialized, stored to disk, and then retrieved. This function is not intended
    /// to operate in constant-time.
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(feature = "ml-kem-512")] {
    /// use fips203::ml_kem_512; // Could also be ml_kem_768 or ml_kem_1024.
    /// use fips203::traits::{Decaps, Encaps, KeyGen, SerDes};
    /// use rand_core::OsRng;
    ///
    /// let (ek, dk) = ml_kem_512::KG::try_keygen_with_rng(&mut OsRng)?;
    /// let ek_bytes = ek.into_bytes(); // Serialize and perhaps store-then-restore encaps key
    /// let dk_bytes = dk.into_bytes(); // Serialize and perhaps store-then-restore decaps key
    /// assert!(ml_kem_512::KG::validate_keypair_with_rng_vartime(
    ///     &mut OsRng, &ek_bytes, &dk_bytes
    /// )); // Validate their correspondence
    ///
    /// # }
    /// # Ok(())}
    /// ```
    fn validate_keypair_with_rng_vartime(
        rng: &mut impl CryptoRngCore, ek: &Self::EncapsByteArray, dk: &Self::DecapsByteArray,
    ) -> bool;
}


/// The `Encaps` trait defines methods for generating shared secrets and ciphertexts using
/// an encapsulation key.
pub trait Encaps {
    /// The shared secret key type generated during encapsulation
    type SharedSecretKey;
    /// The ciphertext type transmitted from the encapsulating party to the decapsulating party
    type CipherText;


    /// Generates a shared secret and ciphertext from an encapsulation key specific to this security parameter set. <br>
    /// This function utilizes the OS default random number generator and is intended to operate in constant
    /// time outside of `rho` which crosses the trust boundary in the clear.
    /// # Errors
    /// Returns an error when the random number generator fails or an internal error condition arises.
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(feature = "ml-kem-512")] {
    /// use rand_core::OsRng;
    /// use fips203::ml_kem_512;  // Could also be ml_kem_768 or ml_kem_1024.
    /// use fips203::traits::{KeyGen, SerDes, Decaps, Encaps};
    ///
    /// let (ek1, dk1) = ml_kem_512::KG::try_keygen_with_rng(&mut OsRng)?;  // Party 1 generates both encaps and decaps keys
    /// let ek1_bytes = ek1.into_bytes();  // Party 1 serializes the encaps key
    ///
    /// let ek2_bytes = ek1_bytes;  // Party 1 sends encaps bytes to party 2
    ///
    /// let ek2 = ml_kem_512::EncapsKey::try_from_bytes(ek2_bytes)?;  // Party 2 deserializes the encaps key
    /// let (ssk2, ct2) = ek2.try_encaps()?;  // Party 2 generates shared secret and ciphertext
    /// let ct2_bytes = ct2.into_bytes();  // Party 2 serializes the ciphertext
    ///
    /// let ct1_bytes = ct2_bytes;  // Party 2 sends the ciphertext to party 1
    ///
    /// let ct1 = ml_kem_512::CipherText::try_from_bytes(ct1_bytes)?;  // Party 1 deserializes the ciphertext
    /// let ssk1 = dk1.try_decaps(&ct1)?;  // Party 1 runs decaps to generate the shared secret
    ///
    /// assert_eq!(ssk1, ssk2);  // Each party has the same shared secret
    /// # }
    /// # Ok(())}
    /// ```
    #[cfg(feature = "default-rng")]
    fn try_encaps(&self) -> Result<(Self::SharedSecretKey, Self::CipherText), &'static str> {
        self.try_encaps_with_rng(&mut OsRng)
    }


    /// Generates a shared secret and ciphertext from an encapsulation key specific to this security parameter set. <br>
    /// This function utilizes a provided random number generator and is intended to operate in constant
    /// time.
    /// # Errors
    /// Returns an error when the random number generator fails or an internal error condition arises.
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(feature = "ml-kem-512")] {
    /// use rand_core::OsRng;
    /// use fips203::ml_kem_512;  // Could also be ml_kem_768 or ml_kem_1024.
    /// use fips203::traits::{KeyGen, SerDes, Decaps, Encaps};
    ///
    /// let (ek1, dk1) = ml_kem_512::KG::try_keygen_with_rng(&mut OsRng)?;  // Party 1 generates both encaps and decaps keys
    /// let ek1_bytes = ek1.into_bytes();  // Party 1 serializes the encaps key
    ///
    /// let ek2_bytes = ek1_bytes;  // Party 1 sends encaps bytes to party 2
    ///
    /// let ek2 = ml_kem_512::EncapsKey::try_from_bytes(ek2_bytes)?;  // Party 2 deserializes the encaps key
    /// let (ssk2, ct2) = ek2.try_encaps_with_rng(&mut OsRng)?;  // Party 2 generates shared secret and ciphertext
    /// let ct2_bytes = ct2.into_bytes();  // Party 2 serializes the ciphertext
    ///
    /// let ct1_bytes = ct2_bytes;  // Party 2 sends the ciphertext to party 1
    ///
    /// let ct1 = ml_kem_512::CipherText::try_from_bytes(ct1_bytes)?;  // Party 1 deserializes the ciphertext
    /// let ssk1 = dk1.try_decaps(&ct1)?;  // Party 1 runs decaps to generate the shared secret
    ///
    /// assert_eq!(ssk1, ssk2);  // Each party has the same shared secret
    /// # }
    /// # Ok(())}
    /// ```
    fn try_encaps_with_rng(
        &self, rng: &mut impl CryptoRngCore,
    ) -> Result<(Self::SharedSecretKey, Self::CipherText), &'static str>;


    /// Generates a shared secret and ciphertext from an encapsulation key specific to this security parameter set. <br>
    /// This function utilizes a provided **seed** (rather than a random number generator) and is intended to operate in constant
    /// time.
    /// # Errors
    /// Returns an error when the random number generator fails or an internal error condition arises.
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(feature = "ml-kem-512")] {
    /// use rand_core::OsRng;
    /// use fips203::ml_kem_512;  // Could also be ml_kem_768 or ml_kem_1024.
    /// use fips203::traits::{KeyGen, SerDes, Decaps, Encaps};
    ///
    /// let (ek1, dk1) = ml_kem_512::KG::try_keygen_with_rng(&mut OsRng)?;  // Party 1 generates both encaps and decaps keys
    /// let ek1_bytes = ek1.into_bytes();  // Party 1 serializes the encaps key
    ///
    /// let ek2_bytes = ek1_bytes;  // Party 1 sends encaps bytes to party 2
    ///
    /// let ek2 = ml_kem_512::EncapsKey::try_from_bytes(ek2_bytes)?;  // Party 2 deserializes the encaps key
    /// let (ssk2, ct2) = ek2.encaps_from_seed(&[1u8; 32]);  // Party 2 generates shared secret and ciphertext
    /// let ct2_bytes = ct2.into_bytes();  // Party 2 serializes the ciphertext
    ///
    /// let ct1_bytes = ct2_bytes;  // Party 2 sends the ciphertext to party 1
    ///
    /// let ct1 = ml_kem_512::CipherText::try_from_bytes(ct1_bytes)?;  // Party 1 deserializes the ciphertext
    /// let ssk1 = dk1.try_decaps(&ct1)?;  // Party 1 runs decaps to generate the shared secret
    ///
    /// assert_eq!(ssk1, ssk2);  // Each party has the same shared secret
    /// # }
    /// # Ok(())}
    /// ```
    fn encaps_from_seed(&self, seed: &[u8; 32]) -> (Self::SharedSecretKey, Self::CipherText) {
        self.try_encaps_with_rng(&mut DummyRng { data: *seed }).expect("rng will not fail")
    }
}

/// Internal RNG implementation for deterministic operations.
/// This is used by the deterministic signing functions and will be refactored.
struct DummyRng {
    data: [u8; 32],
}

impl RngCore for DummyRng {
    fn next_u32(&mut self) -> u32 { unimplemented!() }

    fn next_u64(&mut self) -> u64 { unimplemented!() }

    fn fill_bytes(&mut self, _out: &mut [u8]) { unimplemented!() }

    fn try_fill_bytes(&mut self, out: &mut [u8]) -> Result<(), rand_core::Error> {
        out.copy_from_slice(&self.data);
        Ok(())
    }
}

impl CryptoRng for DummyRng {}

/// The `Decaps` trait defines methods for recovering the shared secret using
/// a decapsulation key and ciphertext.
pub trait Decaps {
    /// The ciphertext type received from the encapsulating party
    type CipherText;
    /// The shared secret key type generated during decapsulation
    type SharedSecretKey;


    /// Generates a shared secret from a decapsulation key and ciphertext specific to this security parameter set. <br>
    /// This function is intended to operate in constant-time.
    /// # Errors
    /// Returns an error if an internal error condition arises (e.g., an invalid `ct`).
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(feature = "ml-kem-512")] {
    /// use rand_core::OsRng;
    /// use fips203::ml_kem_512;  // Could also be ml_kem_768 or ml_kem_1024.
    /// use fips203::traits::{KeyGen, SerDes, Decaps, Encaps};
    ///
    /// let (ek1, dk1) = ml_kem_512::KG::try_keygen_with_rng(&mut OsRng)?;  // Party 1 generates both encaps and decaps keys
    /// let ek1_bytes = ek1.into_bytes();  // Party 1 serializes the encaps key
    ///
    /// let ek2_bytes = ek1_bytes;  // Party 1 sends encaps bytes to party 2
    ///
    /// let ek2 = ml_kem_512::EncapsKey::try_from_bytes(ek2_bytes)?;  // Party 2 deserializes the encaps key
    /// let (ssk2, ct2) = ek2.try_encaps_with_rng(&mut OsRng)?;  // Party 2 generates shared secret and ciphertext
    /// let ct2_bytes = ct2.into_bytes();  // Party 2 serializes the ciphertext
    ///
    /// let ct1_bytes = ct2_bytes;  // Party 2 sends the ciphertext to party 1
    ///
    /// let ct1 = ml_kem_512::CipherText::try_from_bytes(ct1_bytes)?; // Party 1 deserializes the ciphertext
    /// let ssk1 = dk1.try_decaps(&ct1)?;  // Party 1 runs decaps to generate the shared secret
    ///
    /// assert_eq!(ssk1, ssk2);  // Each party has the same shared secret
    /// # }
    /// # Ok(())}
    /// ```
    fn try_decaps(&self, ct: &Self::CipherText) -> Result<Self::SharedSecretKey, &'static str>;
}

/// The `SerDes` trait provides methods for serializing and deserializing cryptographic objects.
pub trait SerDes {
    /// The fixed-size byte array type used for serialization
    type ByteArray;


    /// Produces a byte array of fixed-size specific to the struct being serialized.
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(feature = "ml-kem-512")] {
    /// use rand_core::OsRng;
    /// use fips203::ml_kem_512;  // Could also be ml_kem_768 or ml_kem_1024.
    /// use fips203::traits::{KeyGen, SerDes, Decaps, Encaps};
    ///
    /// let (ek1, dk1) = ml_kem_512::KG::try_keygen_with_rng(&mut OsRng)?;  // Party 1 generates both encaps and decaps keys
    /// let ek1_bytes = ek1.into_bytes();  // Party 1 serializes the encaps key
    ///
    /// let ek2_bytes = ek1_bytes;  // Party 1 sends encaps bytes to party 2
    ///
    /// let ek2 = ml_kem_512::EncapsKey::try_from_bytes(ek2_bytes)?;  // Party 2 deserializes the encaps key
    /// let (ssk2, ct2) = ek2.try_encaps_with_rng(&mut OsRng)?;  // Party 2 generates shared secret and ciphertext
    /// let ct2_bytes = ct2.into_bytes();  // Party 2 serializes the ciphertext
    ///
    /// let ct1_bytes = ct2_bytes;  // Party 2 sends the ciphertext to party 1
    ///
    /// let ct1 = ml_kem_512::CipherText::try_from_bytes(ct1_bytes)?;  // Party 1 deserializes the ciphertext
    /// let ssk1 = dk1.try_decaps(&ct1)?;  // Party 1 runs decaps to generate the shared secret
    ///
    /// assert_eq!(ssk1, ssk2);  // Each party has the same shared secret
    /// # }
    /// # Ok(())}
    /// ```
    fn into_bytes(self) -> Self::ByteArray;


    /// Consumes a byte array of fixed-size specific to the struct being deserialized; performs validation
    /// # Errors
    /// Returns an error on malformed input.
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(feature = "ml-kem-512")] {
    /// use rand_core::OsRng;
    /// use fips203::ml_kem_512;  // Could also be ml_kem_768 or ml_kem_1024.
    /// use fips203::traits::{KeyGen, SerDes, Decaps, Encaps};
    ///
    /// let (ek1, dk1) = ml_kem_512::KG::try_keygen_with_rng(&mut OsRng)?;  // Party 1 generates both encaps and decaps keys
    /// let ek1_bytes = ek1.into_bytes();  // Party 1 serializes the encaps key
    ///
    /// let ek2_bytes = ek1_bytes;  // Party 1 sends encaps bytes to party 2
    ///
    /// let ek2 = ml_kem_512::EncapsKey::try_from_bytes(ek2_bytes)?;  // Party 2 deserializes the encaps key
    /// let (ssk2, ct2) = ek2.try_encaps_with_rng(&mut OsRng)?;  // Party 2 generates shared secret and ciphertext
    /// let ct2_bytes = ct2.into_bytes();  // Party 2 serializes the ciphertext
    ///
    /// let ct1_bytes = ct2_bytes;  // Party 2 sends the ciphertext to party 1
    ///
    /// let ct1 = ml_kem_512::CipherText::try_from_bytes(ct1_bytes)?;  // Party 1 deserializes the ciphertext
    /// let ssk1 = dk1.try_decaps(&ct1)?;  // Party 1 runs decaps to generate the shared secret
    ///
    /// assert_eq!(ssk1, ssk2);  // Each party has the same shared secret
    /// # }
    /// # Ok(())}
    /// ```
    fn try_from_bytes(ba: Self::ByteArray) -> Result<Self, &'static str>
    where
        Self: Sized;
}
