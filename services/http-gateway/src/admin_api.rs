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
//! The new principal's password never reaches the gateway. The requester's tool hashes
//! it and sends the PBKDF2 hash, which is refused below the production work factor. A
//! parked enrolment — hash and any TOTP secret included — is sealed at rest, so it
//! survives a restart until it is approved.

use std::collections::{BTreeMap, BTreeSet};

use admin_identity::{
    AdminChallenge, AdminId, AdminSession, DurableAdminPasskeys, DurableIdentityDirectory,
    ResourceScope, WebAuthnAssertion, PASSKEY_SIG_ALG,
};
use admin_privilege::{ActionClass, AdminActionRequest, Disposition, DurablePrivilege};
use audit_writer::{SealedJournal, Sealer};
use iam::{Credentials, Enrollment, TotpSecret, UserRecord, DEFAULT_ITERATIONS};
use policy::{Clearance, Compartment, Purpose, Role};
use pqc_agility::{OsRng, Signature};

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
/// The most challenges outstanding at once; the oldest is dropped past this.
const MAX_CHALLENGES: usize = 256;
/// The scope every principal enrolment is requested in.
const IAM_SCOPE: &str = "iam";
const PENDING_DOMAIN: &[u8] = b"midgard-admin-pending-enrolments-v1";
const OP_PARKED: u8 = 0;
const OP_SETTLED: u8 = 1;

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

/// A principal as an administrator asked for them to be enrolled.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParkedEnrolment {
    subject: String,
    roles: Vec<String>,
    level: u8,
    compartments: Vec<String>,
    purposes: Vec<String>,
    password_hash: String,
    totp_secret: Option<Vec<u8>>,
}

impl ParkedEnrolment {
    fn into_enrollment(self) -> Enrollment {
        Enrollment {
            user: UserRecord {
                roles: self.roles.into_iter().map(Role).collect(),
                clearance: Clearance {
                    level: policy::ClassificationLevel(self.level),
                    compartments: self.compartments.into_iter().map(Compartment).collect(),
                },
                purposes: self.purposes.into_iter().map(Purpose).collect(),
            },
            credentials: Credentials {
                password_hash: self.password_hash,
                totp: self.totp_secret.map(TotpSecret::new),
                ..Credentials::default()
            },
        }
    }
}

/// Administration of a served enclosure: passkeys, role bindings, the dual-control
/// service, and the enrolments it has parked.
pub struct AdminControl {
    passkeys: DurableAdminPasskeys,
    roles: DurableIdentityDirectory,
    privilege: DurablePrivilege,
    journal: SealedJournal,
    parked: BTreeMap<String, ParkedEnrolment>,
    challenges: BTreeMap<[u8; 32], u64>,
}

impl AdminControl {
    /// Assemble administration over its durable parts, replaying the parked enrolments
    /// sealed in `pending_path`.
    pub fn open(
        passkeys: DurableAdminPasskeys,
        roles: DurableIdentityDirectory,
        privilege: DurablePrivilege,
        pending_path: impl AsRef<std::path::Path>,
        pending_sealer: Box<dyn Sealer>,
    ) -> Result<Self, String> {
        let (journal, records) = SealedJournal::open(pending_path, pending_sealer, PENDING_DOMAIN)
            .map_err(|e| format!("the pending enrolments do not open: {e}"))?;
        let mut parked = BTreeMap::new();
        for record in &records {
            match decode(record)? {
                (id, Some(enrolment)) => {
                    parked.insert(id, enrolment);
                }
                (id, None) => {
                    parked.remove(&id);
                }
            }
        }
        Ok(Self {
            passkeys,
            roles,
            privilege,
            journal,
            parked,
            challenges: BTreeMap::new(),
        })
    }

    /// Administrators with a registered passkey.
    pub fn administrators(&self) -> usize {
        self.passkeys.admins().len()
    }

    /// Enrolments waiting for a second administrator.
    pub fn parked(&self) -> usize {
        self.parked.len()
    }

