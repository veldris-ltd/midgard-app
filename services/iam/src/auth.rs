//! The authenticator: the single path from a credential presentation to a [NFR-SEC-2]
//! PQ-signed token. It verifies the principal's **password** (Argon2id/PBKDF2) and
//! **MFA** (TOTP or WebAuthn) against the durable directory, and only on success asks
//! the IdP to mint the ML-DSA token. No factor check can be skipped — an unknown
//! subject, a bad password, a missing or bad second factor each fail closed before
//! any token is signed. This is the RC-29 gate. [RC-29]

use policy::Purpose;

use crate::directory::{Directory, DurableDirectory};
use crate::idp::{Idp, IssueError};
use crate::mfa::{AuthAssertion, ClientDataParser, WebAuthnPolicy};
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
        /// The edge's parser for the assertion's `clientDataJSON`, which the verifier
        /// runs on the signed bytes. [VLD-ANB-MIDGARD-0084]
        parse_client_data: &'a ClientDataParser<'a>,
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

/// A successful authentication: the minted token and, when a TOTP code was the
/// second factor, the time step it matched. The caller persists that step with the
/// directory's `record_totp_step` before releasing the token, under the same lock as
/// the check, so the code cannot be used again. [VLD-ANB-MIDGARD-0026] Likewise, when
/// a passkey was the second factor, the caller persists its signature counter with
/// `record_passkey_count`. [VLD-ANB-MIDGARD-0084]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthOutcome {
    /// The PQ-signed access token.
    pub token: Vec<u8>,
    /// The TOTP step accepted, if the second factor was a TOTP code.
    pub totp_step: Option<u64>,
    /// The credential id and signature counter of the passkey accepted, if the second
    /// factor was a WebAuthn assertion. [VLD-ANB-MIDGARD-0084]
    pub passkey_count: Option<(Vec<u8>, u32)>,
}

/// The authenticator ties an IdP (token minting), a directory (attributes +
/// credentials), and a password hasher together. The directory is the durable one by
/// default; any [`Directory`] authenticates through the same checks, and both the
/// directory and the hasher may be trait objects chosen at startup.
///
/// A passkey assertion is checked against a [`WebAuthnPolicy`]. The default allows no
/// origin, so no assertion verifies until [`Self::with_webauthn_policy`] names the
/// origins served. [VLD-ANB-MIDGARD-0084]
pub struct Authenticator<'a, H: PasswordHasher + ?Sized, D: Directory + ?Sized = DurableDirectory> {
    idp: &'a Idp,
    directory: &'a D,
    hasher: &'a H,
    webauthn: WebAuthnPolicy,
}

impl<'a, H: PasswordHasher + ?Sized, D: Directory + ?Sized> Authenticator<'a, H, D> {
    /// Build an authenticator, with the default (fail-closed) WebAuthn policy.
    pub fn new(idp: &'a Idp, directory: &'a D, hasher: &'a H) -> Self {
        Self {
            idp,
            directory,
            hasher,
            webauthn: WebAuthnPolicy::default(),
        }
    }

    /// Check passkey assertions against `policy`: the origins accepted and whether
    /// user verification is required. [VLD-ANB-MIDGARD-0084]
    pub fn with_webauthn_policy(mut self, policy: WebAuthnPolicy) -> Self {
        self.webauthn = policy;
        self
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
        self.authenticate_full(subject, password, mfa, purpose, now, ttl_secs)
            .map(|outcome| outcome.token)
    }

