#[cfg(feature = "ml-kem-1024")]
use fips203::ml_kem_1024;
#[cfg(feature = "ml-kem-512")]
use fips203::ml_kem_512;
#[cfg(feature = "ml-kem-768")]
use fips203::ml_kem_768;
use fips203::traits::{Decaps, Encaps, KeyGen, SerDes};
use rand_chacha::rand_core::SeedableRng;


#[test]
#[cfg(feature = "ml-kem-512")]
fn test_expected_flow_512() {
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(123);
    for _i in 0..100 {
        // Alice runs KeyGen, and serializes ek for Bob (to bytes)
        let (alice_ek, alice_dk) = ml_kem_512::KG::try_keygen_with_rng(&mut rng).unwrap();
        let alice_ek_bytes = alice_ek.into_bytes();

        // Alice sends ek bytes to Bob
        let bob_ek_bytes = alice_ek_bytes;

        // Bob deserializes ek bytes, runs Encaps, to get ssk and serializes ct for Alice (to bytes)
        let bob_ek = ml_kem_512::EncapsKey::try_from_bytes(bob_ek_bytes).unwrap();
        let (bob_ssk_bytes, bob_ct) = bob_ek.try_encaps_with_rng(&mut rng).unwrap();
        let bob_ct_bytes = bob_ct.into_bytes();

        // Bob sends ct bytes to Alice
        let alice_ct_bytes = bob_ct_bytes;

        // Alice deserializes runs Decaps
        let alice_ct = ml_kem_512::CipherText::try_from_bytes(alice_ct_bytes).unwrap();
        let alice_ssk_bytes = alice_dk.try_decaps(&alice_ct).unwrap();

        // Alice and Bob now have the same shared secret key
        assert_eq!(bob_ssk_bytes, alice_ssk_bytes);

        // Double check the correspondence of the serialized keypair (we already have alice_ek)
        let alice_dk_bytes = alice_dk.into_bytes();
        assert!(ml_kem_512::KG::validate_keypair_with_rng_vartime(
            &mut rng,
            &alice_ek_bytes,
            &alice_dk_bytes
        ));
    }
}


#[test]
#[cfg(feature = "ml-kem-768")]
fn test_expected_flow_768() {
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(456);
    for _i in 0..100 {
        // Alice runs KeyGen, and serializes ek for Bob (to bytes)
        let (alice_ek, alice_dk) = ml_kem_768::KG::try_keygen_with_rng(&mut rng).unwrap();
        let alice_ek_bytes = alice_ek.into_bytes();

        // Alice sends ek bytes to Bob
        let bob_ek_bytes = alice_ek_bytes;

        // Bob deserializes ek bytes, runs Encaps, to get ssk and serializes ct for Alice (to bytes)
        let bob_ek = ml_kem_768::EncapsKey::try_from_bytes(bob_ek_bytes).unwrap();
        let (bob_ssk_bytes, bob_ct) = bob_ek.try_encaps_with_rng(&mut rng).unwrap();
        let bob_ct_bytes = bob_ct.into_bytes();

        // Bob sends ct bytes to Alice
        let alice_ct_bytes = bob_ct_bytes;

        // Alice deserializes runs Decaps
        let alice_ct = ml_kem_768::CipherText::try_from_bytes(alice_ct_bytes).unwrap();
        let alice_ssk_bytes = alice_dk.try_decaps(&alice_ct).unwrap();

        // Alice and Bob now have the same shared secret key
        assert_eq!(bob_ssk_bytes, alice_ssk_bytes);

        // Double check the correspondence of the serialized keypair (we already have alice_ek)
        let alice_dk_bytes = alice_dk.into_bytes();
        assert!(ml_kem_768::KG::validate_keypair_with_rng_vartime(
            &mut rng,
            &alice_ek_bytes,
            &alice_dk_bytes
        ));
    }
}


#[test]
#[cfg(feature = "ml-kem-1024")]
fn test_expected_flow_1024() {
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(789);
    for _i in 0..100 {
        // Alice runs KeyGen, and serializes ek for Bob (to bytes)
        let (alice_ek, alice_dk) = ml_kem_1024::KG::try_keygen_with_rng(&mut rng).unwrap();
        let alice_ek_bytes = alice_ek.into_bytes();

        // Alice sends ek bytes to Bob
        let bob_ek_bytes = alice_ek_bytes;

        // Bob deserializes ek bytes, runs Encaps, to get ssk and serializes ct for Alice (to bytes)
        let bob_ek = ml_kem_1024::EncapsKey::try_from_bytes(bob_ek_bytes).unwrap();
        let (bob_ssk_bytes, bob_ct) = bob_ek.try_encaps_with_rng(&mut rng).unwrap();
        let bob_ct_bytes = bob_ct.into_bytes();

        // Bob sends ct bytes to Alice
        let alice_ct_bytes = bob_ct_bytes;

        // Alice deserializes runs Decaps
        let alice_ct = ml_kem_1024::CipherText::try_from_bytes(alice_ct_bytes).unwrap();
        let alice_ssk_bytes = alice_dk.try_decaps(&alice_ct).unwrap();

        // Alice and Bob now have the same shared secret key
        assert_eq!(bob_ssk_bytes, alice_ssk_bytes);

        // Double check the correspondence of the serialized keypair (we already have alice_ek)
        let alice_dk_bytes = alice_dk.into_bytes();
        assert!(ml_kem_1024::KG::validate_keypair_with_rng_vartime(
            &mut rng,
            &alice_ek_bytes,
            &alice_dk_bytes
        ));
    }
}


// $ cargo test -- --ignored
#[ignore]
#[test]
#[cfg(feature = "ml-kem-512")]
fn test_forever() {
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(123);
    for i in 0..u64::MAX {
        // Alice runs KeyGen, and serializes ek for Bob (to bytes)
        let (alice_ek, alice_dk) = ml_kem_512::KG::try_keygen_with_rng(&mut rng).unwrap();
        let alice_ek_bytes = alice_ek.into_bytes();

        // Alice sends ek bytes to Bob
        let bob_ek_bytes = alice_ek_bytes;

        // Bob deserializes ek bytes, runs Encaps, to get ssk and serializes ct for Alice (to bytes)
        let bob_ek = ml_kem_512::EncapsKey::try_from_bytes(bob_ek_bytes).unwrap();
        let (bob_ssk_bytes, bob_ct) = bob_ek.try_encaps_with_rng(&mut rng).unwrap();
        let bob_ct_bytes = bob_ct.into_bytes();

        // Bob sends ct bytes to Alice
        let alice_ct_bytes = bob_ct_bytes;

        // Alice deserializes runs Decaps
        let alice_ct = ml_kem_512::CipherText::try_from_bytes(alice_ct_bytes).unwrap();
        let alice_ssk_bytes = alice_dk.try_decaps(&alice_ct).unwrap();

        // Alice and Bob now have the same shared secret key
        assert_eq!(bob_ssk_bytes, alice_ssk_bytes, "shared secret failed on iteration {}", i);

        // Double check the correspondence of the serialized keypair (we already have alice_ek)
        let alice_dk_bytes = alice_dk.into_bytes();
        assert!(ml_kem_512::KG::validate_keypair_with_rng_vartime(
            &mut rng,
            &alice_ek_bytes,
            &alice_dk_bytes
        ));
    }
}
