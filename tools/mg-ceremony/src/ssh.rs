//! The SSH public keys of the people who may sign a release tag.
//!
//! git verifies tag signatures against `deploy/trust/allowed_signers`, so the
//! ceremony reads each signer's `.pub` file, refuses anything git could not sign a
//! tag with (or anything private), and computes the fingerprint `ssh-keygen -lf`
//! prints, so the record and the second channel carry the same value a reader
//! gets on their own machine. No `ssh-keygen` is needed for any of it.

use pqc_agility::sha256;

/// The key types git can sign tags with: ed25519 and ECDSA P-256, each also in its
/// FIDO (hardware-backed) form.
pub const SIGNING_KEY_TYPES: [&str; 4] = [
    "ssh-ed25519",
    "sk-ssh-ed25519@openssh.com",
    "ecdsa-sha2-nistp256",
    "sk-ecdsa-sha2-nistp256@openssh.com",
];

/// One person who may sign a release tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signer {
    /// As git records the tagger: an email address.
    pub principal: String,
    pub key_type: String,
    /// The base64 key blob, as it appears in the `.pub` file.
    pub key: String,
    /// `SHA256:<base64>`, as `ssh-keygen -lf` prints it.
    pub fingerprint: String,
}

impl Signer {
    /// Read a signer from the text of their `.pub` file.
    pub fn parse(principal: &str, text: &str) -> Result<Self, String> {
        check_principal(principal)?;
        if text.contains("PRIVATE KEY") {
            return Err("this is a private key; give the .pub file beside it".into());
        }
        let line = text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .ok_or("the file is empty")?;
        let mut fields = line.split_whitespace();
        let key_type = fields.next().unwrap_or_default();
        if !SIGNING_KEY_TYPES.contains(&key_type) {
            return Err(format!(
                "not an SSH public key git can sign tags with (got '{key_type}'); \
                 use an ed25519 key: ssh-keygen -t ed25519"
            ));
        }
        let key = fields
            .next()
            .ok_or("the file holds no key after its type")?;
        let blob = base64_decode(key).ok_or("the key is not valid base64")?;
        // The blob names its own type first; a mismatch is a damaged or edited file.
        if blob_type(&blob) != Some(key_type.as_bytes()) {
            return Err(format!(
                "the key does not match its stated type '{key_type}'"
            ));
        }
        Ok(Self {
            principal: principal.to_string(),
            key_type: key_type.to_string(),
            key: key.to_string(),
            fingerprint: format!("SHA256:{}", base64_encode_unpadded(&sha256(&blob))),
        })
    }

    /// The `allowed_signers` line for this signer, for the namespace `git`.
    pub fn allowed_signers_line(&self) -> String {
        format!(
            "{} namespaces=\"git\" {} {}",
            self.principal, self.key_type, self.key
        )
    }
}

/// A principal is one word with no quotes, since it is written unquoted into
/// `allowed_signers`. In practice it is the email address the signer tags with.
pub fn check_principal(principal: &str) -> Result<(), String> {
    if principal.is_empty() {
        return Err("give the signer's email address".into());
    }
    if principal.chars().any(|c| c.is_whitespace() || c == '"') {
        return Err(format!(
            "'{principal}' must be one word with no quotes, e.g. an email address"
        ));
    }
    Ok(())
}

/// The type string an SSH key blob starts with: a big-endian u32 length, then
/// that many bytes.
fn blob_type(blob: &[u8]) -> Option<&[u8]> {
    let len = u32::from_be_bytes(blob.get(..4)?.try_into().ok()?) as usize;
    blob.get(4..4 + len)
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64, padded or not. `None` on any byte outside the alphabet.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = ALPHABET.iter().position(|&a| a == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Base64 with no padding, the form OpenSSH prints fingerprints in.
fn base64_encode_unpadded(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // A real ed25519 public key (public material only), and its fingerprint as
    // `ssh-keygen -lf` prints it, computed independently of this module.
    const KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINyw+6K7GLdHy8Z8lAxVMz5rVCCiWQhotLU9107+qSZW alice";
    const FINGERPRINT: &str = "SHA256:JcqaqS1Mgl8TGWjTR0WomD/A/NTvNdkS4iEPMk/24q8";

    #[test]
    fn reads_a_signer_and_prints_the_fingerprint_ssh_keygen_does() {
        let s = Signer::parse("alice@example.invalid", KEY).expect("parse");
        assert_eq!(s.key_type, "ssh-ed25519");
        assert_eq!(s.fingerprint, FINGERPRINT);
        assert_eq!(
            s.allowed_signers_line(),
            "alice@example.invalid namespaces=\"git\" ssh-ed25519 \
             AAAAC3NzaC1lZDI1NTE5AAAAINyw+6K7GLdHy8Z8lAxVMz5rVCCiWQhotLU9107+qSZW"
        );
    }

    #[test]
    fn tolerates_windows_line_endings_and_leading_blank_lines() {
        let s = Signer::parse("a@b", &format!("\r\n\r\n{KEY}\r\n")).expect("parse");
        assert_eq!(s.fingerprint, FINGERPRINT);
    }

    #[test]
    fn refuses_a_private_key() {
        let text = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n-----END OPENSSH PRIVATE KEY-----\n";
        let e = Signer::parse("a@b", text).unwrap_err();
        assert!(e.contains("private key"), "{e}");
    }

    #[test]
    fn refuses_keys_git_cannot_sign_tags_with() {
        let e = Signer::parse("a@b", "ssh-rsa AAAAB3NzaC1yc2E= bob").unwrap_err();
        assert!(
            e.contains("not an SSH public key git can sign tags with"),
            "{e}"
        );
        assert!(Signer::parse("a@b", "not a key").is_err());
        assert!(Signer::parse("a@b", "").is_err());
        assert!(Signer::parse("a@b", "ssh-ed25519").is_err());
    }

    #[test]
    fn refuses_a_key_whose_blob_disagrees_with_its_type() {
        // An ed25519 blob labelled as ECDSA.
        let text = KEY.replace("ssh-ed25519 ", "ecdsa-sha2-nistp256 ");
        let e = Signer::parse("a@b", &text).unwrap_err();
        assert!(e.contains("does not match"), "{e}");
        assert!(Signer::parse("a@b", "ssh-ed25519 !!!!").is_err());
    }

    #[test]
    fn refuses_principals_that_would_break_the_signers_file() {
        assert!(Signer::parse("", KEY).is_err());
        assert!(Signer::parse("two words", KEY).is_err());
        assert!(Signer::parse("q\"uote", KEY).is_err());
    }

    #[test]
    fn base64_round_trips() {
        for len in 0..40u8 {
            let bytes: Vec<u8> = (0..len).map(|i| i.wrapping_mul(37)).collect();
            let enc = base64_encode_unpadded(&bytes);
            assert!(!enc.contains('='));
            assert_eq!(base64_decode(&enc).as_deref(), Some(&bytes[..]));
        }
        assert_eq!(base64_encode_unpadded(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode_unpadded(b"fo"), "Zm8");
    }
}
