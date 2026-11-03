//! Online enrolment of principals under dual control. [RC-29] [FR-AD-5] [AC-AD-3]
//! [AC-AD-4]
//!
//! An administrator proves who they are with their post-quantum passkey: they fetch a
//! single-use challenge, sign it with the ML-DSA-65 key in their sealed keystore, and
//! send the assertion with the request. The gateway signs them in, steps them into the
//! `iam` scope and submits a principal-enrolment action to the privilege service. That
//! action is high-risk, so it is **parked**: nobody can log in as the new principal
//! until a second, distinct administrator qualified for it approves, with a fresh
//! re-authentication of their own. Only then is the principal written to the durable
//! identity directory.
//!
//! ## Who holds the credentials
//! Neither administrator ever does. A request names only who the principal is and
//! what they may do: roles and purposes from the catalogue the served policy defines,
//! a clearance band and pattern-checked compartments. A request carrying a password
//! hash or a TOTP secret is refused. At approval the gateway mints the TOTP secret and
//! a single-use, 128-bit activation code valid for [`ACTIVATION_TTL`]; it stores only
//! the code's hash, and returns the code once, to the approver, who hands it to the
//! principal out of band. The principal activates at `POST /api/v1/activate`, choosing
//! their password and receiving the TOTP secret, and until then the record cannot log
//! in. The approver sees the full record and names the subject they approve.
//! [VLD-ANB-MIDGARD-0002]
//!
//! ## Recovering a failed approval
//! The privilege approval comes first, then the directory write, then the settlement.
//! If the write fails the enrolment stays parked and its original approver may approve
//! it again, which writes and settles it without a second privilege approval; and any
//! parked enrolment can be rejected, which purges it. [VLD-ANB-MIDGARD-0066]
//!
//! A parked enrolment is sealed at rest, so it survives a restart until it is settled.
//!
//! ## Two factors, both verified here
//! A passkey assertion is one factor, possession of the administrator's keystore, and
//! its `userVerified` flag is whatever the client signed: it is recorded for audit and
//! trusted for nothing. Every administration request also carries `totp`, a one-time
//! code the gateway checks against the administrator's secret in
//! [`DurableAdminTotp`], accepting each code once. Only then does the session hold two
//! factors and satisfy the privilege service's MFA check. A missing, wrong or reused
//! code is the same 401, "second factor required". [VLD-ANB-MIDGARD-0008]
//!
//! A challenge is bound to the administrator who asked for it: the request names
//! them, and only they can answer it. Each registered administrator holds at most
//! [`MAX_CHALLENGES_PER_ADMIN`] at once, a fifth replacing their own oldest, so a flood
//! naming one administrator cannot evict another's. A name with no passkey gets a nonce
//! in the same shape but nothing is kept for it, and past a global ceiling a new
//! administrator is refused rather than anyone's challenge dropped.
//! [VLD-ANB-MIDGARD-0020]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use admin_identity::{
    AdminChallenge, AdminId, AdminSession, AuthFactor, DurableAdminPasskeys,
    DurableIdentityDirectory, ResourceScope, WebAuthnAssertion, PASSKEY_SIG_ALG,
};
use admin_privilege::{
    ActionClass, ActionState, AdminActionRequest, Disposition, DurablePrivilege,
};
use audit_writer::{AuditError, SealedJournal, Sealer};
use iam::{Activation, Credentials, Enrollment, PasswordHasher, TotpSecret, UserRecord};
use policy::{Clearance, Compartment, PolicySet, Purpose, Role};
use pqc_agility::{OsRng, Rng, Signature};

use crate::admin_totp::DurableAdminTotp;
use crate::enclosure::{band, level_of};
use crate::json::Json;
use crate::strict_json::{object, only_keys, optional_bool, strings, text};
use crate::{hex_decode, hex_encode};

/// How long a challenge may be answered, in seconds.
pub const CHALLENGE_TTL: u64 = 120;
/// How recent a requester's step-up must be, in seconds.
pub const STEP_UP_MAX_AGE: u64 = 300;
/// How recent an approver's re-authentication must be, in seconds.
pub const REAUTH_MAX_AGE: u64 = 120;
/// The most challenges one administrator may hold at once; a further one replaces
/// their own oldest. [VLD-ANB-MIDGARD-0020]
pub const MAX_CHALLENGES_PER_ADMIN: usize = 4;
/// The most challenges outstanding at once, across administrators. A ceiling only:
/// past it a new administrator is refused, and no one's challenge is dropped.
/// [VLD-ANB-MIDGARD-0020]
pub const MAX_CHALLENGES: usize = 1_024;
/// How often a refusal at the ceiling is recorded, in seconds; those between are
/// counted onto the next record. [VLD-ANB-MIDGARD-0020]
const REFUSAL_RECORD_INTERVAL: u64 = 60;
/// How long an activation code issued at approval stays usable, in seconds (72 h).
/// [VLD-ANB-MIDGARD-0002]
pub const ACTIVATION_TTL: u64 = 72 * 3_600;
/// What the approver is told about the second factor: the principal provisions it
/// themselves, at activation.
pub const MFA_AT_ACTIVATION: &str = "totp at activation";
/// The activation code's length in bytes (128 bits); it travels as 32 hex characters.
const ACTIVATION_CODE_LEN: usize = 16;
/// The TOTP secret minted at approval, in bytes: the length `mg-admin` uses.
const TOTP_SECRET_LEN: usize = 20;
/// The scope every principal enrolment is requested in.
const IAM_SCOPE: &str = "iam";
/// Parked enrolments carry no credential material from version 2 on.
const PENDING_DOMAIN: &[u8] = b"midgard-admin-pending-enrolments-v2";
/// The domain an earlier build sealed parked enrolments, with their credentials, under.
const PENDING_DOMAIN_V1: &[u8] = b"midgard-admin-pending-enrolments-v1";
const OP_PARKED: u8 = 0;
const OP_SETTLED: u8 = 1;
/// The longest compartment name a request may give.
const MAX_COMPARTMENT_LEN: usize = 32;
/// The one refusal for a missing, malformed, wrong or reused one-time code: it does
/// not say which. [VLD-ANB-MIDGARD-0008]
const SECOND_FACTOR_REQUIRED: &str = "second factor required";

