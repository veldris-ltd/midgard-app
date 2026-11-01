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
    ///
    /// This reads the directory only, so a legacy password hash stays as it is; use
    /// [`UpgradingAuthenticator`] to also replace it on sign-in. [MG-SEC-11]
    pub fn authenticate(
        &self,
        subject: &str,
        password: &[u8],
        mfa: MfaProof,
        purpose: Purpose,
        now: u64,
        ttl_secs: u64,
    ) -> Result<Vec<u8>, AuthError> {
        self.check(subject, password, mfa, purpose, now, ttl_secs)
            .map(|(token, _)| token)
    }

    /// The whole check. On success, also the stored password hash when the hasher
    /// wants it replaced (legacy or below current parameters). [MG-SEC-11]
    fn check(
        &self,
        subject: &str,
        password: &[u8],
        mfa: MfaProof,
        purpose: Purpose,
        now: u64,
        ttl_secs: u64,
    ) -> Result<(Vec<u8>, Option<String>), AuthError> {
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
        let token = self
            .idp
            .mint_token(subject, &entry.user, purpose, now, ttl_secs)
            .map_err(AuthError::Issue)?;
        let stale = &entry.credentials.password_hash;
        let stale = self.hasher.needs_rehash(stale).then(|| stale.clone());
        Ok((token, stale))
    }
}

/// What became of the principal's stored password hash after a successful sign-in
/// through [`UpgradingAuthenticator`]. [MG-SEC-11]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordUpgrade {
    /// The stored hash is already in the current format; nothing was written.
    Current,
    /// The stored hash was legacy (or weaker than current) and has been replaced by
    /// an Argon2id hash of the password just presented, and persisted.
    Upgraded,
    /// The enrollment changed between the check and the write (re-enrolled or
    /// removed); the newer entry was left alone.
    Superseded,
    /// The replacement could not be recorded. The sign-in still stands; the next one
    /// tries again.
    NotRecorded(String),
}

/// A successful sign-in: the minted token and what became of the password hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    /// The PQ-signed access token.
    pub token: Vec<u8>,
    /// Whether the stored password hash was upgraded.
    pub upgrade: PasswordUpgrade,
}

/// The authenticator with **rehash on sign-in**: the same checks as
/// [`Authenticator::authenticate`] — nothing is written unless every one passes —
/// then, when the stored hash is legacy PBKDF2 or weaker than the current Argon2id
/// parameters, it hashes the password just presented with the hasher's
/// [`rehash`](PasswordHasher::rehash) and persists it through
/// [`Directory::replace_password_hash`]. Old entries are thereby replaced the next
/// time their owners sign in. [MG-SEC-11] [RC-29]
pub struct UpgradingAuthenticator<'a, H: PasswordHasher + ?Sized, D: Directory + ?Sized> {
    idp: &'a Idp,
    directory: &'a mut D,
    hasher: &'a H,
}

impl<'a, H: PasswordHasher + ?Sized, D: Directory + ?Sized> UpgradingAuthenticator<'a, H, D> {
    /// Build an upgrading authenticator over a directory it may write to.
    pub fn new(idp: &'a Idp, directory: &'a mut D, hasher: &'a H) -> Self {
        Self {
            idp,
            directory,
            hasher,
        }
    }

    /// Authenticate exactly as [`Authenticator::authenticate`] does and, on success,
    /// upgrade a stale password hash. A failure to record the upgrade does not fail
    /// the sign-in; it is reported in [`Login::upgrade`]. [MG-SEC-11]
    pub fn authenticate(
        &mut self,
        subject: &str,
        password: &[u8],
        mfa: MfaProof,
        purpose: Purpose,
        now: u64,
        ttl_secs: u64,
    ) -> Result<Login, AuthError> {
        let (token, stale) = Authenticator::new(self.idp, &*self.directory, self.hasher)
            .check(subject, password, mfa, purpose, now, ttl_secs)?;
        let upgrade = match stale {
            None => PasswordUpgrade::Current,
            Some(current) => {
                let new_hash = self.hasher.rehash(password);
                match self
                    .directory
                    .replace_password_hash(subject, &current, new_hash)
                {
                    Ok(true) => PasswordUpgrade::Upgraded,
                    Ok(false) => PasswordUpgrade::Superseded,
                    Err(e) => PasswordUpgrade::NotRecorded(e),
                }
            }
        };
        Ok(Login { token, upgrade })
    }
}
