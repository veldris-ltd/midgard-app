//! Wire helpers the network connectors share: hex and base64, PBKDF2-HMAC-SHA256, and
//! a SCRAM-SHA-256 client (RFC 5802, RFC 7677). Hashing and HMAC come from
//! `pqc-agility`'s known-answer-tested wrappers over the vetted RustCrypto crates;
//! nothing here is a new primitive, only the constructions the source protocols
//! require. [FR-F-2] [NFR-SEC-1] [MG-SEC-10]

use pqc_agility::hash::{self, ct_eq, hmac_sha256, sha256};

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard, padded base64 (RFC 4648 §4).
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = (u32::from(chunk[0]) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        let sextet = |shift: u32| char::from(BASE64[((n >> shift) & 63) as usize]);
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
    out
}

/// Decode standard, padded base64; `None` for anything else.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 != 0 {
        return None;
    }
    let chunks = bytes.len() / 4;
    let mut out = Vec::with_capacity(chunks * 3);
    for (i, chunk) in bytes.chunks(4).enumerate() {
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && i + 1 != chunks) {
            return None;
        }
        let mut n = 0u32;
        for &c in &chunk[..4 - pad] {
            let value = BASE64.iter().position(|&x| x == c)?;
            n = (n << 6) | value as u32;
        }
        n <<= 6 * pad as u32;
        let decoded = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&decoded[..3 - pad]);
    }
    Some(out)
}

/// PBKDF2 with HMAC-SHA256 (RFC 8018 §5.2), producing `len` bytes. Computed by the
/// vetted RustCrypto `pbkdf2` crate through `pqc-agility`'s hash module, not by a
/// local loop. [MG-SEC-10]
pub fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    hash::pbkdf2_hmac_sha256(password, salt, iterations, &mut out);
    out
}

/// The fewest PBKDF2 iterations a SCRAM server may ask for (RFC 7677 §4).
pub const SCRAM_MIN_ITERATIONS: u32 = 4096;

/// The client side of one SCRAM-SHA-256 exchange, without channel binding. It proves
/// knowledge of the password without sending it, and then requires the server to
/// prove it holds the same verifier — a server that cannot is refused.
#[derive(Debug)]
pub struct ScramSha256 {
    client_first_bare: String,
    nonce: String,
    expected_server_signature: Option<[u8; 32]>,
}

impl ScramSha256 {
    /// Start an exchange for `username` with a fresh, printable `nonce` (no commas).
    pub fn new(username: &str, nonce: &str) -> Self {
        let name = username.replace('=', "=3D").replace(',', "=2C");
        Self {
            client_first_bare: format!("n={name},r={nonce}"),
            nonce: nonce.to_string(),
            expected_server_signature: None,
        }
    }

    /// The client-first message.
    pub fn client_first(&self) -> String {
        format!("n,,{}", self.client_first_bare)
    }

    /// Answer the server-first message: the client-final message with the proof.
    pub fn client_final(&mut self, server_first: &str, password: &str) -> Result<String, String> {
        let (mut nonce, mut salt, mut iterations) = (None, None, None);
        for attribute in server_first.split(',') {
            match attribute.split_once('=') {
                Some(("r", v)) => nonce = Some(v),
                Some(("s", v)) => salt = Some(v),
                Some(("i", v)) => iterations = Some(v),
                Some(("m", _)) => {
                    return Err(
                        "the server requires a SCRAM extension this client does not support".into(),
                    )
                }
                _ => {}
            }
        }
        let nonce = nonce.ok_or("the server's SCRAM challenge has no nonce")?;
        if !nonce.starts_with(&self.nonce) || nonce.len() == self.nonce.len() {
            return Err("the server's SCRAM nonce does not extend the client's".into());
        }
        let salt = salt
            .and_then(base64_decode)
            .ok_or("the server's SCRAM salt is not base64")?;
        let iterations: u32 = iterations
            .and_then(|i| i.parse().ok())
            .ok_or("the server's SCRAM iteration count is not a number")?;
        if iterations < SCRAM_MIN_ITERATIONS {
            return Err(format!(
                "the server asks for {iterations} SCRAM iterations; at least \
                 {SCRAM_MIN_ITERATIONS} are required"
            ));
        }

        let salted = pbkdf2_hmac_sha256(password.as_bytes(), &salt, iterations, 32);
        let client_key = hmac_sha256(&salted, b"Client Key");
        let stored_key = sha256(&client_key);
        let without_proof = format!("c=biws,r={nonce}");
        let auth_message = format!("{},{server_first},{without_proof}", self.client_first_bare);
        let client_signature = hmac_sha256(&stored_key, auth_message.as_bytes());
        let proof: Vec<u8> = client_key
            .iter()
            .zip(client_signature.iter())
            .map(|(k, s)| k ^ s)
            .collect();
        let server_key = hmac_sha256(&salted, b"Server Key");
        self.expected_server_signature = Some(hmac_sha256(&server_key, auth_message.as_bytes()));
        Ok(format!("{without_proof},p={}", base64_encode(&proof)))
    }