/// A refusal, already mapped to its HTTP status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminError {
    /// HTTP status.
    pub status: u16,
    /// What went wrong, for the administrator's tool.
    pub message: String,
}

fn refuse(status: u16, message: impl Into<String>) -> AdminError {
    AdminError {
        status,
        message: message.into(),
    }
}

/// Who an administration request was authenticated as, and with what: for the
/// gateway's audit record. [VLD-ANB-MIDGARD-0008]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminActor {
    /// The administrator.
    pub admin: String,
    /// The factors the gateway verified, joined with `+`: `totp+webauthn`.
    pub factors: String,
    /// Whether the passkey assertion claimed user verification: a client-set hint,
    /// recorded, never relied on.
    pub uv_claimed: bool,
}

impl AdminActor {
    fn of(session: &AdminSession) -> Self {
        Self {
            admin: session.admin().0.clone(),
            factors: session.factor_names().join("+"),
            uv_claimed: session.uv_claimed(),
        }
    }

    /// The evidence an audit record carries: `factors=totp+webauthn uv-claimed=false`.
    pub fn evidence(&self) -> String {
        format!("factors={} uv-claimed={}", self.factors, self.uv_claimed)
    }
}

/// The roles and purposes an enrolment may grant: those the served policy names, so an
/// approver is never asked to approve a role no rule gives anything to, plus the
/// purpose a login token is issued for. [VLD-ANB-MIDGARD-0002]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoleCatalogue {
    roles: BTreeSet<String>,
    purposes: BTreeSet<String>,
}

impl RoleCatalogue {
    /// Every role and every purpose some rule of `policies` names, and `login_purpose`.
    pub fn from_policy(policies: &PolicySet, login_purpose: &str) -> Self {
        let mut catalogue = Self::default();
        catalogue.purposes.insert(login_purpose.to_string());
        for rule in policies.rules() {
            catalogue.roles.insert(rule.role.0.clone());
            if let Some(purpose) = &rule.purpose {
                catalogue.purposes.insert(purpose.0.clone());
            }
        }
        catalogue
    }

    /// The role names an enrolment may grant.
    pub fn roles(&self) -> impl Iterator<Item = &str> + '_ {
        self.roles.iter().map(String::as_str)
    }

    /// The purpose names an enrolment may grant.
    pub fn purposes(&self) -> impl Iterator<Item = &str> + '_ {
        self.purposes.iter().map(String::as_str)
    }
}

/// A principal as an administrator asked for them to be enrolled: who they are and
/// what they may do, never how they authenticate.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParkedEnrolment {
    subject: String,
    roles: Vec<String>,
    level: u8,
    compartments: Vec<String>,
    purposes: Vec<String>,
}

impl ParkedEnrolment {
    /// The directory entry, with credentials minted here and now: a TOTP secret, no
    /// password, and a pending activation. Returns the entry and the activation code,
    /// whose hash alone the entry keeps. [VLD-ANB-MIDGARD-0002]
    fn mint(self, hasher: &dyn PasswordHasher, now: u64) -> (Enrollment, String) {
        let mut code = [0u8; ACTIVATION_CODE_LEN];
        OsRng.fill(&mut code);
        let code = hex_encode(&code);
        let mut secret = vec![0u8; TOTP_SECRET_LEN];
        OsRng.fill(&mut secret);
        let enrollment = Enrollment {
            user: UserRecord {
                roles: self.roles.into_iter().map(Role).collect(),
                clearance: Clearance {
                    level: policy::ClassificationLevel(self.level),
                    compartments: self.compartments.into_iter().map(Compartment).collect(),
                },
                purposes: self.purposes.into_iter().map(Purpose).collect(),
            },
            credentials: Credentials {
                password_hash: String::new(),
                totp: Some(TotpSecret::new(secret)),
                passkeys: Vec::new(),
                activation: Some(Activation {
                    code_hash: hasher.hash(code.as_bytes()),
                    expires_at: now.saturating_add(ACTIVATION_TTL),
                    totp_confirmed: false,
                }),
                totp_last_step: None,
            },
        };
        (enrollment, code)
    }
}

/// A challenge waiting to be answered, by the administrator it was issued to.
#[derive(Debug, Clone)]
struct Challenge {
    admin: AdminId,
    expires: u64,
}

/// Administration of a served enclosure: passkeys, administrators' one-time-code
/// secrets, role bindings, the dual-control service, and the enrolments it has parked.
pub struct AdminControl {
    passkeys: DurableAdminPasskeys,
    totp: DurableAdminTotp,
    roles: DurableIdentityDirectory,
    privilege: DurablePrivilege,
    journal: SealedJournal,
    parked: BTreeMap<String, ParkedEnrolment>,
    challenges: BTreeMap<[u8; 32], Challenge>,
    challenge_ceiling: usize,
    /// When a refusal at the ceiling was last recorded, and how many since.
    ceiling_refusals: (Option<u64>, u64),
}

