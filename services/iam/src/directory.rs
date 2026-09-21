//! The **durable identity directory**: principals with their authorization
//! attributes *and* credentials (password hash + enrolled MFA factors), persisted
//! through the KMS-sealed append-only [`SealedJournal`] and **replayed on boot**. It
//! survives a restart with every enrollment intact and encrypted at rest; a tampered
//! record fails closed on load. [RC-29] [AC-M-4]

use std::collections::BTreeMap;
use std::path::Path;

use audit_writer::{AuditError, SealedJournal, Sealer};
use policy::{ClassificationLevel, Clearance, Compartment, Purpose, Role};
use pqc_agility::{SigId, VerifyingKey};

use crate::mfa::{TotpSecret, WebAuthnCredential};
use crate::store::UserRecord;

const DOMAIN: &[u8] = b"iam-directory-v1";

/// A principal's credentials: a password hash and enrolled MFA factors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Credentials {
    /// PHC-encoded password hash (empty = no password set).
    pub password_hash: String,
    /// Enrolled TOTP secret, if any.
    pub totp: Option<TotpSecret>,
    /// Enrolled passkeys.
    pub passkeys: Vec<WebAuthnCredential>,
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
            let (subject, enrollment) = decode_entry(&rec)
                .map_err(|e| AuditError::Store(format!("directory decode: {e}")))?;
            entries.insert(subject, enrollment);
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
}

impl Directory for DurableDirectory {
    fn enrollment(&self, subject: &str) -> Option<&Enrollment> {
        self.get(subject)
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

fn encode_entry(subject: &str, e: &Enrollment) -> Vec<u8> {
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
}

fn decode_entry(b: &[u8]) -> Result<(String, Enrollment), String> {
    let mut r = Reader { b, pos: 0 };
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
    if r.pos != b.len() {
        return Err("trailing bytes".into());
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
            },
        },
    ))
}
