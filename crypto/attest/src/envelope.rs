//! The `.sig` file: a minimal, self-describing binary envelope carrying the
//! detached ML-DSA signature plus everything a verifier needs to select a key
//! and an algorithm without guessing.
//!
//! ```text
//! magic      6  b"MGSIG1"
//! version    1  u8, currently 1
//! alg        1  u8, SigId wire tag
//! label_len  4  u32 LE  ┐ the domain label the signature was made under, carried
//! label      n          ┘ explicitly so a domain change is a loud rejection
//! key_id_len 4  u32 LE  ┐ which trust anchor verifies this
//! key_id     n          ┘
//! sig_len    4  u32 LE  ┐ the detached signature
//! sig        n          ┘
//! ```
//!
//! Parsing is strict: bad magic, an unknown version, an unknown algorithm tag, a
//! truncated field, or trailing bytes are all rejections. Trailing bytes matter —
//! a permissive parser lets an attacker append data that some other tool reads.
//!
//! ## Not byte-reproducible, deliberately
//! ML-DSA signing is *hedged*: `FipsSig::sign` draws fresh randomness, so signing
//! the same preimage twice yields different (equally valid) signatures, and this
//! file therefore differs between runs. That is the safer construction — it
//! resists fault attacks that deterministic signing does not — and it costs
//! nothing, because artefact reproducibility is established by `mg-repro`
//! comparing binaries and by verification checking that the signature *verifies*,
//! never that it is byte-equal. The attestation, which is a pure function of its
//! inputs, *is* byte-reproducible. `[AC-RF-SC-1f]`

use pqc_agility::{SigId, Signature};

use crate::error::VerifyError;

/// File magic.
pub const MAGIC: &[u8; 6] = b"MGSIG1";
/// Envelope format version.
pub const VERSION: u8 = 1;

/// A parsed `.sig` envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigEnvelope {
    /// The domain label the signature was made under.
    pub domain: Vec<u8>,
    /// The signing key id, for trust-anchor selection.
    pub key_id: String,
    /// The detached signature, tagged with its algorithm.
    pub signature: Signature,
}

fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

/// A cursor that fails closed rather than panicking on a short read.
struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], VerifyError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or_else(|| VerifyError::MalformedEnvelope(format!("{what}: length overflows")))?;
        if end > self.b.len() {
            return Err(VerifyError::MalformedEnvelope(format!(
                "{what}: truncated (need {n} bytes at offset {}, have {})",
                self.at,
                self.b.len().saturating_sub(self.at)
            )));
        }
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn u8(&mut self, what: &str) -> Result<u8, VerifyError> {
        Ok(self.take(1, what)?[0])
    }

    fn len_prefixed(&mut self, what: &str) -> Result<&'a [u8], VerifyError> {
        let n = u32::from_le_bytes(
            self.take(4, what)?
                .try_into()
                .expect("take(4) yields exactly 4 bytes"),
        ) as usize;
        self.take(n, what)
    }
}

impl SigEnvelope {
    /// Serialise the envelope.
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        o.extend_from_slice(MAGIC);
        o.push(VERSION);
        o.push(self.signature.alg().as_u8());
        put_bytes(&mut o, &self.domain);
        put_bytes(&mut o, self.key_id.as_bytes());
        put_bytes(&mut o, self.signature.as_bytes());
        o
    }

    /// Parse an envelope, rejecting anything malformed.
    pub fn parse(bytes: &[u8]) -> Result<Self, VerifyError> {
        let mut c = Cursor { b: bytes, at: 0 };

        if c.take(MAGIC.len(), "magic")? != MAGIC {
            return Err(VerifyError::MalformedEnvelope(
                "bad magic — not an MGSIG1 envelope".into(),
            ));
        }
        let version = c.u8("version")?;
        if version != VERSION {
            return Err(VerifyError::MalformedEnvelope(format!(
                "unsupported envelope version {version} (this build understands {VERSION})"
            )));
        }
        let alg_tag = c.u8("algorithm")?;
        let alg = SigId::from_u8(alg_tag).ok_or_else(|| {
            VerifyError::MalformedEnvelope(format!("unknown algorithm tag {alg_tag}"))
        })?;
        let domain = c.len_prefixed("domain")?.to_vec();
        let key_id = String::from_utf8(c.len_prefixed("key id")?.to_vec())
            .map_err(|_| VerifyError::MalformedEnvelope("key id is not valid UTF-8".into()))?;
        let sig = c.len_prefixed("signature")?.to_vec();

        if c.at != bytes.len() {
            return Err(VerifyError::MalformedEnvelope(format!(
                "{} trailing byte(s) after the envelope",
                bytes.len() - c.at
            )));
        }
        Ok(Self {
            domain,
            key_id,
            signature: Signature::new(alg, sig),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preimage::DOMAIN_LABEL;

    fn sample() -> SigEnvelope {
        SigEnvelope {
            domain: DOMAIN_LABEL.to_vec(),
            key_id: "release-key-1".into(),
            signature: Signature::new(SigId::MlDsa87, vec![7u8; 64]),
        }
    }

    #[test]
    fn round_trips() {
        let e = sample();
        assert_eq!(SigEnvelope::parse(&e.encode()).unwrap(), e);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut b = sample().encode();
        b[0] = b'X';
        assert!(matches!(
            SigEnvelope::parse(&b),
            Err(VerifyError::MalformedEnvelope(_))
        ));
    }

    #[test]
    fn rejects_unknown_version() {
        let mut b = sample().encode();
        b[MAGIC.len()] = 99;
        assert!(matches!(
            SigEnvelope::parse(&b),
            Err(VerifyError::MalformedEnvelope(_))
        ));
    }

    #[test]
    fn rejects_unknown_algorithm_tag() {
        let mut b = sample().encode();
        b[MAGIC.len() + 1] = 200;
        assert!(matches!(
            SigEnvelope::parse(&b),
            Err(VerifyError::MalformedEnvelope(_))
        ));
    }

    #[test]
    fn rejects_truncation_at_every_length() {
        let full = sample().encode();
        for n in 0..full.len() {
            assert!(
                SigEnvelope::parse(&full[..n]).is_err(),
                "a {n}-byte prefix must not parse"
            );
        }
        assert!(SigEnvelope::parse(&full).is_ok());
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut b = sample().encode();
        b.push(0);
        assert!(matches!(
            SigEnvelope::parse(&b),
            Err(VerifyError::MalformedEnvelope(_))
        ));
    }

    #[test]
    fn rejects_an_absurd_length_prefix() {
        // A declared length far beyond the buffer must fail closed, not allocate.
        let mut b = sample().encode();
        let off = MAGIC.len() + 2;
        b[off..off + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            SigEnvelope::parse(&b),
            Err(VerifyError::MalformedEnvelope(_))
        ));
    }

    #[test]
    fn rejects_non_utf8_key_id() {
        let e = SigEnvelope {
            domain: DOMAIN_LABEL.to_vec(),
            key_id: "k".into(),
            signature: Signature::new(SigId::MlDsa87, vec![1u8; 8]),
        };
        let mut b = e.encode();
        // The key id is the second length-prefixed field; corrupt its single byte.
        let key_off = MAGIC.len() + 2 + 4 + DOMAIN_LABEL.len() + 4;
        b[key_off] = 0xff;
        assert!(matches!(
            SigEnvelope::parse(&b),
            Err(VerifyError::MalformedEnvelope(_))
        ));
    }
}