impl AdminControl {
    /// Assemble administration over its durable parts, replaying the parked enrolments
    /// sealed in `pending_path`. Parked enrolments an earlier build sealed with their
    /// credentials are re-parked without them. `totp` holds each administrator's second
    /// factor. [VLD-ANB-MIDGARD-0008]
    pub fn open(
        passkeys: DurableAdminPasskeys,
        totp: DurableAdminTotp,
        roles: DurableIdentityDirectory,
        privilege: DurablePrivilege,
        pending_path: impl AsRef<Path>,
        pending_sealer: Box<dyn Sealer>,
    ) -> Result<Self, String> {
        let (journal, parked) = open_pending(pending_path.as_ref(), pending_sealer)?;
        Ok(Self {
            passkeys,
            totp,
            roles,
            privilege,
            journal,
            parked,
            challenges: BTreeMap::new(),
            challenge_ceiling: MAX_CHALLENGES,
            ceiling_refusals: (None, 0),
        })
    }

    /// Administrators with a registered passkey.
    pub fn administrators(&self) -> usize {
        self.passkeys.admins().len()
    }

    /// Whether `name` is an administrator with a registered passkey, so a refusal
    /// naming them is recorded by name. [AC-27.4]
    pub fn is_administrator(&self, name: &str) -> bool {
        self.passkeys.admins().iter().any(|admin| admin.0 == name)
    }

    /// Administrators with a TOTP secret enrolled: those who can administer, since a
    /// passkey alone is not enough. [VLD-ANB-MIDGARD-0008]
    pub fn administrators_with_totp(&self) -> usize {
        self.totp.administrators_with_totp()
    }

    /// Enrolments waiting for a second administrator.
    pub fn parked(&self) -> usize {
        self.parked.len()
    }

    /// Hold at most `ceiling` challenges across administrators, in place of
    /// [`MAX_CHALLENGES`]; at least one. [VLD-ANB-MIDGARD-0020]
    pub fn set_challenge_ceiling(&mut self, ceiling: usize) {
        self.challenge_ceiling = ceiling.max(1);
    }

    /// Challenges outstanding, across administrators.
    pub fn outstanding(&self) -> usize {
        self.challenges.len()
    }

    /// Challenges outstanding for `admin`.
    pub fn outstanding_for(&self, admin: &AdminId) -> usize {
        self.challenges
            .values()
            .filter(|c| c.admin == *admin)
            .count()
    }

    /// `POST /api/v1/admin/challenge` — a fresh, single-use challenge to sign, bound to
    /// the administrator the body `{"admin": ...}` names. [VLD-ANB-MIDGARD-0020]
    pub fn challenge(&mut self, body: &Json, now: u64) -> Result<Json, AdminError> {
        let at = "the challenge request";
        only_keys(
            object(body, at).map_err(|e| refuse(400, e))?,
            at,
            &["admin"],
        )
        .map_err(|e| refuse(400, e))?;
        let admin = AdminId(
            text(body, at, "admin")
                .map_err(|e| refuse(400, e))?
                .to_string(),
        );

        self.challenges.retain(|_, c| c.expires > now);
        let challenge = AdminChallenge::sample(&mut OsRng);
        // A name with no passkey could never answer; it gets a nonce all the same, so
        // the reply does not say whether the name exists, but nothing is kept for it.
        if self.passkeys.admins().contains(&admin) {
            let held = self.outstanding_for(&admin);
            if held == 0 && self.challenges.len() >= self.challenge_ceiling {
                return Err(refuse(429, "too many outstanding challenges"));
            }
            // Make room from this administrator's own challenges, never another's.
            if held >= MAX_CHALLENGES_PER_ADMIN || self.challenges.len() >= self.challenge_ceiling {
                let oldest = self
                    .challenges
                    .iter()
                    .filter(|(_, c)| c.admin == admin)
                    .min_by_key(|(_, c)| c.expires)
                    .map(|(nonce, _)| *nonce);
                if let Some(nonce) = oldest {
                    self.challenges.remove(&nonce);
                }
            }
            self.challenges.insert(
                challenge.nonce,
                Challenge {
                    admin,
                    expires: now.saturating_add(CHALLENGE_TTL),
                },
            );
        }
        Ok(Json::obj(vec![
            ("challenge", Json::s(&hex_encode(&challenge.nonce))),
            ("expiresIn", Json::Int(CHALLENGE_TTL as i64)),
        ]))
    }

    /// Count a challenge refused at the ceiling. `Some(suppressed)` when it is to be
    /// recorded — the first, or the first a minute after the last recorded — with the
    /// refusals counted since; `None` within the minute. [VLD-ANB-MIDGARD-0020]
    pub fn note_challenge_refusal(&mut self, now: u64) -> Option<u64> {
        let (last, suppressed) = &mut self.ceiling_refusals;
        match *last {
            Some(at) if now < at.saturating_add(REFUSAL_RECORD_INTERVAL) => {
                *suppressed += 1;
                None
            }
            _ => {
                *last = Some(now);
                Some(std::mem::take(suppressed))
            }
        }
    }

