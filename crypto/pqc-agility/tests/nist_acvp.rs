//! NIST ACVP known-answer vectors for the ML-KEM and ML-DSA implementations the
//! production backend ships, run offline. [MG-SEC-15]
//!
//! The vectors are the ACVP `internalProjection.json` files vendored with the
//! `fips203` and `fips204` crates (checked against `.cargo-checksum.json` by
//! `tools/ci/vendor-check.sh`), so this needs no network and no extra crates: a
//! small JSON reader below is enough for their shape. It runs every parameter
//! set the backend enables — ML-KEM-768/1024 key generation, encapsulation and
//! decapsulation; ML-DSA-44/65/87 key generation, signing and verification.
//! Bumping either crate changes the vendored path and fails this test until the
//! path is updated, so the vectors always match the code under test.
#![cfg(feature = "backend-fips")]
#![allow(deprecated)] // `_internal_sign` / `_internal_verify` are the ACVP entry points.

use std::path::PathBuf;

use fips203::traits::{Decaps, Encaps, KeyGen as KemKeyGen, SerDes as KemSerDes};
use fips203::{ml_kem_1024, ml_kem_768};
use fips204::traits::{KeyGen as DsaKeyGen, SerDes as DsaSerDes};
use fips204::{ml_dsa_44, ml_dsa_65, ml_dsa_87};

const FIPS203: &str = "fips203-0.4.3";
const FIPS204: &str = "fips204-0.4.6";

fn vectors(krate: &str, set: &str) -> Json {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor")
        .join(krate)
        .join("tests/nist_vectors")
        .join(set)
        .join("internalProjection.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (vendored vectors missing?)", path.display()));
    let (json, rest) = Json::parse(text.trim_start()).expect("vector file parses");
    assert!(
        rest.trim().is_empty(),
        "trailing data in {}",
        path.display()
    );
    json
}

/// Each `(group, test)` pair in an ACVP file, with a count so a vector file that
/// silently empties out fails rather than passing vacuously.
fn cases(v: &Json) -> Vec<(&Json, &Json)> {
    let out: Vec<_> = v["testGroups"]
        .array()
        .iter()
        .flat_map(|g| g["tests"].array().iter().map(move |t| (g, t)))
        .collect();
    assert!(!out.is_empty(), "no test cases");
    out
}

