// This file implements the NIST ACVP vectors.
//   from: https://github.com/usnistgov/ACVP-Server/blob/65370b861b96efd30dfe0daae607bde26a78a5c8/gen-val/json-files/ML-KEM-keyGen-FIPS203/internalProjection.json
//   from: https://github.com/usnistgov/ACVP-Server/blob/65370b861b96efd30dfe0daae607bde26a78a5c8/gen-val/json-files/ML-KEM-encapDecap-FIPS203/internalProjection.json

use hex::decode;
use rand_core::{CryptoRng, RngCore};
use serde_json::Value;
use std::fs;

#[cfg(feature = "ml-kem-1024")]
use fips203::ml_kem_1024;
#[cfg(feature = "ml-kem-512")]
use fips203::ml_kem_512;
#[cfg(feature = "ml-kem-768")]
use fips203::ml_kem_768;

use fips203::traits::{Decaps, Encaps, KeyGen, SerDes};


// ----- CUSTOM RNG TO REPLAY VALUES -----
struct TestRng {
    data: Vec<Vec<u8>>,
}

impl RngCore for TestRng {
    fn next_u32(&mut self) -> u32 { unimplemented!() }

    fn next_u64(&mut self) -> u64 { unimplemented!() }

    fn fill_bytes(&mut self, out: &mut [u8]) {
        let x = self.data.pop().expect("test rng problem");
        out.copy_from_slice(&x)
    }

    fn try_fill_bytes(&mut self, out: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(out);
        Ok(())
    }
}

impl CryptoRng for TestRng {}

impl TestRng {
    fn new() -> Self { TestRng { data: Vec::new() } }

    fn push(&mut self, new_data: &[u8]) {
        let x = new_data.to_vec();
        self.data.push(x);
    }
}


#[test]
fn test_keygen() {
    let vectors =
        fs::read_to_string("./tests/nist_vectors/ML-KEM-keyGen-FIPS203/internalProjection.json")
            .expect("Unable to read file");
    let v: Value = serde_json::from_str(&vectors).unwrap();

    for test_group in v["testGroups"].as_array().unwrap().iter() {
        for test in test_group["tests"].as_array().unwrap().iter() {
            let z = decode(test["z"].as_str().unwrap()).unwrap();
            let d = decode(test["d"].as_str().unwrap()).unwrap();
            let ek_exp = decode(test["ek"].as_str().unwrap()).unwrap();
            let dk_exp = decode(test["dk"].as_str().unwrap()).unwrap();
            let mut rnd = TestRng::new();
            rnd.push(&z);
            rnd.push(&d);

            #[cfg(feature = "ml-kem-512")]
            if test_group["parameterSet"] == "ML-KEM-512" {
                // Following line picks up seed API
                let (ek_act, dk_act) =
                    ml_kem_512::KG::keygen_from_seed(d.try_into().unwrap(), z.try_into().unwrap());
                assert_eq!(ek_exp, ek_act.into_bytes());
                assert_eq!(dk_exp, dk_act.into_bytes());
            }
            #[cfg(feature = "ml-kem-768")]
            if test_group["parameterSet"] == "ML-KEM-768" {
                let (ek_act, dk_act) = ml_kem_768::KG::try_keygen_with_rng(&mut rnd).unwrap();
                assert_eq!(ek_exp, ek_act.into_bytes());
                assert_eq!(dk_exp, dk_act.into_bytes());
            }
            #[cfg(feature = "ml-kem-1024")]
            if test_group["parameterSet"] == "ML-KEM-1024" {
                let (ek_act, dk_act) = ml_kem_1024::KG::try_keygen_with_rng(&mut rnd).unwrap();
                assert_eq!(ek_exp, ek_act.into_bytes());
                assert_eq!(dk_exp, dk_act.into_bytes());
            }
        }
    }
}