    /// `POST /api/v1/admin/enrolments` — request that a principal be enrolled. The
    /// request is parked for a second administrator; `enrolled` says whether the
    /// subject already exists, which refuses it, and `catalogue` the roles and
    /// purposes it may grant. Only the body's shape is checked before the
    /// administrator proves themselves; the catalogue and the directory are consulted
    /// after the step-up, so a caller who cannot sign in learns nothing of either.
    /// Returns the response and the requester, for the gateway's own audit record.
    /// [VLD-ANB-MIDGARD-0002]
    pub fn request_enrolment(
        &mut self,
        body: &Json,
        catalogue: &RoleCatalogue,
        enrolled: &dyn Fn(&str) -> bool,
        now: u64,
    ) -> Result<(Json, AdminActor), AdminError> {
        let at = "the request";
        only_keys(
            object(body, at).map_err(|e| refuse(400, e))?,
            at,
            &[
                "admin",
                "challenge",
                "assertion",
                "totp",
                "justification",
                "principal",
            ],
        )
        .map_err(|e| refuse(400, e))?;
        let principal = parse_principal(body.get("principal"))?;
        let justification = text(body, at, "justification")
            .map_err(|e| refuse(400, e))?
            .to_string();

        let scope = ResourceScope::all(IAM_SCOPE);
        let (admin, challenge, assertion) = self.proof(body, now)?;
        let mut session = self.sign_in(body, &admin, &challenge, &assertion, now)?;
        self.passkeys
            .authenticator()
            .step_up(&mut session, scope.clone(), &challenge, &assertion, now)
            .map_err(|e| refuse(401, e.to_string()))?;

        within_catalogue(&principal, catalogue)?;
        if enrolled(&principal.subject) {
            return Err(refuse(
                409,
                format!("{} is already enrolled", principal.subject),
            ));
        }
        if self.parked.values().any(|p| p.subject == principal.subject) {
            return Err(refuse(
                409,
                format!(
                    "an enrolment of {} is already waiting for approval",
                    principal.subject
                ),
            ));
        }

        // A subject rejected and requested again within the second still gets an id the
        // privilege service has not seen.
        let base = format!("enrol-{}-{now}", principal.subject);
        let id = (1..)
            .map(|n| match n {
                1 => base.clone(),
                n => format!("{base}-{n}"),
            })
            .find(|id| self.privilege.service().record(id).is_none())
            .expect("an unused id");
        let disposition = self
            .privilege
            .submit(
                self.roles.directory(),
                &session,
                AdminActionRequest {
                    id: id.clone(),
                    requester: admin.clone(),
                    action_class: ActionClass::PrincipalEnroll,
                    scope,
                    justification,
                },
                now,
                STEP_UP_MAX_AGE,
                &now.to_string(),
            )
            .map_err(|e| refuse(403, format!("{e:?}")))?;
        if disposition != Disposition::NeedsApproval {
            // Enrolment is high-risk by construction; anything else is a broken catalogue.
            return Err(refuse(
                500,
                "principal enrolment was not parked for approval",
            ));
        }
        self.journal
            .append(&encode(&id, Some(&principal)))
            .map_err(|e| refuse(500, format!("cannot record the parked enrolment: {e}")))?;
        let subject = principal.subject.clone();
        let clearance = band(policy::ClassificationLevel(principal.level));
        self.parked.insert(id.clone(), principal);
        let actor = AdminActor::of(&session);

        Ok((
            Json::obj(vec![
                ("id", Json::s(&id)),
                ("subject", Json::s(&subject)),
                ("clearance", Json::s(clearance)),
                ("state", Json::s("waiting for a second administrator")),
            ]),
            actor,
        ))
    }

    /// `POST /api/v1/admin/enrolments/approve` — a second administrator approves a
    /// parked enrolment, naming the subject they approve; `enrol` writes it to the
    /// identity directory with credentials minted here, the activation code hashed
    /// with `hasher`. The response carries the activation code, once, and never the
    /// second factor. An enrolment approved before but not written is completed by
    /// its original approver without a second privilege approval. Whether `id` is
    /// waiting is told only to an administrator who has re-authenticated and is
    /// qualified to approve. Returns the response, the approver and the subject, for
    /// the gateway's own audit record. [VLD-ANB-MIDGARD-0002] [VLD-ANB-MIDGARD-0066]
    pub fn approve_enrolment(
        &mut self,
        body: &Json,
        hasher: &dyn PasswordHasher,
        enrol: &mut dyn FnMut(&str, Enrollment) -> Result<(), String>,
        now: u64,
    ) -> Result<(Json, AdminActor, String), AdminError> {
        let at = "the approval";
        only_keys(
            object(body, at).map_err(|e| refuse(400, e))?,
            at,
            &["admin", "challenge", "assertion", "totp", "id", "subject"],
        )
        .map_err(|e| refuse(400, e))?;
        let id = text(body, at, "id")
            .map_err(|e| refuse(400, e))?
            .to_string();
        let named = text(body, at, "subject")
            .map_err(|e| refuse(400, e))?
            .to_string();

        let (admin, session) = self.reauthenticated(body, now)?;
        self.require_enrolment_capability(&admin, now)?;
        let enrolment = self.waiting(&id)?.clone();
        let subject = enrolment.subject.clone();
        if named != subject {
            return Err(refuse(409, format!("{id} enrols {subject}, not {named}")));
        }

        match self.privilege.service().state_of(&id) {
            // Approved before, but the directory write failed: only the administrator
            // who approved it may complete it, still qualified, with the fresh
            // re-authentication just checked. [VLD-ANB-MIDGARD-0066]
            Some(ActionState::Authorised {
                approver: Some(original),
            }) => {
                if *original != admin {
                    return Err(refuse(
                        403,
                        format!(
                            "{id} was approved by {}; only they may complete it",
                            original.0
                        ),
                    ));
                }
            }
            _ => self
                .privilege
                .approve(
                    self.roles.directory(),
                    &session,
                    &id,
                    now,
                    REAUTH_MAX_AGE,
                    &now.to_string(),
                )
                .map_err(|e| refuse(403, format!("{e:?}")))?,
        }

        let (enrollment, code) = enrolment.mint(hasher, now);
        enrol(&subject, enrollment).map_err(|e| {
            refuse(
                500,
                format!(
                    "approved, but the enrolment was not written: {e}; {} may approve it again",
                    admin.0
                ),
            )
        })?;
        self.settle(&id)
            .map_err(|e| refuse(500, format!("enrolled, but not marked settled: {e}")))?;

        Ok((
            Json::obj(vec![
                ("id", Json::s(&id)),
                ("enrolled", Json::s(&subject)),
                ("approvedBy", Json::s(&admin.0)),
                ("activationCode", Json::s(&code)),
                (
                    "activationExpiresAt",
                    Json::Int(now.saturating_add(ACTIVATION_TTL) as i64),
                ),
            ]),
            AdminActor::of(&session),
            subject,
        ))
    }

