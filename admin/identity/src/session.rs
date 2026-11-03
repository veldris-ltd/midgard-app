//! Admin sessions: **WebAuthn-gated** sign-in and per-scope step-up re-authentication.
//!
//! An [`AdminSession`] is only obtainable from [`AdminAuthenticator::sign_in`], which
//! verifies a real WebAuthn assertion (see [`crate::webauthn`]) before the session
//! exists. There is no other constructor — a session **cannot** be built
//! "authenticated" without a verified assertion. Entering an admin scope, and a fresh
//! re-auth at the moment of a high-risk approval, likewise require a freshly-verified
//! phishing-resistant assertion. The clock is injected (Unix seconds) so the state
//! machine is deterministic. [AC-AD-1] [RC-30] [STD:OWASP ASVS V2] [STD:FIDO2]
//!
//! ## What counts as MFA
//! The assertion's `user_verified` flag is chosen and signed by the client, so it is an
//! audit hint ([`AdminSession::uv_claimed`]), never a factor. A session satisfies MFA
//! only with two distinct factors, one of them phishing-resistant: the passkey, plus a
//! second factor the relying party verified itself and recorded with
//! [`AdminSession::record_factor`] (the gateway checks a one-time code). A passkey
//! alone, whatever it claims, is one factor: possession of one keystore file.
//! [VLD-ANB-MIDGARD-0008]

use std::collections::{BTreeMap, BTreeSet};

use pqc_agility::CryptoProvider;

use crate::binding::{AdminId, ResourceScope};
use crate::error::IdentityError;
use crate::webauthn::{
    verify_admin_assertion, AdminChallenge, RegisteredPasskey, WebAuthnAssertion,
};

/// An authentication factor recorded on a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthFactor {
    /// Something you know.
    Password,
    /// A time-based one-time code.
    Totp,
    /// A phishing-resistant WebAuthn / passkey assertion: possession of the passkey.
    /// Its user-verification flag is client-asserted, so it is one factor, not two.
    /// [VLD-ANB-MIDGARD-0008]
    WebAuthn,
}

impl AuthFactor {
    /// Whether the factor is phishing-resistant (WebAuthn / passkeys).
    pub fn is_phishing_resistant(&self) -> bool {
        matches!(self, AuthFactor::WebAuthn)
    }

    /// The factor's name in an audit record: `password`, `totp` or `webauthn`.
    pub fn name(&self) -> &'static str {
        match self {
            AuthFactor::Password => "password",
            AuthFactor::Totp => "totp",
            AuthFactor::WebAuthn => "webauthn",
        }
    }
}

/// An admin's registered credential: their passkey. The second factor is verified by
/// the relying party, outside this crate, and recorded on the session.
/// [VLD-ANB-MIDGARD-0008]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminCredential {
    /// The admin's registered passkey (verifies every assertion).
    pub passkey: RegisteredPasskey,
}

/// Verifies WebAuthn assertions and issues/steps-up [`AdminSession`]s. Holds the
/// admin passkey registry and the crypto provider; it is the **only** source of an
/// authenticated session. [RC-30]
pub struct AdminAuthenticator {
    provider: CryptoProvider,
    admins: BTreeMap<AdminId, AdminCredential>,
}

impl AdminAuthenticator {
    /// Build an authenticator over a crypto provider.
    pub fn new(provider: CryptoProvider) -> Self {
        Self {
            provider,
            admins: BTreeMap::new(),
        }
    }

    /// Register (or replace) an admin's passkey.
    pub fn register(&mut self, admin: AdminId, passkey: RegisteredPasskey) {
        self.admins.insert(admin, AdminCredential { passkey });
    }

    /// The crypto provider (for issuing/answering challenges in tests).
    pub fn provider(&self) -> &CryptoProvider {
        &self.provider
    }

    fn credential(&self, admin: &AdminId) -> Result<&AdminCredential, IdentityError> {
        self.admins.get(admin).ok_or(IdentityError::UnknownAdmin)
    }

    /// Sign in by presenting a WebAuthn assertion that answers `challenge`. The
    /// assertion must verify; only then does an [`AdminSession`] come into existence,
    /// holding the one factor [`AuthFactor::WebAuthn`], which is not MFA. The signed
    /// `user_verified` flag is recorded as [`AdminSession::uv_claimed`] for the audit
    /// record and gates nothing: the client sets it. [AC-AD-1] [RC-30]
    /// [VLD-ANB-MIDGARD-0008]
    pub fn sign_in(
        &self,
        admin: &AdminId,
        challenge: &AdminChallenge,
        assertion: &WebAuthnAssertion,
        now: u64,
    ) -> Result<AdminSession, IdentityError> {
        let cred = self.credential(admin)?;
        // The WebAuthn assertion is mandatory: verify it before anything else.
        verify_admin_assertion(&cred.passkey, challenge, assertion, &self.provider)?;
        let mut factors = BTreeSet::new();
        factors.insert(AuthFactor::WebAuthn);
        Ok(AdminSession {
            admin: admin.clone(),
            factors,
            uv_claimed: assertion.user_verified,
            authenticated_at: now,
            stepped_up: BTreeMap::new(),
            last_reauth: now,
        })
    }

