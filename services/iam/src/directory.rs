//! The **durable identity directory**: principals with their authorization
//! attributes *and* credentials (password hash + enrolled MFA factors), persisted
//! through the KMS-sealed append-only [`SealedJournal`] and **replayed on boot**. It
//! survives a restart with every enrollment intact and encrypted at rest; a tampered
//! record fails closed on load. [RC-29] [AC-M-4]
//!
//! ## Record format
//! Every record written now is **tagged and versioned**: `0xFF, version, kind, body`.
//! Version 3, kind 0 is a full enrollment followed by its pending activation, if any
//! (a tag, then the code hash, the expiry and whether the TOTP factor is confirmed),
//! then the last accepted TOTP step, if any (a tag, then the step). Version 3, kind 1
//! is a TOTP step: a subject and the step it last logged in with, appended on every
//! accepted code so a login does not rewrite the whole enrollment; replay keeps the
//! greater of the entry's step and the record's. A version 2 enrollment is the same
//! without the step, and a record that does not start with `0xFF` is the original,
//! untagged version 1; both are read unchanged, so a directory written by an earlier
//! build still opens. Their entries have no last accepted step (and version 1 no
//! pending activation). Later fields extend the tagged form rather than breaking it
//! again. [VLD-ANB-MIDGARD-0002] [VLD-ANB-MIDGARD-0026]

use std::collections::BTreeMap;
use std::path::Path;

use audit_writer::{AuditError, SealedJournal, Sealer};
use policy::{ClassificationLevel, Clearance, Compartment, Purpose, Role};
use pqc_agility::{SigId, VerifyingKey};

use crate::mfa::{TotpSecret, WebAuthnCredential};
use crate::store::UserRecord;

const DOMAIN: &[u8] = b"iam-directory-v1";
/// The first byte of a tagged record. An untagged (version 1) record starts with
/// the little-endian length of its subject instead.
const TAGGED: u8 = 0xFF;
/// The tagged record version written now.
const RECORD_VERSION: u8 = 3;
/// The previous tagged version: an enrollment with no last accepted TOTP step.
const RECORD_VERSION_2: u8 = 2;
/// A tagged record holding a full enrollment.
const KIND_ENROLLMENT: u8 = 0;
/// A tagged record holding a principal's last accepted TOTP step.
const KIND_TOTP_STEP: u8 = 1;

/// A principal approved under dual control who has not yet activated their account.
/// The gateway mints the credentials at approval and hands the approver a single-use
/// activation code for the principal; only its hash is stored. Until the principal
/// presents it, sets their password and takes their TOTP secret, the record cannot
/// log in. [VLD-ANB-MIDGARD-0002]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activation {
    /// PHC-encoded hash of the activation code, from the gateway's password hasher.
    pub code_hash: String,
    /// Unix time after which the code no longer activates anything.
    pub expires_at: u64,
    /// Whether the principal has confirmed their TOTP factor with a code.
    pub totp_confirmed: bool,
}

/// A principal's credentials: a password hash and enrolled MFA factors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Credentials {
    /// PHC-encoded password hash (empty = no password set).
    pub password_hash: String,
    /// Enrolled TOTP secret, if any.
    pub totp: Option<TotpSecret>,
    /// Enrolled passkeys.
    pub passkeys: Vec<WebAuthnCredential>,
    /// A pending activation: present from approval until the principal activates the
    /// account, during which no login succeeds. [VLD-ANB-MIDGARD-0002]
    pub activation: Option<Activation>,
    /// The TOTP time step this principal last logged in with. A code for this step or
    /// an earlier one is refused, so each code is accepted once. [VLD-ANB-MIDGARD-0026]
    pub totp_last_step: Option<u64>,
}

impl Credentials {
    /// Whether the principal has enrolled any second factor.
    pub fn has_mfa(&self) -> bool {
        self.totp.is_some() || !self.passkeys.is_empty()
    }
}