    /// `POST /api/v1/admin/enrolments/reject` — an administrator qualified to approve
    /// enrolments, other than the requester, rejects a parked enrolment with a reason.
    /// It is settled and purged, and the subject may be requested again. As at
    /// approval, whether `id` is waiting is told only to a qualified administrator who
    /// has re-authenticated. Returns the response, the administrator and the subject,
    /// for the gateway's own audit record. [VLD-ANB-MIDGARD-0066]
    pub fn reject_enrolment(
        &mut self,
        body: &Json,
        now: u64,
    ) -> Result<(Json, AdminActor, String), AdminError> {
        let at = "the rejection";
        only_keys(
            object(body, at).map_err(|e| refuse(400, e))?,
            at,
            &["admin", "challenge", "assertion", "totp", "id", "reason"],
        )
        .map_err(|e| refuse(400, e))?;
        let id = text(body, at, "id")
            .map_err(|e| refuse(400, e))?
            .to_string();
        let reason = text(body, at, "reason")
            .map_err(|e| refuse(400, e))?
            .trim()
            .to_string();
        if reason.is_empty() {
            return Err(refuse(400, "\"reason\" must say why"));
        }

        let (admin, session) = self.reauthenticated(body, now)?;
        self.require_enrolment_capability(&admin, now)?;
        let subject = self.waiting(&id)?.subject.clone();
        match self.privilege.service().state_of(&id) {
            // Rejected before, but the settlement was not recorded: settle it now.
            Some(ActionState::Rejected { .. }) => {}
            _ => self
                .privilege
                .reject(&session, &id, &reason, &now.to_string())
                .map_err(|e| refuse(403, format!("{e:?}")))?,
        }
        self.settle(&id)
            .map_err(|e| refuse(500, format!("rejected, but not marked settled: {e}")))?;

        Ok((
            Json::obj(vec![
                ("id", Json::s(&id)),
                ("rejected", Json::s(&subject)),
                ("rejectedBy", Json::s(&admin.0)),
                ("reason", Json::s(&reason)),
            ]),
            AdminActor::of(&session),
            subject,
        ))
    }

    /// `POST /api/v1/admin/enrolments/list` — the parked enrolments, in full, for any
    /// registered administrator who signs a challenge: everything an approver
    /// approves. [VLD-ANB-MIDGARD-0002]
    pub fn list_enrolments(&mut self, body: &Json, now: u64) -> Result<Json, AdminError> {
        let at = "the listing";
        only_keys(
            object(body, at).map_err(|e| refuse(400, e))?,
            at,
            &["admin", "challenge", "assertion", "totp"],
        )
        .map_err(|e| refuse(400, e))?;
        let (admin, challenge, assertion) = self.proof(body, now)?;
        self.sign_in(body, &admin, &challenge, &assertion, now)?;
        let names = |values: &[String]| Json::Arr(values.iter().map(|v| Json::s(v)).collect());
        let rows = self
            .parked
            .iter()
            .map(|(id, p)| {
                let record = self.privilege.service().record(id);
                let requester = record
                    .map(|r| r.request.requester.0.clone())
                    .unwrap_or_default();
                let justification = record
                    .map(|r| r.request.justification.clone())
                    .unwrap_or_default();
                let state = match record.map(|r| &r.state) {
                    Some(ActionState::Authorised {
                        approver: Some(approver),
                    }) => format!(
                        "approved by {0} but not written; {0} may approve it again",
                        approver.0
                    ),
                    Some(ActionState::Rejected { .. }) => "rejected".to_string(),
                    _ => "waiting for a second administrator".to_string(),
                };
                Json::obj(vec![
                    ("id", Json::s(id)),
                    ("subject", Json::s(&p.subject)),
                    ("requestedBy", Json::s(&requester)),
                    ("roles", names(&p.roles)),
                    (
                        "clearance",
                        Json::s(band(policy::ClassificationLevel(p.level))),
                    ),
                    ("compartments", names(&p.compartments)),
                    ("purposes", names(&p.purposes)),
                    ("justification", Json::s(&justification)),
                    ("mfa", Json::s(MFA_AT_ACTIVATION)),
                    ("state", Json::s(&state)),
                ])
            })
            .collect();
        Ok(Json::obj(vec![("enrolments", Json::Arr(rows))]))
    }

    /// Sign in the presenting administrator and re-authenticate them freshly: what an
    /// approval or a rejection needs.
    fn reauthenticated(
        &mut self,
        body: &Json,
        now: u64,
    ) -> Result<(AdminId, AdminSession), AdminError> {
        let (admin, challenge, assertion) = self.proof(body, now)?;
        let mut session = self.sign_in(body, &admin, &challenge, &assertion, now)?;
        self.passkeys
            .authenticator()
            .reauth(&mut session, &challenge, &assertion, now)
            .map_err(|e| refuse(401, e.to_string()))?;
        Ok((admin, session))
    }

