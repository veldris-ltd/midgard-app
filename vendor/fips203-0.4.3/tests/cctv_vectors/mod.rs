use std::fs;
use std::io::Read;

use flate2::read::GzDecoder;
use hex::decode;
use regex::Regex;

#[cfg(feature = "ml-kem-1024")]
use fips203::ml_kem_1024;
#[cfg(feature = "ml-kem-512")]
use fips203::ml_kem_512;
#[cfg(feature = "ml-kem-768")]
use fips203::ml_kem_768;
use fips203::traits::{Decaps, Encaps, KeyGen, SerDes};

use super::TestRng;

// Note: test vectors are directly copied across from https://github.com/C2SP/CCTV/tree/fd8cecee5f7746d0c6b8c3f4530c8976d629cbfa
// This approach may improve in future..

// More work to do here
//  1. Simplify/refactor code (trait objects?)
//  2. Implement accumulator loop referenced by https://github.com/C2SP/CCTV/tree/main/ML-KEM#accumulated-pq-crystals-vectors
//  3. Utilize any/all vectors available across the web

#[allow(clippy::type_complexity)]
fn get_intermediate_vec(
    filename: &str,
) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let data = fs::read_to_string(filename).expect("Unable to read file");
    let z_regex = Regex::new(r"z = ([0-9a-fA-F]+)").unwrap();
    let z = decode(z_regex.captures(&data).unwrap().get(1).unwrap().as_str()).unwrap();
    let d_regex = Regex::new(r"d = ([0-9a-fA-F]+)").unwrap();
    let d = decode(d_regex.captures(&data).unwrap().get(1).unwrap().as_str()).unwrap();
    let ek_regex = Regex::new(r"ek = ([0-9a-fA-F]+)").unwrap();
    let ek_exp = decode(ek_regex.captures(&data).unwrap().get(1).unwrap().as_str()).unwrap();
    let dk_regex = Regex::new(r"dk = ([0-9a-fA-F]+)").unwrap();
    let dk_exp = decode(dk_regex.captures(&data).unwrap().get(1).unwrap().as_str()).unwrap();
    let m_regex = Regex::new(r"m = ([0-9a-fA-F]+)").unwrap();
    let m = decode(m_regex.captures(&data).unwrap().get(1).unwrap().as_str()).unwrap();
    let k_regex = Regex::new(r"K = ([0-9a-fA-F]+)").unwrap();
    let k_exp = decode(k_regex.captures(&data).unwrap().get(1).unwrap().as_str()).unwrap();
    let c_regex = Regex::new(r"c = ([0-9a-fA-F]+)").unwrap();
    let c_exp = decode(c_regex.captures(&data).unwrap().get(1).unwrap().as_str()).unwrap();

    (d, z, ek_exp, dk_exp, m, k_exp, c_exp)
}

#[ignore] // TODO: Fix for final FIPS 203
#[test]
#[cfg(feature = "ml-kem-512")]
pub fn test_intermediate_512() {
    let (d, z, ek_exp, dk_exp, m, k_exp, c_exp) =
        get_intermediate_vec("./tests/cctv_vectors/ML-KEM/intermediate/ML-KEM-512.txt");
    let mut rnd = TestRng::new();
    rnd.push(&m);
    rnd.push(&d);
    rnd.push(&z);
    let (ek_act, dk_act) = ml_kem_512::KG::try_keygen_with_rng(&mut rnd).unwrap();
    assert_eq!(ek_exp, ek_act.clone().into_bytes());
    assert_eq!(dk_exp, dk_act.clone().into_bytes());
    let (k1_act, c_act) = ek_act.try_encaps_with_rng(&mut rnd).unwrap();
    assert_eq!(k_exp, k1_act.clone().into_bytes());
    assert_eq!(c_exp, c_act.clone().into_bytes());
    let k2_act = dk_act.try_decaps(&c_act).unwrap();
    assert_eq!(k1_act, k2_act);
}

#[ignore] // TODO: Fix for final FIPS 203
#[test]
#[cfg(feature = "ml-kem-768")]
pub fn test_intermediate_768() {
    let (d, z, ek_exp, dk_exp, m, k_exp, c_exp) =
        get_intermediate_vec("./tests/cctv_vectors/ML-KEM/intermediate/ML-KEM-768.txt");
    let mut rnd = TestRng::new();
    rnd.push(&m);
    rnd.push(&d);
    rnd.push(&z);
    let (ek_act, dk_act) = ml_kem_768::KG::try_keygen_with_rng(&mut rnd).unwrap();
    assert_eq!(ek_exp, ek_act.clone().into_bytes());
    assert_eq!(dk_exp, dk_act.clone().into_bytes());
    let (k1_act, c_act) = ek_act.try_encaps_with_rng(&mut rnd).unwrap();
    assert_eq!(k_exp, k1_act.clone().into_bytes());
    assert_eq!(c_exp, c_act.clone().into_bytes());
    let k2_act = dk_act.try_decaps(&c_act).unwrap();
    assert_eq!(k1_act, k2_act);
}

