#[cfg(feature = "ml-kem-512")]
use fips203::ml_kem_512;
use fips203::traits::{KeyGen, SerDes};
use rand_chacha::rand_core::SeedableRng;
use rand_core::RngCore;

// Highlights potential validation opportunities
#[test]
#[cfg(feature = "ml-kem-512")]
fn fails_512() {
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(123);
    for _i in 0..100 {
        let mut bad_ek_bytes = [0u8; ml_kem_512::EK_LEN];
        rng.fill_bytes(&mut bad_ek_bytes);
        let bad_ek = ml_kem_512::EncapsKey::try_from_bytes(bad_ek_bytes);
        assert!(bad_ek.is_err());

        let mut bad_ct_bytes = [0u8; ml_kem_512::CT_LEN];
        rng.fill_bytes(&mut bad_ct_bytes);
        let _bad_ct = ml_kem_512::CipherText::try_from_bytes(bad_ct_bytes);
        // Note: FIPS 203 validation per page 31 only puts size constraints on the ciphertext.
        // A Result is used to allow for future expansion of validation...
        // assert!(bad_ct.is_err());

        let mut bad_dk_bytes = [0u8; ml_kem_512::DK_LEN];
        rng.fill_bytes(&mut bad_dk_bytes);
        let bad_dk = ml_kem_512::DecapsKey::try_from_bytes(bad_dk_bytes);
        // Note: FIPS 203 validation per page 31 only puts size constraints on the decaps key.
        // A Result is used to allow for future expansion of validation...
        assert!(bad_dk.is_err());

        // We can validate the non-correspondence of these serialized keypair
        assert!(!ml_kem_512::KG::validate_keypair_with_rng_vartime(
            &mut rng,
            &bad_ek_bytes,
            &bad_dk_bytes
        ));

        // let bad_ssk_bytes = bad_dk.unwrap().try_decaps(&bad_ct.unwrap());
        // assert!(bad_ssk_bytes.is_err());
    }
}