    /// `POST /api/v1/admin/challenge` — a fresh, single-use challenge to sign.
    pub fn challenge(&mut self, now: u64) -> Json {
        self.challenges.retain(|_, expires| *expires > now);
        while self.challenges.len() >= MAX_CHALLENGES {
            let oldest = self
                .challenges
                .iter()
                .min_by_key(|(_, expires)| **expires)
                .map(|(nonce, _)| *nonce);
            match oldest {
                Some(nonce) => {
                    self.challenges.remove(&nonce);
                }
                None => break,
            }
        }
        let challenge = AdminChallenge::sample(&mut OsRng);
        self.challenges
            .insert(challenge.nonce, now.saturating_add(CHALLENGE_TTL));
        Json::obj(vec![
            ("challenge", Json::s(&hex_encode(&challenge.nonce))),
            ("expiresIn", Json::Int(CHALLENGE_TTL as i64)),
        ])
    }

    /// `POST /api/v1/admin/enrolments` — request that a principal be enrolled. The
    /// request is parked for a second administrator; `enrolled` says whether the
    /// subject already exists, which refuses it. Returns the response and the
    /// requester, for the gateway's own audit record.
    pub fn request_enrolment(
        &mut self,
        body: &Json,
        enrolled: &dyn Fn(&str) -> bool,
        now: u64,
    ) -> Result<(Json, String), AdminError> {
        let at = "the request";
        only_keys(
            object(body, at).map_err(|e| refuse(400, e))?,
            at,
            &[
                "admin",
                "challenge",
                "assertion",
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
        let mut session = self.sign_in(&admin, &challenge, &assertion, now)?;
        self.passkeys
            .authenticator()
            .step_up(&mut session, scope.clone(), &challenge, &assertion, now)
            .map_err(|e| refuse(401, e.to_string()))?;

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

        let id = format!("enrol-{}-{now}", principal.subject);
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

        Ok((
            Json::obj(vec![
                ("id", Json::s(&id)),
                ("subject", Json::s(&subject)),
                ("clearance", Json::s(clearance)),
                ("state", Json::s("waiting for a second administrator")),
            ]),
            admin.0,
        ))
    }

    /// `POST /api/v1/admin/enrolments/approve` — a second administrator approves a
    /// parked enrolment; `enrol` writes it to the identity directory. Returns the
    /// response, the approver and the subject, for the gateway's own audit record.
    pub fn approve_enrolment(
        &mut self,
        body: &Json,
        enrol: &mut dyn FnMut(&str, Enrollment) -> Result<(), String>,
        now: u64,
    ) -> Result<(Json, String, String), AdminError> {
        let at = "the approval";
        only_keys(
            object(body, at).map_err(|e| refuse(400, e))?,
            at,
            &["admin", "challenge", "assertion", "id"],
        )
        .map_err(|e| refuse(400, e))?;
        let id = text(body, at, "id")
            .map_err(|e| refuse(400, e))?
            .to_string();
        if !self.parked.contains_key(&id) {
            return Err(refuse(404, format!("no enrolment {id} is waiting")));
        }

        let (admin, challenge, assertion) = self.proof(body, now)?;
        let mut session = self.sign_in(&admin, &challenge, &assertion, now)?;
        self.passkeys
            .authenticator()
            .reauth(&mut session, &challenge, &assertion, now)
            .map_err(|e| refuse(401, e.to_string()))?;

        self.privilege
            .approve(
                self.roles.directory(),
                &session,
                &id,
                now,
                REAUTH_MAX_AGE,
                &now.to_string(),
            )
            .map_err(|e| refuse(403, format!("{e:?}")))?;

        let enrolment = self.parked.get(&id).cloned().expect("checked above");
        let subject = enrolment.subject.clone();
        enrol(&subject, enrolment.into_enrollment()).map_err(|e| {
            refuse(
                500,
                format!("approved, but the enrolment was not written: {e}"),
            )
        })?;
        self.journal
            .append(&encode(&id, None))
            .map_err(|e| refuse(500, format!("enrolled, but not marked settled: {e}")))?;
        self.parked.remove(&id);

        Ok((
            Json::obj(vec![
                ("id", Json::s(&id)),
                ("enrolled", Json::s(&subject)),
                ("approvedBy", Json::s(&admin.0)),
            ]),
            admin.0,
            subject,
        ))
    }

    /// `POST /api/v1/admin/enrolments/list` — the parked enrolments, for any
    /// registered administrator who signs a challenge.
    pub fn list_enrolments(&mut self, body: &Json, now: u64) -> Result<Json, AdminError> {
        let at = "the listing";
        only_keys(
            object(body, at).map_err(|e| refuse(400, e))?,
            at,
            &["admin", "challenge", "assertion"],
        )
        .map_err(|e| refuse(400, e))?;
        let (admin, challenge, assertion) = self.proof(body, now)?;
        self.sign_in(&admin, &challenge, &assertion, now)?;
        let rows = self
            .parked
            .iter()
            .map(|(id, p)| {
                let requester = self
                    .privilege
                    .service()
                    .record(id)
                    .map(|r| r.request.requester.0.clone())
                    .unwrap_or_default();
                Json::obj(vec![
                    ("id", Json::s(id)),
                    ("subject", Json::s(&p.subject)),
                    ("requestedBy", Json::s(&requester)),
                    (
                        "roles",
                        Json::Arr(p.roles.iter().map(|r| Json::s(r)).collect()),
                    ),
                    (
                        "clearance",
                        Json::s(band(policy::ClassificationLevel(p.level))),
                    ),
                ])
            })
            .collect();
        Ok(Json::obj(vec![("enrolments", Json::Arr(rows))]))
    }

    /// The admin, and the challenge and assertion they present, with the challenge
    /// consumed so it can never be answered twice.
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
            Some(expires) if expires > now => {}
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

    fn sign_in(
        &self,
        admin: &AdminId,
        challenge: &AdminChallenge,
        assertion: &WebAuthnAssertion,
        now: u64,
    ) -> Result<AdminSession, AdminError> {
        self.passkeys
            .authenticator()
            .sign_in(admin, challenge, assertion, now)
            .map_err(|e| refuse(401, e.to_string()))
    }
}

/// The principal an enrolment request describes, validated before anyone signs in.
fn parse_principal(item: Option<&Json>) -> Result<ParkedEnrolment, AdminError> {
    let at = "the principal";
    let item = item.ok_or_else(|| refuse(400, "missing \"principal\""))?;
    only_keys(
        object(item, at).map_err(|e| refuse(400, e))?,
        at,
        &[
            "subject",
            "roles",
            "clearance",
            "compartments",
            "purposes",
            "passwordHash",
            "totpSecret",
        ],
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

    let password_hash = text(item, at, "passwordHash")
        .map_err(|e| refuse(400, e))?
        .to_string();
    let iterations = password_hash
        .strip_prefix("pbkdf2-sha256$i=")
        .and_then(|rest| rest.split('$').next())
        .and_then(|i| i.parse::<u32>().ok())
        .ok_or_else(|| refuse(400, "\"passwordHash\" must be a pbkdf2-sha256 hash"))?;
    if iterations < DEFAULT_ITERATIONS {
        return Err(refuse(
            400,
            format!(
                "\"passwordHash\" uses {iterations} iterations; at least {DEFAULT_ITERATIONS} are required"
            ),
        ));
    }
    let totp_secret = match item.get("totpSecret") {
        None => None,
        Some(_) => {
            let secret = hex_decode(text(item, at, "totpSecret").map_err(|e| refuse(400, e))?)
                .map_err(|()| refuse(400, "\"totpSecret\" must be hex"))?;
            if secret.len() < 20 {
                return Err(refuse(400, "\"totpSecret\" must be at least 20 bytes"));
            }
            Some(secret)
        }
    };

    Ok(ParkedEnrolment {
        subject: subject.to_string(),
        roles: list("roles", true)?,
        level: level.0,
        compartments: list("compartments", false)?,
        purposes: list("purposes", true)?,
        password_hash,
        totp_secret,
    })
}

// ---- parked-enrolment journal codec ----------------------------------------------------

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
            put(&mut o, p.password_hash.as_bytes());
            match &p.totp_secret {
                None => o.push(0),
                Some(secret) => {
                    o.push(1);
                    put(&mut o, secret);
                }
            }
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
}

fn decode(b: &[u8]) -> Result<(String, Option<ParkedEnrolment>), String> {
    let mut c = Cursor { b, at: 0 };
    let op = c.u8()?;
    let id = c.text()?;
    let parked = match op {
        OP_SETTLED => None,
        OP_PARKED => Some(ParkedEnrolment {
            subject: c.text()?,
            roles: c.list()?,
            level: c.u8()?,
            compartments: c.list()?,
            purposes: c.list()?,
            password_hash: c.text()?,
            totp_secret: match c.u8()? {
                0 => None,
                1 => Some(c.bytes()?),
                _ => return Err("pending enrolment record: bad TOTP tag".into()),
            },
        }),
        _ => return Err("pending enrolment record: unknown operation".into()),
    };
    if c.at != b.len() {
        return Err("pending enrolment record: trailing bytes".into());
    }
    Ok((id, parked))
}