    /// Authenticate exactly as [`Self::authenticate`] does and also return the TOTP
    /// step accepted, which the caller must persist before the token leaves so the code
    /// is single use. A code for the principal's last accepted step, or an earlier
    /// one, is refused as [`AuthError::BadMfa`]. [VLD-ANB-MIDGARD-0026] [RC-29] The
    /// same holds for a passkey's signature counter. [VLD-ANB-MIDGARD-0084]
    pub fn authenticate_full(
        &self,
        subject: &str,
        password: &[u8],
        mfa: MfaProof,
        purpose: Purpose,
        now: u64,
        ttl_secs: u64,
    ) -> Result<AuthOutcome, AuthError> {
        self.check(subject, password, mfa, purpose, now, ttl_secs)
            .map(|(outcome, _)| outcome)
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
    ) -> Result<(AuthOutcome, Option<String>), AuthError> {
        let Some(entry) = self.directory.enrollment(subject) else {
            // Do the password work anyway: an unknown principal must not be refused
            // measurably faster than a known one with a wrong password.
            self.hasher.spend_verification(password);
            return Err(AuthError::UnknownSubject);
        };

        // 0. An account approved but not yet activated has no password of its owner's
        // choosing, and its second factor is not yet theirs: it cannot log in. The
        // password work is spent so the refusal looks like any wrong password.
        // [VLD-ANB-MIDGARD-0002]
        if entry.credentials.password_hash.is_empty() || entry.credentials.activation.is_some() {
            self.hasher.spend_verification(password);
            return Err(AuthError::BadPassword);
        }

        // 1. Password (constant-time verify against the stored hash).
        if !self
            .hasher
            .verify(password, &entry.credentials.password_hash)
        {
            return Err(AuthError::BadPassword);
        }

        // 2. MFA — mandatory whenever the principal has a factor enrolled. A TOTP code
        // is accepted once: its step must be later than the last one accepted.
        // [VLD-ANB-MIDGARD-0026] A passkey assertion is checked against the policy,
        // and its signature counter must be later than the last one accepted.
        // [VLD-ANB-MIDGARD-0084]
        let mut totp_step = None;
        let mut passkey_count = None;
        if entry.credentials.has_mfa() {
            match mfa {
                MfaProof::Totp(code) => {
                    let secret = entry
                        .credentials
                        .totp
                        .as_ref()
                        .ok_or(AuthError::MfaRequired)?;
                    let step = secret
                        .verify_step(&code, now, entry.credentials.totp_last_step)
                        .ok_or(AuthError::BadMfa)?;
                    totp_step = Some(step);
                }
                MfaProof::WebAuthn {
                    assertion,
                    challenge,
                    parse_client_data,
                } => {
                    let cred = entry
                        .credentials
                        .passkeys
                        .iter()
                        .find(|k| k.credential_id == assertion.credential_id)
                        .ok_or(AuthError::MfaRequired)?;
                    let count = cred
                        .verify_assertion(
                            self.idp.provider(),
                            &self.webauthn,
                            challenge,
                            &assertion,
                            parse_client_data,
                        )
                        .map_err(|_| AuthError::BadMfa)?;
                    passkey_count = Some((cred.credential_id.clone(), count));
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
        Ok((
            AuthOutcome {
                token,
                totp_step,
                passkey_count,
            },
            stale,
        ))
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

/// A successful sign-in: the minted token, what became of the password hash and the
/// TOTP step accepted, which the caller persists before releasing the token, as for
/// [`AuthOutcome`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    /// The PQ-signed access token.
    pub token: Vec<u8>,
    /// Whether the stored password hash was upgraded.
    pub upgrade: PasswordUpgrade,
    /// The TOTP step accepted, if the second factor was a TOTP code.
    /// [VLD-ANB-MIDGARD-0026]
    pub totp_step: Option<u64>,
    /// The passkey accepted and its signature counter, if the second factor was a
    /// WebAuthn assertion. [VLD-ANB-MIDGARD-0084]
    pub passkey_count: Option<(Vec<u8>, u32)>,
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
    webauthn: WebAuthnPolicy,
}

impl<'a, H: PasswordHasher + ?Sized, D: Directory + ?Sized> UpgradingAuthenticator<'a, H, D> {
    /// Build an upgrading authenticator over a directory it may write to, with the
    /// default (fail-closed) WebAuthn policy.
    pub fn new(idp: &'a Idp, directory: &'a mut D, hasher: &'a H) -> Self {
        Self {
            idp,
            directory,
            hasher,
            webauthn: WebAuthnPolicy::default(),
        }
    }

    /// Check passkey assertions against `policy`, as
    /// [`Authenticator::with_webauthn_policy`] does. [VLD-ANB-MIDGARD-0084]
    pub fn with_webauthn_policy(mut self, policy: WebAuthnPolicy) -> Self {
        self.webauthn = policy;
        self
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
        let (outcome, stale) = Authenticator::new(self.idp, &*self.directory, self.hasher)
            .with_webauthn_policy(self.webauthn.clone())
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
        Ok(Login {
            token: outcome.token,
            upgrade,
            totp_step: outcome.totp_step,
            passkey_count: outcome.passkey_count,
        })
    }
}