#[ignore] // TODO: Fix for final FIPS 203
#[test]
#[cfg(feature = "ml-kem-1024")]
pub fn test_intermediate_1024() {
    let (d, z, ek_exp, dk_exp, m, k_exp, c_exp) =
        get_intermediate_vec("./tests/cctv_vectors/ML-KEM/intermediate/ML-KEM-1024.txt");
    let mut rnd = TestRng::new();
    rnd.push(&m);
    rnd.push(&d);
    rnd.push(&z);
    let (ek_act, dk_act) = ml_kem_1024::KG::try_keygen_with_rng(&mut rnd).unwrap();
    assert_eq!(ek_exp, ek_act.clone().into_bytes());
    assert_eq!(dk_exp, dk_act.clone().into_bytes());
    let (k1_act, c_act) = ek_act.try_encaps_with_rng(&mut rnd).unwrap();
    assert_eq!(k_exp, k1_act.clone().into_bytes());
    assert_eq!(c_exp, c_act.clone().into_bytes());
    let k2_act = dk_act.try_decaps(&c_act).unwrap();
    assert_eq!(k1_act, k2_act);
}

fn get_strcmp_vec(filename: &str) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let data = fs::read_to_string(filename).expect("Unable to read file");
    let dk_regex = Regex::new(r"dk = ([0-9a-fA-F]+)").unwrap();
    let dk_exp = decode(dk_regex.captures(&data).unwrap().get(1).unwrap().as_str()).unwrap();
    let k_regex = Regex::new(r"K = ([0-9a-fA-F]+)").unwrap();
    let k_exp = decode(k_regex.captures(&data).unwrap().get(1).unwrap().as_str()).unwrap();
    let c_regex = Regex::new(r"c = ([0-9a-fA-F]+)").unwrap();
    let c_exp = decode(c_regex.captures(&data).unwrap().get(1).unwrap().as_str()).unwrap();

    (dk_exp, k_exp, c_exp)
}

#[ignore] // TODO: Fix for final FIPS 203
#[test]
#[cfg(feature = "ml-kem-512")]
pub fn test_strcmp_512() {
    let (dk_exp, k_exp, c_exp) =
        get_strcmp_vec("./tests/cctv_vectors/ML-KEM/strcmp/ML-KEM-512.txt");
    let dk = ml_kem_512::DecapsKey::try_from_bytes(dk_exp.try_into().unwrap()).unwrap();
    let c = ml_kem_512::CipherText::try_from_bytes(c_exp.try_into().unwrap()).unwrap();
    let k_act = dk.try_decaps(&c).unwrap();
    assert_eq!(k_exp, k_act.into_bytes());
}

#[ignore] // TODO: Fix for final FIPS 203
#[test]
#[cfg(feature = "ml-kem-768")]
pub fn test_strcmp_768() {
    let (dk_exp, k_exp, c_exp) =
        get_strcmp_vec("./tests/cctv_vectors/ML-KEM/strcmp/ML-KEM-768.txt");
    let dk = ml_kem_768::DecapsKey::try_from_bytes(dk_exp.try_into().unwrap()).unwrap();
    let c = ml_kem_768::CipherText::try_from_bytes(c_exp.try_into().unwrap()).unwrap();
    let k_act = dk.try_decaps(&c).unwrap();
    assert_eq!(k_exp, k_act.into_bytes());
}

#[ignore] // TODO: Fix for final FIPS 203
#[test]
#[cfg(feature = "ml-kem-1024")]
pub fn test_strcmp_1024() {
    let (dk_exp, k_exp, c_exp) =
        get_strcmp_vec("./tests/cctv_vectors/ML-KEM/strcmp/ML-KEM-1024.txt");
    let dk = ml_kem_1024::DecapsKey::try_from_bytes(dk_exp.try_into().unwrap()).unwrap();
    let c = ml_kem_1024::CipherText::try_from_bytes(c_exp.try_into().unwrap()).unwrap();
    let k_act = dk.try_decaps(&c).unwrap();
    assert_eq!(k_exp, k_act.into_bytes());
}

