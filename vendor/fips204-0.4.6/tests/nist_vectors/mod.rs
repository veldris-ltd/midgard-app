// This file applies the NIST ACVP vectors.
//   from: https://github.com/usnistgov/ACVP-Server/blob/65370b861b96efd30dfe0daae607bde26a78a5c8/gen-val/json-files/ML-DSA-keyGen-FIPS204/internalProjection.json
//   from: https://github.com/usnistgov/ACVP-Server/blob/65370b861b96efd30dfe0daae607bde26a78a5c8/gen-val/json-files/ML-DSA-sigGen-FIPS204/internalProjection.json
//   from: https://github.com/usnistgov/ACVP-Server/blob/65370b861b96efd30dfe0daae607bde26a78a5c8/gen-val/json-files/ML-DSA-sigVer-FIPS204/internalProjection.json


use hex::decode;
use rand_core::{CryptoRng, RngCore};
use serde_json::Value;
use std::fs;

#[cfg(feature = "ml-dsa-44")]
use fips204::ml_dsa_44;
#[cfg(feature = "ml-dsa-65")]
use fips204::ml_dsa_65;
#[cfg(feature = "ml-dsa-87")]
use fips204::ml_dsa_87;

use fips204::traits::SerDes;


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
        fs::read_to_string("./tests/nist_vectors/ML-DSA-keyGen-FIPS204/internalProjection.json")
            .expect("Unable to read file");
    let v: Value = serde_json::from_str(&vectors).unwrap();

    for test_group in v["testGroups"].as_array().unwrap().iter() {
        for test in test_group["tests"].as_array().unwrap().iter() {
            let seed = decode(test["seed"].as_str().unwrap()).unwrap();
            let pk_exp = decode(test["pk"].as_str().unwrap()).unwrap();
            let sk_exp = decode(test["sk"].as_str().unwrap()).unwrap();
            let mut rnd = TestRng::new();
            rnd.push(&seed);

            #[cfg(feature = "ml-dsa-44")]
            if test_group["parameterSet"] == "ML-DSA-44" {
                let (pk_act, sk_act) = ml_dsa_44::try_keygen_with_rng(&mut rnd).unwrap();
                assert_eq!(pk_exp, pk_act.into_bytes());
                assert_eq!(sk_exp, sk_act.into_bytes());
            }

            #[cfg(feature = "ml-dsa-65")]
            if test_group["parameterSet"] == "ML-DSA-65" {
                let (pk_act, sk_act) = ml_dsa_65::try_keygen_with_rng(&mut rnd).unwrap();
                assert_eq!(pk_exp, pk_act.into_bytes());
                assert_eq!(sk_exp, sk_act.into_bytes());
            }

            #[cfg(feature = "ml-dsa-87")]
            if test_group["parameterSet"] == "ML-DSA-87" {
                let (pk_act, sk_act) = ml_dsa_87::try_keygen_with_rng(&mut rnd).unwrap();
                assert_eq!(pk_exp, pk_act.into_bytes());
                assert_eq!(sk_exp, sk_act.into_bytes());
            }
        }
    }
}