    /// Refuse an administrator who does not hold the capability that qualifies an
    /// approver of a principal enrolment in the `iam` scope now, as the privilege
    /// service requires of an approver. [MG-SEC-07]
    fn require_enrolment_capability(&self, admin: &AdminId, now: u64) -> Result<(), AdminError> {
        let capability = ActionClass::PrincipalEnroll.required_capability();
        if self.roles.directory().has_capability(
            admin,
            capability,
            &ResourceScope::all(IAM_SCOPE),
            now,
        ) {
            Ok(())
        } else {
            Err(refuse(
                403,
                format!("ApproverNotQualified {{ capability: \"{capability}\" }}"),
            ))
        }
    }

    /// The enrolment `id` parked, or a 404. Asked only once the caller has proved
    /// themselves and is qualified, so the answer is no oracle. [VLD-ANB-MIDGARD-0066]
    fn waiting(&self, id: &str) -> Result<&ParkedEnrolment, AdminError> {
        self.parked
            .get(id)
            .ok_or_else(|| refuse(404, format!("no enrolment {id} is waiting")))
    }

    /// Record `id` settled, then forget it.
    fn settle(&mut self, id: &str) -> Result<(), AuditError> {
        self.journal.append(&encode(id, None))?;
        self.parked.remove(id);
        Ok(())
    }

    /// The admin, and the challenge and assertion they present, with the challenge
    /// consumed so it can never be answered twice. A challenge issued to another
    /// administrator is refused like an unknown one, and consumed all the same.
    /// [VLD-ANB-MIDGARD-0020]
    fn proof(
        &mut self,
        body: &Json,
        now: u64,
    ) -> Result<(AdminId, AdminChallenge, WebAuthnAssertion), AdminError> {
        let admin = AdminId(
            text(body, "the request", "admin")
                .map_err(|e| refuse(400, e))?
                .to_string(),
        );
        let nonce: [u8; 32] =
            hex_decode(text(body, "the request", "challenge").map_err(|e| refuse(400, e))?)
                .ok()
                .and_then(|b| b.try_into().ok())
                .ok_or_else(|| refuse(400, "\"challenge\" must be 64 hex characters"))?;
        match self.challenges.remove(&nonce) {
            Some(c) if c.expires > now && c.admin == admin => {}
            _ => return Err(refuse(401, "the challenge is unknown, used or expired")),
        }

        let at = "the assertion";
        let item = body
            .get("assertion")
            .ok_or_else(|| refuse(400, "missing \"assertion\""))?;
        only_keys(
            object(item, at).map_err(|e| refuse(400, e))?,
            at,
            &["credentialId", "userVerified", "signature"],
        )
        .map_err(|e| refuse(400, e))?;
        let signature = hex_decode(text(item, at, "signature").map_err(|e| refuse(400, e))?)
            .map_err(|()| refuse(400, "\"signature\" must be hex"))?;
        let assertion = WebAuthnAssertion {
            credential_id: text(item, at, "credentialId")
                .map_err(|e| refuse(400, e))?
                .to_string(),
            user_verified: optional_bool(item, at, "userVerified", false)
                .map_err(|e| refuse(400, e))?,
            signature: Signature::new(PASSKEY_SIG_ALG, signature),
        };
        Ok((admin, AdminChallenge::new(nonce), assertion))
    }

    /// Sign the administrator in with both factors: the passkey assertion, then the
    /// one-time code in `body`'s `totp`, checked here and spent. The code is looked at
    /// only once the passkey has verified, so nobody without the passkey can spend an
    /// administrator's codes. A missing, malformed, wrong or reused code is one 401.
    /// [VLD-ANB-MIDGARD-0008]
    fn sign_in(
        &mut self,
        body: &Json,
        admin: &AdminId,
        challenge: &AdminChallenge,
        assertion: &WebAuthnAssertion,
        now: u64,
    ) -> Result<AdminSession, AdminError> {
        let mut session = self
            .passkeys
            .authenticator()
            .sign_in(admin, challenge, assertion, now)
            .map_err(|e| refuse(401, e.to_string()))?;
        let code = match body.get("totp") {
            Some(Json::Str(code)) => code.as_str(),
            _ => return Err(refuse(401, SECOND_FACTOR_REQUIRED)),
        };
        self.totp
            .verify(&admin.0, code, now)
            .map_err(|_| refuse(401, SECOND_FACTOR_REQUIRED))?;
        session.record_factor(AuthFactor::Totp);
        Ok(session)
    }
}