#[ignore] // TODO: Fix for final FIPS 203
#[test]
#[cfg(feature = "ml-kem-512")]
pub fn test_unlucky_512() {
    let (d, z, ek_exp, dk_exp, m, k_exp, c_exp) =
        get_intermediate_vec("./tests/cctv_vectors/ML-KEM/unluckysample/ML-KEM-512.txt");
    let mut rnd = TestRng::new();
    rnd.push(&m);
    rnd.push(&d);
    rnd.push(&z);
    let (ek_act, dk_act) = ml_kem_512::KG::try_keygen_with_rng(&mut rnd).unwrap();
    assert_eq!(ek_exp, ek_act.clone().into_bytes());
    assert_eq!(dk_exp, dk_act.clone().into_bytes());
    let (k1_act, c_act) = ek_act.try_encaps_with_rng(&mut rnd).unwrap();
    assert_eq!(k_exp, k1_act.clone().into_bytes());
    assert_eq!(c_exp, c_act.clone().into_bytes());
    let k2_act = dk_act.try_decaps(&c_act).unwrap();
    assert_eq!(k1_act, k2_act);
}

#[ignore] // TODO: Fix for final FIPS 203
#[test]
#[cfg(feature = "ml-kem-768")]
pub fn test_unlucky_768() {
    let (d, z, ek_exp, dk_exp, m, k_exp, c_exp) =
        get_intermediate_vec("./tests/cctv_vectors/ML-KEM/unluckysample/ML-KEM-768.txt");
    let mut rnd = TestRng::new();
    rnd.push(&m);
    rnd.push(&d);
    rnd.push(&z);
    let (ek_act, dk_act) = ml_kem_768::KG::try_keygen_with_rng(&mut rnd).unwrap();
    assert_eq!(ek_exp, ek_act.clone().into_bytes());
    assert_eq!(dk_exp, dk_act.clone().into_bytes());
    let (k1_act, c_act) = ek_act.try_encaps_with_rng(&mut rnd).unwrap();
    assert_eq!(k_exp, k1_act.clone().into_bytes());
    assert_eq!(c_exp, c_act.clone().into_bytes());
    let k2_act = dk_act.try_decaps(&c_act).unwrap();
    assert_eq!(k1_act, k2_act);
}

#[ignore] // TODO: Fix for final FIPS 203
#[test]
#[cfg(feature = "ml-kem-1024")]
pub fn test_unlucky_1024() {
    let (d, z, ek_exp, dk_exp, m, k_exp, c_exp) =
        get_intermediate_vec("./tests/cctv_vectors/ML-KEM/unluckysample/ML-KEM-1024.txt");
    let mut rnd = TestRng::new();
    rnd.push(&m);
    rnd.push(&d);
    rnd.push(&z);
    let (ek_act, dk_act) = ml_kem_1024::KG::try_keygen_with_rng(&mut rnd).unwrap();
    assert_eq!(ek_exp, ek_act.clone().into_bytes());
    assert_eq!(dk_exp, dk_act.clone().into_bytes());
    let (k1_act, c_act) = ek_act.try_encaps_with_rng(&mut rnd).unwrap();
    assert_eq!(k_exp, k1_act.clone().into_bytes());
    assert_eq!(c_exp, c_act.clone().into_bytes());
    let k2_act = dk_act.try_decaps(&c_act).unwrap();
    assert_eq!(k1_act, k2_act);
}

#[ignore] // TODO: Fix for final FIPS 203
#[test]
#[cfg(feature = "ml-kem-512")]
fn test_modulus_512() {
    let gz = fs::read("./tests/cctv_vectors/ML-KEM/modulus/ML-KEM-512.txt.gz").unwrap();
    let mut d = GzDecoder::new(&gz[..]);
    let mut s = String::new();
    d.read_to_string(&mut s).unwrap();
    for line in s.lines() {
        let ek_bytes = decode(line).unwrap();
        let ek = ml_kem_512::EncapsKey::try_from_bytes(ek_bytes.try_into().unwrap());
        assert!(ek.is_err())
    }
}

#[ignore] // TODO: Fix for final FIPS 203
#[test]
#[cfg(feature = "ml-kem-768")]
fn test_modulus_768() {
    let gz = fs::read("./tests/cctv_vectors/ML-KEM/modulus/ML-KEM-768.txt.gz").unwrap();
    let mut d = GzDecoder::new(&gz[..]);
    let mut s = String::new();
    d.read_to_string(&mut s).unwrap();
    for line in s.lines() {
        let ek_bytes = decode(line).unwrap();
        let ek = ml_kem_768::EncapsKey::try_from_bytes(ek_bytes.try_into().unwrap());
        assert!(ek.is_err())
    }
}

#[ignore] // TODO: Fix for final FIPS 203
#[test]
#[cfg(feature = "ml-kem-1024")]
fn test_modulus_1024() {
    let gz = fs::read("./tests/cctv_vectors/ML-KEM/modulus/ML-KEM-1024.txt.gz").unwrap();
    let mut d = GzDecoder::new(&gz[..]);
    let mut s = String::new();
    d.read_to_string(&mut s).unwrap();
    for line in s.lines() {
        let ek_bytes = decode(line).unwrap();
        let ek = ml_kem_1024::EncapsKey::try_from_bytes(ek_bytes.try_into().unwrap());
        assert!(ek.is_err())
    }
}