#[test]
fn test_siggen() {
    let vectors =
        fs::read_to_string("./tests/nist_vectors/ML-DSA-sigGen-FIPS204/internalProjection.json")
            .expect("Unable to read file");
    let v: Value = serde_json::from_str(&vectors).unwrap();

    #[allow(clippy::unnecessary_unwrap, deprecated)]
    for test_group in v["testGroups"].as_array().unwrap().iter() {
        for test in test_group["tests"].as_array().unwrap().iter() {
            let sk_bytes = decode(test["sk"].as_str().unwrap()).unwrap();
            let message = decode(test["message"].as_str().unwrap()).unwrap();
            let sig_exp = decode(test["signature"].as_str().unwrap()).unwrap();
            let seed = test["rnd"].as_str();
            let seed = if seed.is_none() {
                [0u8; 32]
            } else {
                decode(seed.unwrap()).unwrap().try_into().unwrap()
            };
            let mut rnd = TestRng::new();
            rnd.push(&seed);

            #[cfg(feature = "ml-dsa-44")]
            if test_group["parameterSet"] == "ML-DSA-44" {
                let sk =
                    ml_dsa_44::PrivateKey::try_from_bytes(sk_bytes.clone().try_into().unwrap())
                        .unwrap();
                //let sig_act = sk.try_sign_with_rng(&mut rnd, &message, &[]).unwrap();
                let sig_act = ml_dsa_44::_internal_sign(&sk, &message, &[], seed).unwrap();
                assert_eq!(sig_exp, sig_act);
            }

            #[cfg(feature = "ml-dsa-65")]
            if test_group["parameterSet"] == "ML-DSA-65" {
                let sk =
                    ml_dsa_65::PrivateKey::try_from_bytes(sk_bytes.clone().try_into().unwrap())
                        .unwrap();
                //let sig_act = sk.try_sign_with_rng(&mut rnd, &message, &[]).unwrap();
                let sig_act = ml_dsa_65::_internal_sign(&sk, &message, &[], seed).unwrap();
                assert_eq!(sig_exp, sig_act);
            }

            #[cfg(feature = "ml-dsa-87")]
            if test_group["parameterSet"] == "ML-DSA-87" {
                let sk =
                    ml_dsa_87::PrivateKey::try_from_bytes(sk_bytes.try_into().unwrap()).unwrap();
                //let sig_act = sk.try_sign_with_rng(&mut rnd, &message, &[]).unwrap();
                let sig_act = ml_dsa_87::_internal_sign(&sk, &message, &[], seed).unwrap();
                assert_eq!(sig_exp, sig_act);
            }
        }
    }
}

#[test]
fn test_sigver() {
    let vectors =
        fs::read_to_string("./tests/nist_vectors/ML-DSA-sigVer-FIPS204/internalProjection.json")
            .expect("Unable to read file");
    let v: Value = serde_json::from_str(&vectors).unwrap();

    #[allow(deprecated)]
    for test_group in v["testGroups"].as_array().unwrap().iter() {
        let pk_bytes = decode(test_group["pk"].as_str().unwrap()).unwrap();
        for test in test_group["tests"].as_array().unwrap().iter() {
            let message = decode(test["message"].as_str().unwrap()).unwrap();
            let signature = decode(test["signature"].as_str().unwrap()).unwrap();
            let test_passed = test["testPassed"].as_bool().unwrap();

            #[cfg(feature = "ml-dsa-44")]
            if test_group["parameterSet"] == "ML-DSA-44" {
                let pk = ml_dsa_44::PublicKey::try_from_bytes(pk_bytes.clone().try_into().unwrap())
                    .unwrap();
                let res = ml_dsa_44::_internal_verify(
                    &pk,
                    &message,
                    &signature.clone().try_into().unwrap(),
                    &[],
                );
                assert_eq!(res, test_passed);
            }

            #[cfg(feature = "ml-dsa-65")]
            if test_group["parameterSet"] == "ML-DSA-65" {
                let pk = ml_dsa_65::PublicKey::try_from_bytes(pk_bytes.clone().try_into().unwrap())
                    .unwrap();
                let res = ml_dsa_65::_internal_verify(
                    &pk,
                    &message,
                    &signature.clone().try_into().unwrap(),
                    &[],
                );
                assert_eq!(res, test_passed);
            }

            #[cfg(feature = "ml-dsa-87")]
            if test_group["parameterSet"] == "ML-DSA-87" {
                let pk = ml_dsa_87::PublicKey::try_from_bytes(pk_bytes.clone().try_into().unwrap())
                    .unwrap();
                let res = ml_dsa_87::_internal_verify(
                    &pk,
                    &message,
                    &signature.clone().try_into().unwrap(),
                    &[],
                );
                assert_eq!(res, test_passed);
            }
        }
    }
}
