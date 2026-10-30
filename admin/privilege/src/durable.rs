//! Durable, KMS-sealed privilege & dual-control service. Two things persist: the
//! PQ-signed decision **audit ledger** (a durable [`FileLedger`], re-verifiable on
//! boot) and the in-flight **action state** (each [`ActionEntry`] appended to a
//! [`SealedJournal`]). On [`DurablePrivilege::open`] the ledger is reopened and the
//! action state replayed, so both the forensic lineage and pending dual-control
//! approvals survive a restart. [FR-AD-5] [AC-M-4] [RC-14]

use std::collections::BTreeMap;

use audit_writer::{AuditWriter, FileLedger, LedgerId, SealedEntry, Sealer};
use pqc_agility::{SignatureScheme, SigningKey, VerifyingKey};

use crate::control::controls_for;
use crate::error::PrivError;
use crate::risk::ActionClass;
use crate::service::{ActionEntry, ActionState, AdminActionRequest, Disposition, PrivilegeService};
use admin_identity::{AdminId, AdminSession, IdentityDirectory, ResourceScope, Subsystem};

const DOMAIN: &[u8] = b"midgard-admin-privilege-state-v1";
/// The privilege service's audit ledger, in the directory it is opened at.
pub const AUDIT_FILE: &str = "audit.ledger";
const STATE_FILE: &str = "actions.journal";

/// A privilege service whose audit lineage and in-flight action state are durable.
pub struct DurablePrivilege {
    inner: PrivilegeService<FileLedger>,
    journal: audit_writer::SealedJournal,
}

impl DurablePrivilege {
    /// Open (creating if absent) the durable privilege service rooted at `dir`.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        dir: impl AsRef<std::path::Path>,
        audit_sealer: Box<dyn Sealer>,
        state_sealer: Box<dyn Sealer>,
        scheme: Box<dyn SignatureScheme>,
        signing_key: SigningKey,
        verifying_key: VerifyingKey,
        ledger_id: LedgerId,
    ) -> Result<Self, PrivError> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir).map_err(|e| PrivError::Io(e.to_string()))?;

        // Durable, PQ-signed audit ledger (fails closed on tamper).
        let ledger =
            FileLedger::open_or_create(dir.join(AUDIT_FILE), audit_sealer, ledger_id.clone())
                .map_err(|e| PrivError::Io(e.to_string()))?;

        // Recover in-flight action state.
        let (journal, records) =
            audit_writer::SealedJournal::open(dir.join(STATE_FILE), state_sealer, DOMAIN)
                .map_err(|e| PrivError::Io(e.to_string()))?;
        let mut actions: BTreeMap<String, ActionEntry> = BTreeMap::new();
        for r in &records {
            let entry = decode_entry(r)?;
            actions.insert(entry.request.id.clone(), entry);
        }

        let writer = AuditWriter::new(ledger, scheme, signing_key, verifying_key, ledger_id);
        let inner = PrivilegeService::restore(writer, actions);
        Ok(Self { inner, journal })
    }

    /// Submit an admin action (audited), then durably persist its state.
    #[allow(clippy::too_many_arguments)]
    pub fn submit(
        &mut self,
        dir: &IdentityDirectory,
        session: &AdminSession,
        req: AdminActionRequest,
        now: u64,
        step_up_max_age: u64,
        ts: &str,
    ) -> Result<Disposition, PrivError> {
        let id = req.id.clone();
        let disp = self
            .inner
            .submit(dir, session, req, now, step_up_max_age, ts)?;
        self.persist(&id)?;
        Ok(disp)
    }

    /// Approve a parked action (audited), then durably persist its state.
    pub fn approve(
        &mut self,
        dir: &IdentityDirectory,
        approver_session: &AdminSession,
        id: &str,
        now: u64,
        reauth_max_age: u64,
        ts: &str,
    ) -> Result<(), PrivError> {
        self.inner
            .approve(dir, approver_session, id, now, reauth_max_age, ts)?;
        self.persist(id)
    }

    /// Reject a parked action (audited), then durably persist its state.
    pub fn reject(
        &mut self,
        approver_session: &AdminSession,
        id: &str,
        reason: &str,
        ts: &str,
    ) -> Result<(), PrivError> {
        self.inner.reject(approver_session, id, reason, ts)?;
        self.persist(id)
    }

    /// The underlying service (reads: `record`, `state_of`).
    pub fn service(&self) -> &PrivilegeService<FileLedger> {
        &self.inner
    }
    /// The sealed audit entries (for `audit_writer::verify_chain`).
    pub fn audit_entries(&self) -> &[SealedEntry] {
        self.inner.audit_entries()
    }
    /// The audit verifying key.
    pub fn verifying_key(&self) -> &VerifyingKey {
        self.inner.verifying_key()
    }
    /// The audit ledger id.
    pub fn ledger_id(&self) -> &LedgerId {
        self.inner.ledger_id()
    }

    fn persist(&mut self, id: &str) -> Result<(), PrivError> {
        let entry = self.inner.record(id).expect("record just mutated").clone();
        self.journal
            .append(&encode_entry(&entry))
            .map_err(|e| PrivError::Io(e.to_string()))
    }
}