/// A full directory entry: authorization attributes + credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enrollment {
    /// Roles / clearance / authorised purposes (what a token carries).
    pub user: UserRecord,
    /// Password + MFA credentials (what authentication checks).
    pub credentials: Credentials,
}

/// A durable, encrypted, restart-surviving directory of enrollments.
pub struct DurableDirectory {
    journal: SealedJournal,
    entries: BTreeMap<String, Enrollment>,
}

impl DurableDirectory {
    /// Open (creating if absent) the directory at `path`, sealed by `sealer`,
    /// replaying every persisted enrollment into memory. [RC-29]
    pub fn open(path: impl AsRef<Path>, sealer: Box<dyn Sealer>) -> Result<Self, AuditError> {
        let (journal, records) = SealedJournal::open(path, sealer, DOMAIN)?;
        let mut entries = BTreeMap::new();
        for rec in records {
            match decode_record(&rec)
                .map_err(|e| AuditError::Store(format!("directory decode: {e}")))?
            {
                Record::Enrollment(subject, enrollment) => {
                    entries.insert(subject, enrollment);
                }
                Record::TotpStep(subject, step) => {
                    // A step for a principal no longer enrolled protects nothing.
                    if let Some(entry) = entries.get_mut(&subject) {
                        advance(&mut entry.credentials, step);
                    }
                }
            }
        }
        Ok(Self { journal, entries })
    }

    /// Enroll (or replace) a principal: append the sealed record and update memory.
    pub fn enroll(&mut self, subject: &str, enrollment: Enrollment) -> Result<(), AuditError> {
        let record = encode_entry(subject, &enrollment);
        self.journal.append(&record)?;
        self.entries.insert(subject.to_string(), enrollment);
        Ok(())
    }

    /// Replace the credentials of an enrolled principal, keeping its attributes:
    /// append a full record and update memory. `Ok(false)` if `subject` is not
    /// enrolled. [VLD-ANB-MIDGARD-0002]
    pub fn update_credentials(
        &mut self,
        subject: &str,
        credentials: Credentials,
    ) -> Result<bool, AuditError> {
        let Some(entry) = self.entries.get(subject) else {
            return Ok(false);
        };
        // Replacing the credentials never winds the last accepted TOTP step back.
        let mut credentials = credentials;
        if let Some(step) = entry.credentials.totp_last_step {
            advance(&mut credentials, step);
        }
        let updated = Enrollment {
            user: entry.user.clone(),
            credentials,
        };
        self.enroll(subject, updated).map(|()| true)
    }

    /// Record that `subject` has just logged in with the TOTP code for `step`: append
    /// a small sealed step record and update memory, so that code, and every earlier
    /// one, is refused from now on, across a restart too. A step at or below the one
    /// already recorded writes nothing. `Ok(false)` if `subject` is not enrolled.
    /// Call it under the same lock as the verification, and do not let the login
    /// complete if it fails. [VLD-ANB-MIDGARD-0026]
    pub fn record_totp_step(&mut self, subject: &str, step: u64) -> Result<bool, AuditError> {
        let Some(entry) = self.entries.get_mut(subject) else {
            return Ok(false);
        };
        if entry
            .credentials
            .totp_last_step
            .is_some_and(|last| step <= last)
        {
            return Ok(true);
        }
        self.journal.append(&encode_totp_step(subject, step))?;
        advance(&mut entry.credentials, step);
        Ok(true)
    }

    /// Look up an enrollment.
    pub fn get(&self, subject: &str) -> Option<&Enrollment> {
        self.entries.get(subject)
    }

    /// The enrolled subjects, in order.
    pub fn subjects(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.keys().map(String::as_str)
    }

    /// The number of enrolled principals.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the directory is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Where the authenticator looks a principal up. A deployment authenticates against
/// the [`DurableDirectory`]; development and tests use the [`InMemoryDirectory`] —
/// and both go through the same password-then-MFA path. [RC-29]
pub trait Directory {
    /// The enrollment for `subject`, if one exists.
    fn enrollment(&self, subject: &str) -> Option<&Enrollment>;