/// The principal an enrolment request describes, its shape checked before anyone
/// signs in; what it grants is checked against the served policy by
/// [`within_catalogue`] only once the requester has stepped up. It carries no
/// credential material: a password hash or TOTP secret is an unknown key, refused like
/// any other. [VLD-ANB-MIDGARD-0002]
fn parse_principal(item: Option<&Json>) -> Result<ParkedEnrolment, AdminError> {
    let at = "the principal";
    let item = item.ok_or_else(|| refuse(400, "missing \"principal\""))?;
    only_keys(
        object(item, at).map_err(|e| refuse(400, e))?,
        at,
        &["subject", "roles", "clearance", "compartments", "purposes"],
    )
    .map_err(|e| refuse(400, e))?;

    let subject = text(item, at, "subject").map_err(|e| refuse(400, e))?;
    if subject.chars().count() > 64 || subject.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(refuse(
            400,
            "\"subject\" must be at most 64 characters, with no whitespace",
        ));
    }
    let band_name = text(item, at, "clearance").map_err(|e| refuse(400, e))?;
    let level = level_of(band_name).ok_or_else(|| {
        refuse(
            400,
            format!("\"clearance\" must be open, internal, restricted or confidential, not \"{band_name}\""),
        )
    })?;
    let list = |key: &str, required: bool| -> Result<Vec<String>, AdminError> {
        match item.get(key) {
            None if !required => Ok(Vec::new()),
            _ => {
                let values = strings(item, at, key).map_err(|e| refuse(400, e))?;
                let unique: BTreeSet<&String> = values.iter().collect();
                if required && unique.is_empty() {
                    return Err(refuse(400, format!("\"{key}\" needs at least one value")));
                }
                Ok(unique.into_iter().cloned().collect())
            }
        }
    };
    let roles = list("roles", true)?;
    let purposes = list("purposes", true)?;
    let compartments = list("compartments", false)?;
    if let Some(bad) = compartments.iter().find(|c| {
        c.is_empty()
            || c.len() > MAX_COMPARTMENT_LEN
            || !c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    }) {
        return Err(refuse(
            400,
            format!(
                "\"compartments\" must each be 1 to {MAX_COMPARTMENT_LEN} letters, digits or \
                 hyphens, not \"{bad}\""
            ),
        ));
    }

    Ok(ParkedEnrolment {
        subject: subject.to_string(),
        roles,
        level: level.0,
        compartments,
        purposes,
    })
}

/// Refuse a principal granted a role or purpose the served policy does not name; the
/// refusal says what it does name. Told only to a requester who has stepped up.
/// [VLD-ANB-MIDGARD-0002]
fn within_catalogue(
    principal: &ParkedEnrolment,
    catalogue: &RoleCatalogue,
) -> Result<(), AdminError> {
    let within = |key: &str, values: &[String], allowed: Vec<&str>| match values
        .iter()
        .find(|v| !allowed.contains(&v.as_str()))
    {
        None => Ok(()),
        Some(unknown) => Err(refuse(
            400,
            format!(
                "\"{key}\" may only name what the served policy names ({}), not \"{unknown}\"",
                allowed.join(", ")
            ),
        )),
    };
    within("roles", &principal.roles, catalogue.roles().collect())?;
    within(
        "purposes",
        &principal.purposes,
        catalogue.purposes().collect(),
    )
}

// ---- parked-enrolment journal ----------------------------------------------------------

/// One sealer behind several journal opens: opening the journal under the wrong
/// domain consumes the sealer it was given, and the version-1 path needs a second try.
struct SharedSealer(Arc<dyn Sealer>);

impl Sealer for SharedSealer {
    fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
        self.0.seal(aad, plaintext)
    }

    fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
        self.0.unseal(aad, sealed)
    }
}

type Decoder = fn(&[u8]) -> Result<(String, Option<ParkedEnrolment>), String>;

/// The parked enrolments a journal's records leave waiting.
fn replay(
    records: &[Vec<u8>],
    decode: Decoder,
) -> Result<BTreeMap<String, ParkedEnrolment>, String> {
    let mut parked = BTreeMap::new();
    for record in records {
        match decode(record)? {
            (id, Some(enrolment)) => {
                parked.insert(id, enrolment);
            }
            (id, None) => {
                parked.remove(&id);
            }
        }
    }
    Ok(parked)
}

/// Open the parked-enrolment journal at `path`. A journal an earlier build sealed
/// under the version-1 domain, with credential material in its records, is replayed
/// without that material and rewritten under version 2: the new journal is written
/// beside it and renamed over it, so a crash leaves one or the other whole.
/// [VLD-ANB-MIDGARD-0002]
fn open_pending(
    path: &Path,
    sealer: Box<dyn Sealer>,
) -> Result<(SealedJournal, BTreeMap<String, ParkedEnrolment>), String> {
    let sealer: Arc<dyn Sealer> = Arc::from(sealer);
    let shared = || -> Box<dyn Sealer> { Box::new(SharedSealer(Arc::clone(&sealer))) };
    let refused = |e: AuditError| format!("the pending enrolments do not open: {e}");

    let current = match SealedJournal::open(path, shared(), PENDING_DOMAIN) {
        Ok((journal, records)) => return Ok((journal, replay(&records, decode)?)),
        Err(e) => e,
    };
    let Ok((earlier, records)) = SealedJournal::open(path, shared(), PENDING_DOMAIN_V1) else {
        return Err(refused(current));
    };
    drop(earlier);
    let parked = replay(&records, decode_v1)?;

    let staging = path.with_extension("v2-staging");
    let _ = std::fs::remove_file(&staging);
    {
        let (mut journal, _) =
            SealedJournal::open(&staging, shared(), PENDING_DOMAIN).map_err(refused)?;
        for (id, enrolment) in &parked {
            journal
                .append(&encode(id, Some(enrolment)))
                .map_err(|e| format!("cannot rewrite the pending enrolments: {e}"))?;
        }
    }
    std::fs::rename(&staging, path)
        .map_err(|e| format!("cannot replace {}: {e}", path.display()))?;
    let (journal, records) =
        SealedJournal::open(path, shared(), PENDING_DOMAIN).map_err(refused)?;
    Ok((journal, replay(&records, decode)?))
}

