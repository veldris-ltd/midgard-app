/// Runs tests using data from exports posted by NIST in their ACVP-Server repository
/// ACVP: Automated Cryptographic Validation Protocol
///
/// Repo: https://github.com/usnistgov/ACVP-Server/
///
/// Test files:
///   - https://github.com/usnistgov/ACVP-Server/raw/master/gen-val/json-files/SLH-DSA-keyGen-FIPS205/internalProjection.json
///   - https://github.com/usnistgov/ACVP-Server/raw/master/gen-val/json-files/SLH-DSA-sigGen-FIPS205/internalProjection.json
///   - https://github.com/usnistgov/ACVP-Server/raw/master/gen-val/json-files/SLH-DSA-sigVer-FIPS205/internalProjection.json
use fips205::traits::{KeyGen, SerDes, Signer, Verifier};
use rand_core::{CryptoRng, RngCore};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::fs::File;
use std::panic;
// use fips205::slh_dsa_sha2_128f::_test_only_raw_sign;
// use fips205::slh_dsa_shake_192f::PrivateKey;

fn dehex<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let buf = String::deserialize(deserializer)?;
    hex::decode(buf).map_err(serde::de::Error::custom)
}

struct TestRng {
    data: Vec<Vec<u8>>,
}

impl RngCore for TestRng {
    fn next_u32(&mut self) -> u32 { unimplemented!() }

    fn next_u64(&mut self) -> u64 { unimplemented!() }

    fn fill_bytes(&mut self, out: &mut [u8]) {
        let x = self.data.pop().expect("TestRng problem");
        out.copy_from_slice(&x)
    }