    /// Replace `subject`'s stored password hash with `new_hash` — the rehash-on-sign-in
    /// upgrade — but only while it still equals `current`, so an enrolment replaced in
    /// the meantime is never overwritten. `Ok(true)`: replaced (and, for a durable
    /// directory, persisted); `Ok(false)`: the subject is gone or its hash changed;
    /// `Err`: this directory cannot record it. The default records nothing.
    /// [MG-SEC-11]
    fn replace_password_hash(
        &mut self,
        subject: &str,
        current: &str,
        new_hash: String,
    ) -> Result<bool, String> {
        let _ = (subject, current, new_hash);
        Err("this directory cannot update stored credentials".into())
    }
}

/// Keep the greater of `credentials`' last accepted TOTP step and `step`.
fn advance(credentials: &mut Credentials, step: u64) {
    credentials.totp_last_step = Some(credentials.totp_last_step.map_or(step, |s| s.max(step)));
}

/// The updated enrollment, if `subject`'s password hash is still `current`.
fn with_password_hash(
    entry: Option<&Enrollment>,
    current: &str,
    new_hash: String,
) -> Option<Enrollment> {
    let entry = entry.filter(|e| e.credentials.password_hash == current)?;
    let mut updated = entry.clone();
    updated.credentials.password_hash = new_hash;
    Some(updated)
}

impl Directory for DurableDirectory {
    fn enrollment(&self, subject: &str) -> Option<&Enrollment> {
        self.get(subject)
    }

    /// Appends the updated enrollment to the sealed journal, so the upgrade survives
    /// a restart exactly like an enrolment does. [MG-SEC-11] [RC-29]
    fn replace_password_hash(
        &mut self,
        subject: &str,
        current: &str,
        new_hash: String,
    ) -> Result<bool, String> {
        match with_password_hash(self.get(subject), current, new_hash) {
            Some(updated) => self
                .enroll(subject, updated)
                .map(|()| true)
                .map_err(|e| e.to_string()),
            None => Ok(false),
        }
    }
}

/// A non-durable directory: enrollments live in memory and are lost on exit.
#[derive(Default)]
pub struct InMemoryDirectory {
    entries: BTreeMap<String, Enrollment>,
}

impl InMemoryDirectory {
    /// An empty directory.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enroll (or replace) a principal.
    pub fn enroll(&mut self, subject: &str, enrollment: Enrollment) {
        self.entries.insert(subject.to_string(), enrollment);
    }

    /// Remove a principal, returning their enrollment if they were enrolled. A token
    /// already issued to them is then refused by a verifier that checks the directory.
    /// [VLD-ANB-MIDGARD-0021]
    pub fn remove(&mut self, subject: &str) -> Option<Enrollment> {
        self.entries.remove(subject)
    }

    /// Replace the credentials of an enrolled principal, keeping its attributes.
    /// `false` if `subject` is not enrolled. [VLD-ANB-MIDGARD-0002]
    pub fn update_credentials(&mut self, subject: &str, credentials: Credentials) -> bool {
        match self.entries.get_mut(subject) {
            Some(entry) => {
                let last = entry.credentials.totp_last_step;
                entry.credentials = credentials;
                if let Some(step) = last {
                    advance(&mut entry.credentials, step);
                }
                true
            }
            None => false,
        }
    }

    /// Record that `subject` has just logged in with the TOTP code for `step`, so that
    /// code and every earlier one is refused from now on. `false` if `subject` is not
    /// enrolled. [VLD-ANB-MIDGARD-0026]
    pub fn record_totp_step(&mut self, subject: &str, step: u64) -> bool {
        match self.entries.get_mut(subject) {
            Some(entry) => {
                advance(&mut entry.credentials, step);
                true
            }
            None => false,
        }
    }

    /// The number of enrolled principals.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the directory is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Directory for InMemoryDirectory {
    fn enrollment(&self, subject: &str) -> Option<&Enrollment> {
        self.entries.get(subject)
    }

