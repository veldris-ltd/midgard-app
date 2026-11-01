//! The pinned audit key: the one public key a ledger is checked with, taken from a
//! source independent of the keystore that signs it. [MG-SEC-05]
//!
//! Checking a ledger with the verifying key from its own signing keystore proves
//! nothing against whoever can replace that keystore: they sign a ledger of their
//! choosing with a key of their choosing and it "verifies". A [`PinnedKey`] is
//! supplied by the operator at verify time (a trust-anchor file kept away from the
//! keystore, a config value), and a verifier refuses any signer whose key is not the
//! pinned one ([`PinnedKey::check`]) and verifies only under the pin
//! ([`crate::verify::verify_ledger`]).
//!
//! The text form is the trust-anchor line `attest::Keystore::export_verifying_key`
//! writes, `<alg> <key-id> <hex>`, so `mg-admin init`'s `trust/audit.pub` can be
//! copied out as the pin unchanged.

use std::path::Path;

use pqc_agility::{SigId, VerifyingKey};

use crate::error::AuditError;

/// Every signature algorithm a pin may name, by its stable string id.
const PIN_ALGS: [SigId; 4] = [
    SigId::MlDsa44,
    SigId::MlDsa65,
    SigId::MlDsa87,
    SigId::SlhDsaSha2_128s,
];

/// The audit verifying key a ledger must verify under. [MG-SEC-05]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedKey {
    key: VerifyingKey,
}

fn bad(why: impl Into<String>) -> AuditError {
    AuditError::Store(format!("audit key pin: {}", why.into()))
}

impl PinnedKey {
    /// Pin a key held in memory.
    pub fn new(key: VerifyingKey) -> Self {
        Self { key }
    }

    /// Parse exactly one `<alg> <key-id> <hex>` line. Blank lines and `#` comments
    /// are ignored; no line, more than one, or a malformed one is an error — never a
    /// silently absent pin.
    pub fn parse(text: &str) -> Result<Self, AuditError> {
        let mut lines = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'));
        let line = lines.next().ok_or_else(|| bad("no key line"))?;
        if lines.next().is_some() {
            return Err(bad("more than one key line; pin exactly one audit key"));
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        let [alg, _key_id, hex] = parts[..] else {
            return Err(bad("expected `<alg> <key-id> <hex>`"));
        };
        let alg = PIN_ALGS
            .iter()
            .copied()
            .find(|a| a.as_str() == alg)
            .ok_or_else(|| bad(format!("{alg} is not a signature algorithm")))?;
        let bytes = unhex(hex).ok_or_else(|| bad("the key is not valid hex"))?;
        if bytes.is_empty() {
            return Err(bad("the key is empty"));
        }
        Ok(Self::new(VerifyingKey::new(alg, bytes)))
    }

    /// Read and parse a pin file.
    pub fn read(path: impl AsRef<Path>) -> Result<Self, AuditError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|e| bad(format!("cannot read {}: {e}", path.display())))?;
        Self::parse(&text)
    }

    /// The pinned key.
    pub fn key(&self) -> &VerifyingKey {
        &self.key
    }

    /// Refuse a signer whose key is not the pinned one — e.g. a keystore that was
    /// swapped for another. [MG-SEC-05]
    pub fn check(&self, signer: &VerifyingKey) -> Result<(), AuditError> {
        if signer == &self.key {
            Ok(())
        } else {
            Err(AuditError::KeyNotPinned)
        }
    }

    /// Resolve the key for an entry's algorithm: the pin, and only for its own
    /// algorithm. For [`crate::verify::verify_chain`].
    pub fn resolve(&self, alg: SigId) -> Option<VerifyingKey> {
        (alg == self.key.alg()).then(|| self.key.clone())
    }
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let nib = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    s.as_bytes()
        .chunks(2)
        .map(|p| Some(nib(p[0])? << 4 | nib(p[1])?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_trust_anchor_line_and_refuses_anything_else() {
        let pin = PinnedKey::parse("# audit\nML-DSA-65 midgard-audit 0a0B\n").unwrap();
        assert_eq!(
            pin.key(),
            &VerifyingKey::new(SigId::MlDsa65, vec![0x0a, 0x0b])
        );
        assert!(pin.check(pin.key()).is_ok());
        assert!(matches!(
            pin.check(&VerifyingKey::new(SigId::MlDsa65, vec![0x0a, 0x0c])),
            Err(AuditError::KeyNotPinned)
        ));
        for text in [
            "",
            "# only a comment",
            "ML-DSA-65 k 0a\nML-DSA-65 k2 0b",
            "ML-DSA-65 0a",
            "RSA-2048 k 0a",
            "ML-DSA-65 k 0g",
            "ML-DSA-65 k 0",
        ] {
            assert!(PinnedKey::parse(text).is_err(), "{text:?} must not pin");
        }
    }
}
