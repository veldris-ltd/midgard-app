//! Standard base64 (RFC 4648 §4, padded), for artefact bytes inside JSON bodies.
//!
//! The registry routes carry JSON so a browser bridge can forward them unchanged; bytes
//! travel as base64 strings. Decoding is strict: no whitespace, no missing or misplaced
//! padding and no non-zero trailing bits, so one encoding means one byte string.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode `bytes` as padded standard base64.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn value(c: u8) -> Option<u32> {
    Some(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    } as u32)
}

/// Decode padded standard base64, refusing anything but its one canonical form.
pub fn decode(text: &str) -> Result<Vec<u8>, String> {
    let s = text.as_bytes();
    if s.len() % 4 != 0 {
        return Err("base64 length is not a multiple of 4".into());
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let quads = s.len() / 4;
    for (q, quad) in s.chunks(4).enumerate() {
        let pad = quad.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && q + 1 != quads) {
            return Err("base64 padding is misplaced".into());
        }
        let mut n = 0u32;
        for &c in &quad[..4 - pad] {
            n = (n << 6) | value(c).ok_or("base64 has a character outside its alphabet")?;
        }
        n <<= 6 * pad as u32;
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        let keep = 3 - pad;
        if bytes[keep..].iter().any(|&b| b != 0) {
            return Err("base64 has non-zero trailing bits".into());
        }
        out.extend_from_slice(&bytes[..keep]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_matches_rfc_4648_vectors() {
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(plain.as_bytes()), coded);
            assert_eq!(decode(coded).unwrap(), plain.as_bytes());
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&all)).unwrap(), all);
    }

    #[test]
    fn refuses_non_canonical_input() {
        for bad in [
            "Zg=", "Zg", "Zh==", "Z===", "Zg==Zg==", "Zm 9v", "Zm9v\n", "Zm9*",
        ] {
            assert!(decode(bad).is_err(), "{bad:?}");
        }
    }
}