    /// Enter an admin scope by performing a step-up: a **fresh** assertion must verify
    /// and the session must already satisfy MFA; then the step-up is recorded and the
    /// re-auth clock refreshed. A session holding the passkey alone is
    /// [`IdentityError::WeakStepUpFactor`]. [AC-AD-1] [VLD-ANB-MIDGARD-0008]
    pub fn step_up(
        &self,
        session: &mut AdminSession,
        scope: ResourceScope,
        challenge: &AdminChallenge,
        assertion: &WebAuthnAssertion,
        now: u64,
    ) -> Result<(), IdentityError> {
        let cred = self.credential(&session.admin)?;
        verify_admin_assertion(&cred.passkey, challenge, assertion, &self.provider)?;
        if !session.has_mfa() {
            return Err(IdentityError::WeakStepUpFactor);
        }
        session.stepped_up.insert(scope, now);
        session.last_reauth = now;
        Ok(())
    }

    /// A fresh re-authentication (required at the moment of approving a high-risk
    /// action): on a session that satisfies MFA, verify a fresh assertion and refresh
    /// the clock. [AC-AD-1] [VLD-ANB-MIDGARD-0008]
    pub fn reauth(
        &self,
        session: &mut AdminSession,
        challenge: &AdminChallenge,
        assertion: &WebAuthnAssertion,
        now: u64,
    ) -> Result<(), IdentityError> {
        let cred = self.credential(&session.admin)?;
        verify_admin_assertion(&cred.passkey, challenge, assertion, &self.provider)?;
        if !session.has_mfa() {
            return Err(IdentityError::WeakStepUpFactor);
        }
        session.last_reauth = now;
        Ok(())
    }
}

/// An authenticated administrator session. Constructable **only** via
/// [`AdminAuthenticator::sign_in`] (all fields private, no other constructor), so its
/// mere existence is proof a WebAuthn assertion was verified. [RC-30]
#[derive(Debug, Clone)]
pub struct AdminSession {
    admin: AdminId,
    factors: BTreeSet<AuthFactor>,
    uv_claimed: bool,
    authenticated_at: u64,
    stepped_up: BTreeMap<ResourceScope, u64>,
    last_reauth: u64,
}

impl AdminSession {
    /// The admin this session belongs to.
    pub fn admin(&self) -> &AdminId {
        &self.admin
    }

    /// The verified factors on this session.
    pub fn factors(&self) -> &BTreeSet<AuthFactor> {
        &self.factors
    }

    /// When the session was authenticated (Unix seconds).
    pub fn authenticated_at(&self) -> u64 {
        self.authenticated_at
    }

    /// Record a second factor on this session. Only the relying party calls this, and
    /// only after it has itself verified `factor` for this administrator (the gateway
    /// checks a one-time code against the secret it holds); a client's say-so is never
    /// a factor. [VLD-ANB-MIDGARD-0008]
    pub fn record_factor(&mut self, factor: AuthFactor) {
        self.factors.insert(factor);
    }

    /// The verified factors' names, in order, for the audit record.
    pub fn factor_names(&self) -> Vec<&'static str> {
        self.factors.iter().map(AuthFactor::name).collect()
    }

    /// Whether the signed assertion that opened the session claimed user verification.
    /// The client sets the flag, so it is recorded for audit and trusted for nothing.
    /// [VLD-ANB-MIDGARD-0008]
    pub fn uv_claimed(&self) -> bool {
        self.uv_claimed
    }

    /// Whether the session satisfies MFA: at least two distinct verified factors, one
    /// of them phishing-resistant. A passkey alone never does, whatever its
    /// `user_verified` flag says. [VLD-ANB-MIDGARD-0008] [STD:OWASP ASVS V6]
    pub fn has_mfa(&self) -> bool {
        self.factors.len() >= 2 && self.factors.iter().any(AuthFactor::is_phishing_resistant)
    }

    /// Assert the session has a fresh step-up for `scope`. Fails closed if the scope
    /// was never entered or its step-up is older than `max_age`. [AC-AD-1]
    pub fn authorize_scope(
        &self,
        scope: &ResourceScope,
        now: u64,
        max_age: u64,
    ) -> Result<(), IdentityError> {
        match self.stepped_up.get(scope) {
            None => Err(IdentityError::StepUpRequired {
                scope: render_scope(scope),
            }),
            Some(&at) if now.saturating_sub(at) >= max_age => Err(IdentityError::StepUpStale {
                scope: render_scope(scope),
            }),
            Some(_) => Ok(()),
        }
    }

    /// Whether the session's most recent re-auth is within `max_age` of `now`.
    pub fn has_fresh_reauth(&self, now: u64, max_age: u64) -> bool {
        now.saturating_sub(self.last_reauth) < max_age
    }
}

fn render_scope(scope: &ResourceScope) -> String {
    format!("{}/{}", scope.subsystem.0, scope.selector)
}
