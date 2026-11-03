//! Administrators' second factor: a TOTP secret per administrator, sealed at rest in
//! `admin/totp.journal` under the issuer's custody root like the passkeys, and checked
//! by the gateway on every administration request. [VLD-ANB-MIDGARD-0008]
//!
//! A passkey alone is one factor, possession of one keystore file, and its
//! user-verification flag is whatever the client signs. The second factor is a code
//! the gateway verifies itself, against a secret only it and the administrator's
//! authenticator app hold, so a copied keystore file is not enough to administer.
//!
//! Each code is accepted once. The store is an [`iam::DurableDirectory`] opened in its
//! own sealing domain: the same version 3 records, single-use step tracking and replay
//! that make a principal's login code single use (WP-57), with the administrator id as
//! the subject and nothing but the TOTP secret and the last accepted step in the
//! entry. A verification appends the step before it succeeds, so a crash in between
//! fails closed. Keeping it out of `admin-identity` keeps that crate free of `iam`.
//! [VLD-ANB-MIDGARD-0026] [RC-29]

use std::collections::BTreeSet;
use std::path::Path;

use audit_writer::{AuditError, Sealer};
use iam::{Credentials, DurableDirectory, Enrollment, TotpSecret, UserRecord};
use policy::{ClassificationLevel, Clearance};

/// The sealing domain of the administrators' TOTP journal.
const DOMAIN: &[u8] = b"midgard-admin-totp-v1";

/// A refused one-time code. It does not say why: unknown administrator, wrong code,
/// a code already used, or a step that could not be recorded are all this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeRefused;

impl std::fmt::Display for CodeRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("one-time code refused")
    }
}

impl std::error::Error for CodeRefused {}

/// Administrators' TOTP secrets and last accepted steps, durable and sealed.
pub struct DurableAdminTotp {
    store: DurableDirectory,
}

impl DurableAdminTotp {
    /// Open (creating if absent) the journal at `path`, sealed by `sealer`, replaying
    /// every enrolled secret and accepted step. A journal that does not authenticate
    /// refuses to open.
    pub fn open(path: impl AsRef<Path>, sealer: Box<dyn Sealer>) -> Result<Self, AuditError> {
        Ok(Self {
            store: DurableDirectory::open_in_domain(path, sealer, DOMAIN)?,
        })
    }

    /// Enrol `admin`'s TOTP secret, replacing any earlier one (a reset for a lost
    /// authenticator). The last accepted step is never wound back, so a code accepted
    /// before the reset stays spent.
    pub fn enrol(&mut self, admin: &str, secret: TotpSecret) -> Result<(), AuditError> {
        let credentials = Credentials {
            totp: Some(secret),
            ..Credentials::default()
        };
        if self.store.update_credentials(admin, credentials.clone())? {
            return Ok(());
        }
        self.store.enroll(
            admin,
            Enrollment {
                // An administrator's entry carries no authority: roles and purposes
                // come from the role bindings, never from here.
                user: UserRecord {
                    roles: BTreeSet::new(),
                    clearance: Clearance {
                        level: ClassificationLevel(0),
                        compartments: BTreeSet::new(),
                    },
                    purposes: BTreeSet::new(),
                },
                credentials,
            },
        )
    }

    /// Verify `code` for `admin` at `now`, with one 30 s step of skew, refusing a code
    /// for a step at or below the last one accepted, then record its step before
    /// returning `Ok`. Every refusal is the same [`CodeRefused`].
    pub fn verify(&mut self, admin: &str, code: &str, now: u64) -> Result<(), CodeRefused> {
        let entry = self.store.get(admin).ok_or(CodeRefused)?;
        let secret = entry.credentials.totp.as_ref().ok_or(CodeRefused)?;
        let step = secret
            .verify_step(code, now, entry.credentials.totp_last_step)
            .ok_or(CodeRefused)?;
        match self.store.record_totp_step(admin, step) {
            Ok(true) => Ok(()),
            Ok(false) | Err(_) => Err(CodeRefused),
        }
    }

    /// Whether `admin` has a TOTP secret enrolled.
    pub fn is_enrolled(&self, admin: &str) -> bool {
        self.store
            .get(admin)
            .is_some_and(|e| e.credentials.totp.is_some())
    }

    /// How many administrators have a TOTP secret enrolled, for the boot summary.
    pub fn administrators_with_totp(&self) -> usize {
        self.store
            .subjects()
            .filter(|admin| self.is_enrolled(admin))
            .count()
    }
}