// --- ActionEntry codec (risk + controls recomputed from action_class) -----

fn action_class_u8(a: ActionClass) -> u8 {
    match a {
        ActionClass::ReadDashboard => 0,
        ActionClass::ConfigLabelEdit => 1,
        ActionClass::ScopedGrant => 2,
        ActionClass::ModelDeprecate => 3,
        ActionClass::KeyRotation => 4,
        ActionClass::PrivilegeEscalation => 5,
        ActionClass::ModelRevocation => 6,
        ActionClass::FleetWideRelease => 7,
        ActionClass::BreakGlass => 8,
        ActionClass::PanelPrivilegeChange => 9,
        ActionClass::PrincipalEnroll => 10,
    }
}

fn action_class_from_u8(v: u8) -> Result<ActionClass, PrivError> {
    Ok(match v {
        0 => ActionClass::ReadDashboard,
        1 => ActionClass::ConfigLabelEdit,
        2 => ActionClass::ScopedGrant,
        3 => ActionClass::ModelDeprecate,
        4 => ActionClass::KeyRotation,
        5 => ActionClass::PrivilegeEscalation,
        6 => ActionClass::ModelRevocation,
        7 => ActionClass::FleetWideRelease,
        8 => ActionClass::BreakGlass,
        9 => ActionClass::PanelPrivilegeChange,
        10 => ActionClass::PrincipalEnroll,
        _ => return Err(PrivError::Io("unknown action class".into())),
    })
}

fn put_str(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u32).to_le_bytes());
    o.extend_from_slice(s.as_bytes());
}

fn encode_entry(e: &ActionEntry) -> Vec<u8> {
    let mut o = Vec::new();
    let req = &e.request;
    put_str(&mut o, &req.id);
    put_str(&mut o, &req.requester.0);
    o.push(action_class_u8(req.action_class));
    put_str(&mut o, &req.scope.subsystem.0);
    put_str(&mut o, &req.scope.selector);
    put_str(&mut o, &req.justification);
    match &e.state {
        ActionState::PendingApproval => o.push(0),
        ActionState::Authorised { approver } => {
            o.push(1);
            match approver {
                Some(a) => {
                    o.push(1);
                    put_str(&mut o, &a.0);
                }
                None => o.push(0),
            }
        }
        ActionState::Rejected { approver, reason } => {
            o.push(2);
            put_str(&mut o, &approver.0);
            put_str(&mut o, reason);
        }
    }
    o.extend_from_slice(&(e.audit_seqs.len() as u32).to_le_bytes());
    for s in &e.audit_seqs {
        o.extend_from_slice(&s.to_le_bytes());
    }
    o
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], PrivError> {
        let e = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| PrivError::Io("truncated action record".into()))?;
        let s = &self.b[self.p..e];
        self.p = e;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, PrivError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<usize, PrivError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    fn u64(&mut self) -> Result<u64, PrivError> {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(a))
    }
    fn s(&mut self) -> Result<String, PrivError> {
        let n = self.u32()?;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| PrivError::Io("action record invalid utf8".into()))
    }
}

fn decode_entry(b: &[u8]) -> Result<ActionEntry, PrivError> {
    let mut r = R { b, p: 0 };
    let id = r.s()?;
    let requester = AdminId(r.s()?);
    let action_class = action_class_from_u8(r.u8()?)?;
    let subsystem = Subsystem(r.s()?);
    let selector = r.s()?;
    let justification = r.s()?;
    let state = match r.u8()? {
        0 => ActionState::PendingApproval,
        1 => ActionState::Authorised {
            approver: match r.u8()? {
                0 => None,
                1 => Some(AdminId(r.s()?)),
                _ => return Err(PrivError::Io("bad approver tag".into())),
            },
        },
        2 => ActionState::Rejected {
            approver: AdminId(r.s()?),
            reason: r.s()?,
        },
        _ => return Err(PrivError::Io("unknown action state".into())),
    };
    let nseq = r.u32()?;
    let mut audit_seqs = Vec::with_capacity(nseq);
    for _ in 0..nseq {
        audit_seqs.push(r.u64()?);
    }
    if r.p != b.len() {
        return Err(PrivError::Io("action record trailing bytes".into()));
    }
    // Risk + controls are a pure function of the action class — recompute them.
    let risk = action_class.risk();
    let controls = controls_for(risk);
    Ok(ActionEntry {
        request: AdminActionRequest {
            id,
            requester,
            action_class,
            scope: ResourceScope {
                subsystem,
                selector,
            },
            justification,
        },
        risk,
        controls,
        state,
        audit_seqs,
    })
}