#[test]
fn test_encaps() {
    let vectors = fs::read_to_string(
        "./tests/nist_vectors/ML-KEM-encapDecap-FIPS203/internalProjection.json",
    )
    .expect("Unable to read file");
    let v: Value = serde_json::from_str(&vectors).unwrap();

    for test_group in v["testGroups"].as_array().unwrap().iter() {
        if test_group["function"] == "encapsulation" {
            let parameter_set = &test_group["parameterSet"];
            for test in test_group["tests"].as_array().unwrap().iter() {
                let ek = decode(test["ek"].as_str().unwrap()).unwrap();
                let m = decode(test["m"].as_str().unwrap()).unwrap();
                let ct_exp = decode(test["c"].as_str().unwrap()).unwrap();
                let ssk_exp = decode(test["k"].as_str().unwrap()).unwrap();
                let mut rnd = TestRng::new();
                rnd.push(&m);

                #[cfg(feature = "ml-kem-512")]
                if parameter_set == "ML-KEM-512" {
                    let ek = ml_kem_512::EncapsKey::try_from_bytes(ek.clone().try_into().unwrap())
                        .unwrap();
                    let (ssk_act, ct_act) = ek.try_encaps_with_rng(&mut rnd).unwrap();
                    assert_eq!(ssk_exp, ssk_act.into_bytes());
                    assert_eq!(ct_exp, ct_act.into_bytes());
                }
                #[cfg(feature = "ml-kem-768")]
                if parameter_set == "ML-KEM-768" {
                    let ek = ml_kem_768::EncapsKey::try_from_bytes(ek.clone().try_into().unwrap())
                        .unwrap();
                    let (ssk_act, ct_act) = ek.try_encaps_with_rng(&mut rnd).unwrap();
                    assert_eq!(ssk_exp, ssk_act.into_bytes());
                    assert_eq!(ct_exp, ct_act.into_bytes());
                }
                #[cfg(feature = "ml-kem-1024")]
                if parameter_set == "ML-KEM-1024" {
                    let ek =
                        ml_kem_1024::EncapsKey::try_from_bytes(ek.try_into().unwrap()).unwrap();
                    let (ssk_act, ct_act) = ek.try_encaps_with_rng(&mut rnd).unwrap();
                    assert_eq!(ssk_exp, ssk_act.into_bytes());
                    assert_eq!(ct_exp, ct_act.into_bytes());
                }
            }
        }
    }
}


#[test]
fn test_decaps() {
    let vectors = fs::read_to_string(
        "./tests/nist_vectors/ML-KEM-encapDecap-FIPS203/internalProjection.json",
    )
    .expect("Unable to read file");
    let v: Value = serde_json::from_str(&vectors).unwrap();

    for test_group in v["testGroups"].as_array().unwrap().iter() {
        if test_group["function"] == "decapsulation" {
            let parameter_set = &test_group["parameterSet"];
            let dk = decode(test_group["dk"].as_str().unwrap()).unwrap();
            for test in test_group["tests"].as_array().unwrap().iter() {
                let c = decode(test["c"].as_str().unwrap()).unwrap();
                let k_exp = decode(test["k"].as_str().unwrap()).unwrap();

                #[cfg(feature = "ml-kem-512")]
                if parameter_set == "ML-KEM-512" {
                    let dk = ml_kem_512::DecapsKey::try_from_bytes(dk.clone().try_into().unwrap())
                        .unwrap();
                    let c = ml_kem_512::CipherText::try_from_bytes(c.clone().try_into().unwrap())
                        .unwrap();
                    let k_act = dk.try_decaps(&c).unwrap();
                    assert_eq!(k_exp, k_act.into_bytes());
                }
                #[cfg(feature = "ml-kem-768")]
                if parameter_set == "ML-KEM-768" {
                    let dk = ml_kem_768::DecapsKey::try_from_bytes(dk.clone().try_into().unwrap())
                        .unwrap();
                    let c = ml_kem_768::CipherText::try_from_bytes(c.clone().try_into().unwrap())
                        .unwrap();
                    let k_act = dk.try_decaps(&c).unwrap();
                    assert_eq!(k_exp, k_act.into_bytes());
                }
                #[cfg(feature = "ml-kem-1024")]
                if parameter_set == "ML-KEM-1024" {
                    let dk = ml_kem_1024::DecapsKey::try_from_bytes(dk.clone().try_into().unwrap())
                        .unwrap();
                    let c = ml_kem_1024::CipherText::try_from_bytes(c.clone().try_into().unwrap())
                        .unwrap();
                    let k_act = dk.try_decaps(&c).unwrap();
                    assert_eq!(k_exp, k_act.into_bytes());
                }
            }
        }
    }
}
