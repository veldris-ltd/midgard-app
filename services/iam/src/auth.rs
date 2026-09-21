//! The authenticator: the single path from a credential presentation to a [NFR-SEC-2]
//! PQ-signed token. It verifies the principal's **password** (Argon2id/PBKDF2) and
//! **MFA** (TOTP or WebAuthn) against the durable directory, and only on success asks
//! the IdP to mint the ML-DSA token. No factor check can be skipped — an unknown
//! subject, a bad password, a missing or bad second factor each fail closed before
//! any token is signed. This is the RC-29 gate. [RC-29]

use policy::Purpose;

use crate::directory::{Directory, DurableDirectory};
use crate::idp::{Idp, IssueError};
use crate::mfa::AuthAssertion;
use crate::password::PasswordHasher;

/// The second-factor proof a caller presents.
pub enum MfaProof<'a> {
    /// A TOTP code from the user's authenticator app.
    Totp(String),
    /// A WebAuthn assertion plus the server challenge it answers.
    WebAuthn {
        /// The assertion returned by the authenticator.
        assertion: AuthAssertion,
        /// The challenge the server issued for this login.
        challenge: &'a [u8],
    },
    /// No second factor presented (only valid if the user has none enrolled).
    None,
}

/// Why authentication failed. Each variant is fail-closed: no token is minted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// No such enrolled principal.
    UnknownSubject,
    /// The password did not verify.
    BadPassword,
    /// The principal has a second factor enrolled but none (or a wrong kind) was
    /// presented.
    MfaRequired,
    /// The presented second factor did not verify.
    BadMfa,
    /// Credentials verified, but token issuance was refused (e.g. purpose).
    Issue(IssueError),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::UnknownSubject => write!(f, "unknown subject"),
            AuthError::BadPassword => write!(f, "password verification failed"),
            AuthError::MfaRequired => write!(f, "a second factor is required"),
            AuthError::BadMfa => write!(f, "second factor verification failed"),
            AuthError::Issue(e) => write!(f, "token issuance refused: {e}"),
        }
    }
}
impl std::error::Error for AuthError {}

/// The authenticator ties an IdP (token minting), a directory (attributes +
/// credentials), and a password hasher together. The directory is the durable one by
/// default; any [`Directory`] authenticates through the same checks, and both the
/// directory and the hasher may be trait objects chosen at startup.
pub struct Authenticator<'a, H: PasswordHasher + ?Sized, D: Directory + ?Sized = DurableDirectory> {
    idp: &'a Idp,
    directory: &'a D,
    hasher: &'a H,
}

impl<'a, H: PasswordHasher + ?Sized, D: Directory + ?Sized> Authenticator<'a, H, D> {
    /// Build an authenticator.
    pub fn new(idp: &'a Idp, directory: &'a D, hasher: &'a H) -> Self {
        Self {
            idp,
            directory,
            hasher,
        }
    }

    /// Authenticate a principal and, on success, mint a PQ-signed token. Verifies
    /// password **and** MFA before issuance; every failure path mints nothing. `now`
    /// is the unix time used both to bound the token and to check TOTP. [RC-29]
    pub fn authenticate(
        &self,
        subject: &str,
        password: &[u8],
        mfa: MfaProof,
        purpose: Purpose,
        now: u64,
        ttl_secs: u64,
    ) -> Result<Vec<u8>, AuthError> {
        let Some(entry) = self.directory.enrollment(subject) else {
            // Do the password work anyway: an unknown principal must not be refused
            // measurably faster than a known one with a wrong password.
            self.hasher.spend_verification(password);
            return Err(AuthError::UnknownSubject);
        };

        // 1. Password (constant-time verify against the stored hash).
        if !self
            .hasher
            .verify(password, &entry.credentials.password_hash)
        {
            return Err(AuthError::BadPassword);
        }

        // 2. MFA — mandatory whenever the principal has a factor enrolled.
        if entry.credentials.has_mfa() {
            match mfa {
                MfaProof::Totp(code) => {
                    let secret = entry
                        .credentials
                        .totp
                        .as_ref()
                        .ok_or(AuthError::MfaRequired)?;
                    if !secret.verify(&code, now) {
                        return Err(AuthError::BadMfa);
                    }
                }
                MfaProof::WebAuthn {
                    assertion,
                    challenge,
                } => {
                    let cred = entry
                        .credentials
                        .passkeys
                        .iter()
                        .find(|k| k.credential_id == assertion.credential_id)
                        .ok_or(AuthError::MfaRequired)?;
                    cred.verify_assertion(self.idp.provider(), challenge, &assertion)
                        .map_err(|_| AuthError::BadMfa)?;
                }
                MfaProof::None => return Err(AuthError::MfaRequired),
            }
        }

        // 3. Only now mint the token, from the directory's attributes.
        self.idp
            .mint_token(subject, &entry.user, purpose, now, ttl_secs)
            .map_err(AuthError::Issue)
    }
}
