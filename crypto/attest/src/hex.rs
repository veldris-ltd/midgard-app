//! Lowercase hex. Three private copies of the encoder already exist in the tree
//! (`pqc-agility/src/hash.rs` tests, `mg-repro`, `mimir/staging/src/provenance.rs`);
//! this is the shared one for the supply-chain tools, and the only one with a
//! decoder.

/// Encode bytes as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    s
}

/// Decode lowercase or uppercase hex. Returns `None` on an odd length or any
/// non-hex byte — a malformed digest is a rejection, not a best effort.
pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let nibble = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    for pair in b.chunks(2) {
        out.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let bytes: Vec<u8> = (0u8..=255).collect();
        assert_eq!(unhex(&hex(&bytes)).unwrap(), bytes);
    }

    #[test]
    fn encodes_lowercase_fixed_width() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff]), "000fff");
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(unhex("abc").is_none(), "odd length");
        assert!(unhex("zz").is_none(), "non-hex");
        assert!(unhex("ab cd").is_none(), "embedded space");
    }
}