fn hex(j: &Json) -> Vec<u8> {
    let s = j.str();
    assert!(s.len() % 2 == 0, "odd-length hex");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn arr<const N: usize>(j: &Json) -> [u8; N] {
    hex(j)
        .try_into()
        .expect("vector field has the parameter set's length")
}

#[test]
fn ml_kem_key_generation_matches_acvp() {
    let v = vectors(FIPS203, "ML-KEM-keyGen-FIPS203");
    let mut ran = 0;
    for (g, t) in cases(&v) {
        let (d, z) = (arr::<32>(&t["d"]), arr::<32>(&t["z"]));
        let (ek, dk) = match g["parameterSet"].str() {
            "ML-KEM-768" => {
                let (ek, dk) = ml_kem_768::KG::keygen_from_seed(d, z);
                (ek.into_bytes().to_vec(), dk.into_bytes().to_vec())
            }
            "ML-KEM-1024" => {
                let (ek, dk) = ml_kem_1024::KG::keygen_from_seed(d, z);
                (ek.into_bytes().to_vec(), dk.into_bytes().to_vec())
            }
            _ => continue, // ML-KEM-512 is not offered.
        };
        assert_eq!(ek, hex(&t["ek"]), "tcId {}", t["tcId"].num());
        assert_eq!(dk, hex(&t["dk"]), "tcId {}", t["tcId"].num());
        ran += 1;
    }
    assert!(ran >= 2 * 25, "only {ran} ML-KEM keyGen vectors ran");
}

#[test]
fn ml_kem_encapsulation_and_decapsulation_match_acvp() {
    let v = vectors(FIPS203, "ML-KEM-encapDecap-FIPS203");
    let mut ran = 0;
    for (g, t) in cases(&v) {
        let set = g["parameterSet"].str();
        let (k, c) = match (g["function"].str(), set) {
            ("encapsulation", "ML-KEM-768") => {
                let ek = ml_kem_768::EncapsKey::try_from_bytes(arr(&t["ek"])).unwrap();
                let (k, c) = ek.encaps_from_seed(&arr(&t["m"]));
                (k.into_bytes().to_vec(), Some(c.into_bytes().to_vec()))
            }
            ("encapsulation", "ML-KEM-1024") => {
                let ek = ml_kem_1024::EncapsKey::try_from_bytes(arr(&t["ek"])).unwrap();
                let (k, c) = ek.encaps_from_seed(&arr(&t["m"]));
                (k.into_bytes().to_vec(), Some(c.into_bytes().to_vec()))
            }
            ("decapsulation", "ML-KEM-768") => {
                let dk = ml_kem_768::DecapsKey::try_from_bytes(arr(&g["dk"])).unwrap();
                let c = ml_kem_768::CipherText::try_from_bytes(arr(&t["c"])).unwrap();
                (dk.try_decaps(&c).unwrap().into_bytes().to_vec(), None)
            }
            ("decapsulation", "ML-KEM-1024") => {
                let dk = ml_kem_1024::DecapsKey::try_from_bytes(arr(&g["dk"])).unwrap();
                let c = ml_kem_1024::CipherText::try_from_bytes(arr(&t["c"])).unwrap();
                (dk.try_decaps(&c).unwrap().into_bytes().to_vec(), None)
            }
            _ => continue,
        };
        assert_eq!(k, hex(&t["k"]), "{set} tcId {}", t["tcId"].num());
        if let Some(c) = c {
            assert_eq!(c, hex(&t["c"]), "{set} tcId {}", t["tcId"].num());
        }
        ran += 1;
    }
    assert!(ran >= 40, "only {ran} ML-KEM encap/decap vectors ran");
}

#[test]
fn ml_dsa_key_generation_matches_acvp() {
    let v = vectors(FIPS204, "ML-DSA-keyGen-FIPS204");
    let mut ran = 0;
    for (g, t) in cases(&v) {
        let xi = arr::<32>(&t["seed"]);
        let (pk, sk) = match g["parameterSet"].str() {
            "ML-DSA-44" => {
                let (pk, sk) = ml_dsa_44::KG::keygen_from_seed(&xi);
                (pk.into_bytes().to_vec(), sk.into_bytes().to_vec())
            }
            "ML-DSA-65" => {
                let (pk, sk) = ml_dsa_65::KG::keygen_from_seed(&xi);
                (pk.into_bytes().to_vec(), sk.into_bytes().to_vec())
            }
            "ML-DSA-87" => {
                let (pk, sk) = ml_dsa_87::KG::keygen_from_seed(&xi);
                (pk.into_bytes().to_vec(), sk.into_bytes().to_vec())
            }
            other => panic!("unexpected parameter set {other}"),
        };
        assert_eq!(pk, hex(&t["pk"]), "tcId {}", t["tcId"].num());
        assert_eq!(sk, hex(&t["sk"]), "tcId {}", t["tcId"].num());
        ran += 1;
    }
    assert!(ran >= 75, "only {ran} ML-DSA keyGen vectors ran");
}

#[test]
fn ml_dsa_signing_matches_acvp() {
    let v = vectors(FIPS204, "ML-DSA-sigGen-FIPS204");
    let mut ran = 0;
    for (g, t) in cases(&v) {
        let msg = hex(&t["message"]);
        // Deterministic groups carry no `rnd`: FIPS 204 uses 32 zero bytes.
        let rnd = match t.get("rnd") {
            Some(r) => arr::<32>(r),
            None => [0u8; 32],
        };
        let sig = match g["parameterSet"].str() {
            "ML-DSA-44" => {
                let sk = ml_dsa_44::PrivateKey::try_from_bytes(arr(&t["sk"])).unwrap();
                ml_dsa_44::_internal_sign(&sk, &msg, &[], rnd)
                    .unwrap()
                    .to_vec()
            }
            "ML-DSA-65" => {
                let sk = ml_dsa_65::PrivateKey::try_from_bytes(arr(&t["sk"])).unwrap();
                ml_dsa_65::_internal_sign(&sk, &msg, &[], rnd)
                    .unwrap()
                    .to_vec()
            }
            "ML-DSA-87" => {
                let sk = ml_dsa_87::PrivateKey::try_from_bytes(arr(&t["sk"])).unwrap();
                ml_dsa_87::_internal_sign(&sk, &msg, &[], rnd)
                    .unwrap()
                    .to_vec()
            }
            other => panic!("unexpected parameter set {other}"),
        };
        assert_eq!(sig, hex(&t["signature"]), "tcId {}", t["tcId"].num());
        ran += 1;
    }
    assert!(ran >= 60, "only {ran} ML-DSA sigGen vectors ran");
}

#[test]
fn ml_dsa_verification_matches_acvp_including_rejections() {
    let v = vectors(FIPS204, "ML-DSA-sigVer-FIPS204");
    let (mut ran, mut rejected) = (0, 0);
    for (g, t) in cases(&v) {
        let msg = hex(&t["message"]);
        let sig = hex(&t["signature"]);
        let ok = match g["parameterSet"].str() {
            "ML-DSA-44" => {
                let pk = ml_dsa_44::PublicKey::try_from_bytes(arr(&g["pk"])).unwrap();
                ml_dsa_44::_internal_verify(&pk, &msg, &sig.try_into().unwrap(), &[])
            }
            "ML-DSA-65" => {
                let pk = ml_dsa_65::PublicKey::try_from_bytes(arr(&g["pk"])).unwrap();
                ml_dsa_65::_internal_verify(&pk, &msg, &sig.try_into().unwrap(), &[])
            }
            "ML-DSA-87" => {
                let pk = ml_dsa_87::PublicKey::try_from_bytes(arr(&g["pk"])).unwrap();
                ml_dsa_87::_internal_verify(&pk, &msg, &sig.try_into().unwrap(), &[])
            }
            other => panic!("unexpected parameter set {other}"),
        };
        let expected = t["testPassed"].bool();
        assert_eq!(ok, expected, "tcId {}", t["tcId"].num());
        ran += 1;
        rejected += usize::from(!expected);
    }
    assert!(ran >= 45, "only {ran} ML-DSA sigVer vectors ran");
    assert!(rejected > 0, "the sigVer set exercises rejections");
}

// ---- a minimal JSON reader, enough for ACVP vector files ------------------

#[derive(Debug)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

static NULL: Json = Json::Null;

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn array(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            other => panic!("expected an array, got {other:?}"),
        }
    }
    fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            other => panic!("expected a string, got {other:?}"),
        }
    }
    fn num(&self) -> f64 {
        match self {
            Json::Num(n) => *n,
            _ => f64::NAN,
        }
    }
    fn bool(&self) -> bool {
        match self {
            Json::Bool(b) => *b,
            other => panic!("expected a bool, got {other:?}"),
        }
    }

    fn parse(s: &str) -> Option<(Json, &str)> {
        let s = s.trim_start();
        let c = s.chars().next()?;
        match c {
            '{' => {
                let mut rest = s[1..].trim_start();
                let mut fields = Vec::new();
                if let Some(r) = rest.strip_prefix('}') {
                    return Some((Json::Obj(fields), r));
                }
                loop {
                    let (key, r) = Json::parse(rest)?;
                    let r = r.trim_start().strip_prefix(':')?;
                    let (val, r) = Json::parse(r)?;
                    let Json::Str(key) = key else { return None };
                    fields.push((key, val));
                    let r = r.trim_start();
                    if let Some(r) = r.strip_prefix(',') {
                        rest = r;
                    } else {
                        return Some((Json::Obj(fields), r.strip_prefix('}')?));
                    }
                }
            }
            '[' => {
                let mut rest = s[1..].trim_start();
                let mut items = Vec::new();
                if let Some(r) = rest.strip_prefix(']') {
                    return Some((Json::Arr(items), r));
                }
                loop {
                    let (val, r) = Json::parse(rest)?;
                    items.push(val);
                    let r = r.trim_start();
                    if let Some(r) = r.strip_prefix(',') {
                        rest = r;
                    } else {
                        return Some((Json::Arr(items), r.strip_prefix(']')?));
                    }
                }
            }
            '"' => {
                // Vector files hold hex and identifiers only: no escapes needed.
                let end = s[1..].find('"')? + 1;
                let body = &s[1..end];
                assert!(!body.contains('\\'), "escaped strings are not expected");
                Some((Json::Str(body.to_string()), &s[end + 1..]))
            }
            't' => Some((Json::Bool(true), s.strip_prefix("true")?)),
            'f' => Some((Json::Bool(false), s.strip_prefix("false")?)),
            'n' => Some((Json::Null, s.strip_prefix("null")?)),
            _ => {
                let end = s
                    .find(|c: char| !(c.is_ascii_digit() || "+-.eE".contains(c)))
                    .unwrap_or(s.len());
                Some((Json::Num(s[..end].parse().ok()?), &s[end..]))
            }
        }
    }
}

impl std::ops::Index<&str> for Json {
    type Output = Json;
    fn index(&self, key: &str) -> &Json {
        self.get(key).unwrap_or(&NULL)
    }
}