    /// Check the server-final message: the server must prove it knows the verifier.
    pub fn verify_server_final(&self, server_final: &str) -> Result<(), String> {
        if let Some(error) = server_final.strip_prefix("e=") {
            return Err(format!("the server refused SCRAM authentication: {error}"));
        }
        let expected = self
            .expected_server_signature
            .ok_or("no SCRAM exchange is in progress")?;
        let signature = server_final
            .strip_prefix("v=")
            .and_then(base64_decode)
            .ok_or("the server's SCRAM final message has no signature")?;
        if ct_eq(&signature, &expected) {
            Ok(())
        } else {
            Err(
                "the server's SCRAM signature does not verify: it does not hold this password's \
                 verifier"
                    .into(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(plain.as_bytes()), encoded);
            assert_eq!(base64_decode(encoded).unwrap(), plain.as_bytes());
        }
        for bad in ["Zg", "Zg=a", "Z===", "Zg==Zg==", "Zm9*"] {
            assert!(base64_decode(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn pbkdf2_matches_known_vectors() {
        // RFC 7914 §11 and the widely published PBKDF2-HMAC-SHA256 vectors.
        assert_eq!(
            hex(&pbkdf2_hmac_sha256(b"password", b"salt", 1, 32)),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
        assert_eq!(
            hex(&pbkdf2_hmac_sha256(b"password", b"salt", 4096, 32)),
            "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a"
        );
        assert_eq!(
            hex(&pbkdf2_hmac_sha256(b"passwd", b"salt", 1, 64)),
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc\
             49ca9cccf179b645991664b39d77ef317c71b845b1e30bd509112041d3a19783"
        );
    }

    #[test]
    fn scram_matches_rfc_7677() {
        let mut scram = ScramSha256::new("user", "rOprNGfwEbeRWgbNEkqO");
        assert_eq!(scram.client_first(), "n,,n=user,r=rOprNGfwEbeRWgbNEkqO");
        let server_first =
            "r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096";
        assert_eq!(
            scram.client_final(server_first, "pencil").unwrap(),
            "c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,\
             p=dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ="
        );
        scram
            .verify_server_final("v=6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=")
            .unwrap();
        assert!(scram
            .verify_server_final("v=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
            .is_err());
        assert!(scram.verify_server_final("e=invalid-proof").is_err());
    }

    #[test]
    fn scram_refuses_a_weak_or_foreign_challenge() {
        let mut scram = ScramSha256::new("user", "abc");
        assert!(scram
            .client_final("r=xyzdef,s=c2FsdA==,i=4096", "p")
            .is_err());
        assert!(scram.client_final("r=abc,s=c2FsdA==,i=4096", "p").is_err());
        assert!(scram
            .client_final("r=abcdef,s=c2FsdA==,i=1024", "p")
            .is_err());
        assert!(scram.client_final("r=abcdef,s=!!,i=4096", "p").is_err());
        assert!(scram
            .client_final("m=ext,r=abcdef,s=c2FsdA==,i=4096", "p")
            .is_err());
    }
}