    fn try_fill_bytes(&mut self, out: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(out);
        Ok(()) // panic on probs is OK
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyGenTest {
    tc_id: usize,
    #[serde(deserialize_with = "dehex")]
    sk_seed: Vec<u8>,
    #[serde(deserialize_with = "dehex")]
    sk_prf: Vec<u8>,
    #[serde(deserialize_with = "dehex")]
    pk_seed: Vec<u8>,
    #[serde(deserialize_with = "dehex")]
    sk: Vec<u8>,
    #[serde(deserialize_with = "dehex")]
    pk: Vec<u8>,
}

macro_rules! test_keygen {
    ($test_group: ident, $param_set: ident, $fail_count: ident) => {
        for test in $test_group["tests"].as_array().unwrap() {
            let test: KeyGenTest = serde_json::from_value(test.clone()).unwrap();
            print!("Testing key generation with {} test case id {}... ", $param_set, test.tc_id);

            // Pre-load RNG
            let mut rnd = TestRng::new();
            rnd.push(&test.pk_seed);
            rnd.push(&test.sk_prf);
            rnd.push(&test.sk_seed);

            // Generate key
            let (pk, sk) = KG::try_keygen_with_rng(&mut rnd).unwrap();

            // Check against known answers
            let pk_match = pk.into_bytes() == test.pk.as_slice();
            let sk_match = sk.into_bytes() == test.sk.as_slice();
            if pk_match && sk_match {
                println!("Passed.")
            } else {
                println!("Failed.");
                $fail_count += 1;
            }
        }
    };
}

#[test]
fn run_keygen_tests() {
    let mut fail_count = 0;
    let file = "tests/nist_acvp_vectors/SLH-DSA-keyGen-FIPS205/internalProjection.json";
    let keygen_kat_file = File::open(file).expect("Error opening json file");
    let kat_json: Value =
        serde_json::from_reader(keygen_kat_file).expect("Error parsing json file");
    assert_eq!(kat_json["algorithm"].as_str().unwrap(), "SLH-DSA");
    assert_eq!(kat_json["mode"].as_str().unwrap(), "keyGen");
    assert_eq!(kat_json["revision"].as_str().unwrap(), "FIPS205");
    for test_group in kat_json["testGroups"].as_array().unwrap() {
        let param_set = test_group["parameterSet"].as_str().unwrap();
        match param_set {
            "SLH-DSA-SHA2-128s" => {
                use fips205::slh_dsa_sha2_128s::KG;
                test_keygen!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHAKE-128s" => {
                use fips205::slh_dsa_shake_128s::KG;
                test_keygen!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHA2-128f" => {
                use fips205::slh_dsa_sha2_128f::KG;
                test_keygen!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHAKE-128f" => {
                use fips205::slh_dsa_shake_128f::KG;
                test_keygen!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHA2-192s" => {
                use fips205::slh_dsa_sha2_192s::KG;
                test_keygen!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHAKE-192s" => {
                use fips205::slh_dsa_shake_192s::KG;
                test_keygen!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHA2-192f" => {
                use fips205::slh_dsa_sha2_192f::KG;
                test_keygen!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHAKE-192f" => {
                use fips205::slh_dsa_shake_192f::KG;
                test_keygen!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHA2-256s" => {
                use fips205::slh_dsa_sha2_256s::KG;
                test_keygen!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHAKE-256s" => {
                use fips205::slh_dsa_shake_256s::KG;
                test_keygen!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHA2-256f" => {
                use fips205::slh_dsa_sha2_256f::KG;
                test_keygen!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHAKE-256f" => {
                use fips205::slh_dsa_shake_256f::KG;
                test_keygen!(test_group, param_set, fail_count);
            }
            _ => {
                println!("Unrecognized Parameter set in test file: {}", param_set);
            }
        }
    }
    assert_eq!(fail_count, 0);
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignTest {
    tc_id: usize,
    #[serde(deserialize_with = "dehex")]
    sk: Vec<u8>,
    #[serde(deserialize_with = "dehex", default)]
    additional_randomness: Vec<u8>,
    #[serde(deserialize_with = "dehex")]
    message: Vec<u8>,
    #[serde(deserialize_with = "dehex")]
    signature: Vec<u8>,
}

macro_rules! test_sign {
    ($test_group: ident, $param_set: ident, $deterministic:ident, $fail_count: ident) => {
        for test in $test_group["tests"].as_array().unwrap() {
            let test: SignTest = serde_json::from_value(test.clone()).unwrap();
            print!("Testing signing with {}, test case {}... ", $param_set, test.tc_id);

            // Load private key
            let sk = PrivateKey::try_from_bytes(
                test.sk.as_slice().try_into().expect("Wrong length private key"),
            )
            .expect("Unable to load private key");

            // Calculate signature
            let sig_exp = if $deterministic {
                //sk.try_sign(&test.message, false)
                let mut rnd = TestRng::new();
                sk._test_only_raw_sign(&mut rnd, &test.message, false)
                    .expect("Error signing message")
            } else {
                let mut rnd = TestRng::new();
                rnd.push(test.additional_randomness.as_slice());
                //                sk.try_sign_with_rng(&mut rnd, &test.message, true)
                sk._test_only_raw_sign(&mut rnd, &test.message, true)
                    .expect("Error signing message")
            };

            // Check against known answer
            if sig_exp == test.signature.as_slice() {
                println!("Passed.");
            } else {
                println!("Failed.");
                $fail_count += 1;
            }
        }
    };
}

#[test]
#[allow(deprecated)]
fn run_signing_tests() {
    let mut fail_count = 0;
    let file = "tests/nist_acvp_vectors/SLH-DSA-sigGen-FIPS205/internalProjection.json";
    let sign_kat_file = File::open(file).expect("Error opening json file");
    let kat_json: Value = serde_json::from_reader(sign_kat_file).expect("Error parsing json file");
    assert_eq!(kat_json["algorithm"].as_str().unwrap(), "SLH-DSA");
    assert_eq!(kat_json["mode"].as_str().unwrap(), "sigGen");
    assert_eq!(kat_json["revision"].as_str().unwrap(), "FIPS205");
    for test_group in kat_json["testGroups"].as_array().unwrap() {
        let param_set = test_group["parameterSet"].as_str().unwrap();
        let deterministic = test_group["deterministic"].as_bool().unwrap();
        match param_set {
            "SLH-DSA-SHA2-128s" => {
                use fips205::slh_dsa_sha2_128s::PrivateKey;
                test_sign!(test_group, param_set, deterministic, fail_count);
            }
            "SLH-DSA-SHAKE-128s" => {
                use fips205::slh_dsa_shake_128s::PrivateKey;
                test_sign!(test_group, param_set, deterministic, fail_count);
            }
            "SLH-DSA-SHA2-128f" => {
                use fips205::slh_dsa_sha2_128f::PrivateKey;
                test_sign!(test_group, param_set, deterministic, fail_count);
            }
            "SLH-DSA-SHAKE-128f" => {
                use fips205::slh_dsa_shake_128f::PrivateKey;
                test_sign!(test_group, param_set, deterministic, fail_count);
            }
            "SLH-DSA-SHA2-192s" => {
                use fips205::slh_dsa_sha2_192s::PrivateKey;
                test_sign!(test_group, param_set, deterministic, fail_count);
            }
            "SLH-DSA-SHAKE-192s" => {
                use fips205::slh_dsa_shake_192s::PrivateKey;
                test_sign!(test_group, param_set, deterministic, fail_count);
            }
            "SLH-DSA-SHA2-192f" => {
                use fips205::slh_dsa_sha2_192f::PrivateKey;
                test_sign!(test_group, param_set, deterministic, fail_count);
            }
            "SLH-DSA-SHAKE-192f" => {
                use fips205::slh_dsa_shake_192f::PrivateKey;
                test_sign!(test_group, param_set, deterministic, fail_count);
            }
            "SLH-DSA-SHA2-256s" => {
                use fips205::slh_dsa_sha2_256s::PrivateKey;
                test_sign!(test_group, param_set, deterministic, fail_count);
            }
            "SLH-DSA-SHAKE-256s" => {
                use fips205::slh_dsa_shake_256s::PrivateKey;
                test_sign!(test_group, param_set, deterministic, fail_count);
            }
            "SLH-DSA-SHA2-256f" => {
                use fips205::slh_dsa_sha2_256f::PrivateKey;
                test_sign!(test_group, param_set, deterministic, fail_count);
            }
            "SLH-DSA-SHAKE-256f" => {
                use fips205::slh_dsa_shake_256f::PrivateKey;
                test_sign!(test_group, param_set, deterministic, fail_count);
            }
            _ => {
                println!("Unrecognized Parameter set in test file: {}", param_set);
            }
        }
    }
    assert_eq!(fail_count, 0);
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerifyTest {
    tc_id: usize,
    test_passed: bool,
    #[serde(deserialize_with = "dehex")]
    pk: Vec<u8>,
    #[serde(deserialize_with = "dehex")]
    message: Vec<u8>,
    #[serde(deserialize_with = "dehex")]
    signature: Vec<u8>,
    reason: String,
}

macro_rules! test_verify {
    ($test_group: ident, $param_set: ident, $fail_count: ident) => {
        for test in $test_group["tests"].as_array().unwrap() {
            let test: VerifyTest = serde_json::from_value(test.clone()).unwrap();
            print!("Testing signing with {}, test case {}... ", $param_set, test.tc_id,);

            if test.test_passed == false {
                println!("\nExpecting failed validation for: {}", test.reason);
            }

            let is_valid: Result<bool, _> = panic::catch_unwind(|| {
                // Load public key
                let pk = PublicKey::try_from_bytes(
                    test.pk.as_slice().try_into().expect("Wrong length public key"),
                )
                .expect("Unable to load public key");

                // Verify signature
                pk._test_only_raw_verify(
                    test.message.as_slice(),
                    test.signature.as_slice().try_into().expect("Signature length incorrect"),
                )
                .expect("Verification failed")
            });

            // Check against known answer
            let is_valid = match is_valid {
                Ok(true) => true,
                Ok(false) => false,
                Err(_) => false,
            };
            if is_valid == test.test_passed {
                println!("Passed.");
            } else {
                println!("Failed.");
                $fail_count += 1;
            };
        }
    };
}

#[test]
#[allow(deprecated)]
fn run_verification_tests() {
    let mut fail_count = 0;
    let file = "tests/nist_acvp_vectors/SLH-DSA-sigVer-FIPS205/internalProjection.json";
    let sign_kat_file = File::open(file).expect("Error opening json file");
    let kat_json: Value = serde_json::from_reader(sign_kat_file).expect("Error parsing json file");
    assert_eq!(kat_json["algorithm"].as_str().unwrap(), "SLH-DSA");
    assert_eq!(kat_json["mode"].as_str().unwrap(), "sigVer");
    assert_eq!(kat_json["revision"].as_str().unwrap(), "FIPS205");
    for test_group in kat_json["testGroups"].as_array().unwrap() {
        let param_set = test_group["parameterSet"].as_str().unwrap();
        match param_set {
            "SLH-DSA-SHA2-128s" => {
                use fips205::slh_dsa_sha2_128s::PublicKey;
                test_verify!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHAKE-128s" => {
                use fips205::slh_dsa_shake_128s::PublicKey;
                test_verify!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHA2-128f" => {
                use fips205::slh_dsa_sha2_128f::PublicKey;
                test_verify!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHAKE-128f" => {
                use fips205::slh_dsa_shake_128f::PublicKey;
                test_verify!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHA2-192s" => {
                use fips205::slh_dsa_sha2_192s::PublicKey;
                test_verify!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHAKE-192s" => {
                use fips205::slh_dsa_shake_192s::PublicKey;
                test_verify!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHA2-192f" => {
                use fips205::slh_dsa_sha2_192f::PublicKey;
                test_verify!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHAKE-192f" => {
                use fips205::slh_dsa_shake_192f::PublicKey;
                test_verify!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHA2-256s" => {
                use fips205::slh_dsa_sha2_256s::PublicKey;
                test_verify!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHAKE-256s" => {
                use fips205::slh_dsa_shake_256s::PublicKey;
                test_verify!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHA2-256f" => {
                use fips205::slh_dsa_sha2_256f::PublicKey;
                test_verify!(test_group, param_set, fail_count);
            }
            "SLH-DSA-SHAKE-256f" => {
                use fips205::slh_dsa_shake_256f::PublicKey;
                test_verify!(test_group, param_set, fail_count);
            }
            _ => {
                println!("Unrecognized Parameter set in test file: {}", param_set);
            }
        }
    }
    assert_eq!(fail_count, 0);
}