    fn replace_password_hash(
        &mut self,
        subject: &str,
        current: &str,
        new_hash: String,
    ) -> Result<bool, String> {
        match with_password_hash(self.entries.get(subject), current, new_hash) {
            Some(updated) => {
                self.enroll(subject, updated);
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

// ---- record codec ---------------------------------------------------------

fn put_u32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
    put_u32(o, b.len() as u32);
    o.extend_from_slice(b);
}
fn put_str(o: &mut Vec<u8>, s: &str) {
    put_bytes(o, s.as_bytes());
}

/// A tagged version-3 record: the enrollment, its pending activation, then its last
/// accepted TOTP step.
fn encode_entry(subject: &str, e: &Enrollment) -> Vec<u8> {
    let mut o = vec![TAGGED, RECORD_VERSION, KIND_ENROLLMENT];
    o.extend_from_slice(&encode_body(subject, e));
    match &e.credentials.activation {
        None => o.push(0),
        Some(a) => {
            o.push(1);
            put_str(&mut o, &a.code_hash);
            o.extend_from_slice(&a.expires_at.to_le_bytes());
            o.push(u8::from(a.totp_confirmed));
        }
    }
    match e.credentials.totp_last_step {
        None => o.push(0),
        Some(step) => {
            o.push(1);
            o.extend_from_slice(&step.to_le_bytes());
        }
    }
    o
}

/// A tagged version-3 TOTP step record: the subject and its last accepted step.
fn encode_totp_step(subject: &str, step: u64) -> Vec<u8> {
    let mut o = vec![TAGGED, RECORD_VERSION, KIND_TOTP_STEP];
    put_str(&mut o, subject);
    o.extend_from_slice(&step.to_le_bytes());
    o
}

/// The version-1 body, which a tagged record carries unchanged: subject, attributes
/// and credentials, with no activation.
fn encode_body(subject: &str, e: &Enrollment) -> Vec<u8> {
    let mut o = Vec::new();
    put_str(&mut o, subject);
    // UserRecord
    put_u32(&mut o, e.user.roles.len() as u32);
    for r in &e.user.roles {
        put_str(&mut o, &r.0);
    }
    o.push(e.user.clearance.level.0);
    put_u32(&mut o, e.user.clearance.compartments.len() as u32);
    for c in &e.user.clearance.compartments {
        put_str(&mut o, &c.0);
    }
    put_u32(&mut o, e.user.purposes.len() as u32);
    for p in &e.user.purposes {
        put_str(&mut o, &p.0);
    }
    // Credentials
    put_str(&mut o, &e.credentials.password_hash);
    match &e.credentials.totp {
        Some(t) => {
            o.push(1);
            put_bytes(&mut o, t.secret());
        }
        None => o.push(0),
    }
    put_u32(&mut o, e.credentials.passkeys.len() as u32);
    for k in &e.credentials.passkeys {
        put_bytes(&mut o, &k.credential_id);
        o.push(k.public_key.alg().as_u8());
        put_bytes(&mut o, k.public_key.as_bytes());
        put_str(&mut o, &k.rp_id);
    }
    o
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|e| *e <= self.b.len())
            .ok_or("truncated")?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<usize, String> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
    }
    fn bytes(&mut self) -> Result<Vec<u8>, String> {
        let n = self.u32()?;
        Ok(self.take(n)?.to_vec())
    }
    fn string(&mut self) -> Result<String, String> {
        String::from_utf8(self.bytes()?).map_err(|_| "utf8".into())
    }
    fn u64(&mut self) -> Result<u64, String> {
        let s = self.take(8)?;
        Ok(u64::from_le_bytes(s.try_into().expect("eight bytes")))
    }
}

/// A decoded directory record.
enum Record {
    /// A full enrollment, replacing any earlier one for the subject.
    Enrollment(String, Enrollment),
    /// The subject's last accepted TOTP step.
    TotpStep(String, u64),
}

/// Decode a record of any version. A tagged record that does not decode is tried as
/// version 1, since an untagged record whose subject length has `0xFF` as its low
/// byte also starts with the tag.
fn decode_record(b: &[u8]) -> Result<Record, String> {
    let v1 = |b| decode_v1(b).map(|(subject, e)| Record::Enrollment(subject, e));
    if b.first() == Some(&TAGGED) {
        return decode_tagged(b).or_else(|tagged| v1(b).map_err(|_| tagged));
    }
    v1(b)
}

fn decode_tagged(b: &[u8]) -> Result<Record, String> {
    let mut r = Reader { b, pos: 1 };
    let version = match (r.u8()?, r.u8()?) {
        (RECORD_VERSION, KIND_TOTP_STEP) => {
            let subject = r.string()?;
            let step = r.u64()?;
            if r.pos != b.len() {
                return Err("trailing bytes".into());
            }
            return Ok(Record::TotpStep(subject, step));
        }
        (version @ (RECORD_VERSION | RECORD_VERSION_2), KIND_ENROLLMENT) => version,
        (version, kind) => return Err(format!("unknown record version {version} kind {kind}")),
    };
    let (subject, mut enrollment) = decode_body(&mut r)?;
    enrollment.credentials.activation = match r.u8()? {
        0 => None,
        1 => Some(Activation {
            code_hash: r.string()?,
            expires_at: r.u64()?,
            totp_confirmed: match r.u8()? {
                0 => false,
                1 => true,
                _ => return Err("bad totp-confirmed flag".into()),
            },
        }),
        _ => return Err("bad activation tag".into()),
    };
    if version == RECORD_VERSION {
        enrollment.credentials.totp_last_step = match r.u8()? {
            0 => None,
            1 => Some(r.u64()?),
            _ => return Err("bad totp-step tag".into()),
        };
    }
    if r.pos != b.len() {
        return Err("trailing bytes".into());
    }
    Ok(Record::Enrollment(subject, enrollment))
}

fn decode_v1(b: &[u8]) -> Result<(String, Enrollment), String> {
    let mut r = Reader { b, pos: 0 };
    let entry = decode_body(&mut r)?;
    if r.pos != b.len() {
        return Err("trailing bytes".into());
    }
    Ok(entry)
}

/// The version-1 body both versions share, leaving the reader after the passkeys.
fn decode_body(r: &mut Reader<'_>) -> Result<(String, Enrollment), String> {
    let subject = r.string()?;
    let mut roles = std::collections::BTreeSet::new();
    for _ in 0..r.u32()? {
        roles.insert(Role(r.string()?));
    }
    let level = ClassificationLevel(r.u8()?);
    let mut compartments = std::collections::BTreeSet::new();
    for _ in 0..r.u32()? {
        compartments.insert(Compartment(r.string()?));
    }
    let mut purposes = std::collections::BTreeSet::new();
    for _ in 0..r.u32()? {
        purposes.insert(Purpose(r.string()?));
    }
    let password_hash = r.string()?;
    let totp = match r.u8()? {
        0 => None,
        1 => Some(TotpSecret::new(r.bytes()?)),
        _ => return Err("bad totp tag".into()),
    };
    let mut passkeys = Vec::new();
    for _ in 0..r.u32()? {
        let credential_id = r.bytes()?;
        let alg = SigId::from_u8(r.u8()?).ok_or("bad sig alg")?;
        let public_key = VerifyingKey::new(alg, r.bytes()?);
        let rp_id = r.string()?;
        passkeys.push(WebAuthnCredential {
            credential_id,
            public_key,
            rp_id,
        });
    }
    Ok((
        subject,
        Enrollment {
            user: UserRecord {
                roles,
                clearance: Clearance {
                    level,
                    compartments,
                },
                purposes,
            },
            credentials: Credentials {
                password_hash,
                totp,
                passkeys,
                activation: None,
                totp_last_step: None,
            },
        },
    ))
}