fn put(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

fn put_list(o: &mut Vec<u8>, items: &[String]) {
    o.extend_from_slice(&(items.len() as u32).to_le_bytes());
    for item in items {
        put(o, item.as_bytes());
    }
}

/// One journal record: an enrolment parked under `id`, or `id` settled.
fn encode(id: &str, parked: Option<&ParkedEnrolment>) -> Vec<u8> {
    let mut o = Vec::new();
    match parked {
        None => {
            o.push(OP_SETTLED);
            put(&mut o, id.as_bytes());
        }
        Some(p) => {
            o.push(OP_PARKED);
            put(&mut o, id.as_bytes());
            put(&mut o, p.subject.as_bytes());
            put_list(&mut o, &p.roles);
            o.push(p.level);
            put_list(&mut o, &p.compartments);
            put_list(&mut o, &p.purposes);
        }
    }
    o
}

struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&end| end <= self.b.len())
            .ok_or("pending enrolment record: truncated")?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<usize, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }

    fn bytes(&mut self) -> Result<Vec<u8>, String> {
        let n = self.u32()?;
        Ok(self.take(n)?.to_vec())
    }

    fn text(&mut self) -> Result<String, String> {
        String::from_utf8(self.bytes()?)
            .map_err(|_| "pending enrolment record: not UTF-8".to_string())
    }

    fn list(&mut self) -> Result<Vec<String>, String> {
        (0..self.u32()?).map(|_| self.text()).collect()
    }

    fn enrolment(&mut self) -> Result<ParkedEnrolment, String> {
        Ok(ParkedEnrolment {
            subject: self.text()?,
            roles: self.list()?,
            level: self.u8()?,
            compartments: self.list()?,
            purposes: self.list()?,
        })
    }

    fn end(&self) -> Result<(), String> {
        if self.at == self.b.len() {
            Ok(())
        } else {
            Err("pending enrolment record: trailing bytes".into())
        }
    }
}

fn decode(b: &[u8]) -> Result<(String, Option<ParkedEnrolment>), String> {
    let mut c = Cursor { b, at: 0 };
    let op = c.u8()?;
    let id = c.text()?;
    let parked = match op {
        OP_SETTLED => None,
        OP_PARKED => Some(c.enrolment()?),
        _ => return Err("pending enrolment record: unknown operation".into()),
    };
    c.end()?;
    Ok((id, parked))
}

/// A version-1 record: as version 2, then the requester's password hash and any TOTP
/// secret, which are read past and discarded. [VLD-ANB-MIDGARD-0002]
fn decode_v1(b: &[u8]) -> Result<(String, Option<ParkedEnrolment>), String> {
    let mut c = Cursor { b, at: 0 };
    let op = c.u8()?;
    let id = c.text()?;
    let parked = match op {
        OP_SETTLED => None,
        OP_PARKED => {
            let enrolment = c.enrolment()?;
            let _password_hash = c.bytes()?;
            match c.u8()? {
                0 => {}
                1 => {
                    let _totp_secret = c.bytes()?;
                }
                _ => return Err("pending enrolment record: bad TOTP tag".into()),
            }
            Some(enrolment)
        }
        _ => return Err("pending enrolment record: unknown operation".into()),
    };
    c.end()?;
    Ok((id, parked))
}

#[cfg(all(test, feature = "backend-fips"))]
mod tests {
    use super::*;
    use kms::{Kms, KmsSealer, SoftwareTrustRoot};
    use pqc_agility::backend::fips::provider as fips_provider;

    /// The same deterministic trust root on every call, as a restart would reopen it.
    struct SeedRng(u64);
    impl Rng for SeedRng {
        fn fill(&mut self, buf: &mut [u8]) {
            for b in buf {
                self.0 = self
                    .0
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                *b = (self.0 >> 33) as u8;
            }
        }
    }

    fn sealer() -> Box<dyn Sealer> {
        let root =
            SoftwareTrustRoot::generate(&fips_provider(), "pending-root", &mut SeedRng(7)).unwrap();
        Box::new(KmsSealer::new(Kms::new(fips_provider(), root)))
    }

    fn v1_parked(id: &str, subject: &str) -> Vec<u8> {
        let mut o = encode(
            id,
            Some(&ParkedEnrolment {
                subject: subject.into(),
                roles: vec!["operator".into()],
                level: 1,
                compartments: vec![],
                purposes: vec!["operations".into()],
            }),
        );
        put(&mut o, b"$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA");
        o.push(1);
        put(&mut o, &[0u8; 20]);
        o
    }

    /// [VLD-ANB-MIDGARD-0002] [RC-29] A journal an earlier build wrote is replayed
    /// without its credential material, rewritten under version 2, and opens as
    /// version 2 from then on.
    #[test]
    fn a_v1_pending_journal_is_re_parked_without_credentials() {
        let path = std::env::temp_dir().join(format!("pending-v1-{}.journal", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let (mut journal, _) = SealedJournal::open(&path, sealer(), PENDING_DOMAIN_V1).unwrap();
            journal
                .append(&v1_parked("enrol-olivia-1", "olivia"))
                .unwrap();
            journal
                .append(&v1_parked("enrol-oscar-2", "oscar"))
                .unwrap();
            journal.append(&encode("enrol-oscar-2", None)).unwrap();
        }

        let (_, parked) = open_pending(&path, sealer()).expect("a v1 journal opens");
        assert_eq!(parked.keys().collect::<Vec<_>>(), ["enrol-olivia-1"]);
        assert_eq!(parked["enrol-olivia-1"].subject, "olivia");

        let (_, records) = SealedJournal::open(&path, sealer(), PENDING_DOMAIN)
            .expect("the journal is now version 2");
        assert_eq!(records.len(), 1);
        assert!(!records[0].windows(9).any(|w| w == b"$argon2id"));
        let (_, again) = open_pending(&path, sealer()).unwrap();
        assert_eq!(again, parked);
        let _ = std::fs::remove_file(&path);
    }
}
