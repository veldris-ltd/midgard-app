//! [`DurableWorkflow`] — the human-gated action-workflow engine made durable.
//!
//! Two things must survive a restart: the **PQ-signed decision lineage** and the
//! **in-flight workflow state** (which tickets are proposed / authorised / etc.).
//! The lineage persists in a durable [`FileLedger`] (per-entry `fsync`, KMS-sealed
//! at rest); the in-flight [`ActionRecord`] map is snapshotted KMS-sealed alongside
//! it on every transition. On [`DurableWorkflow::open`] the ledger is reopened and
//! the state snapshot unsealed, and the engine is rebuilt from both — so both the
//! audit chain and the pending workflows come back intact. [FR-H-5] [AC-M-4] [RC-13]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use audit_writer::{AuditWriter, FileLedger, LedgerId, SealedEntry, Sealer};
use heimdall_workflow::{
    ActionRecord, ActuationOutcome, Actuator, ProposedAction, WorkflowEngine, WorkflowState,
};
use pqc_agility::{SignatureScheme, SigningKey, VerifyingKey};

use crate::error::StoreError;
use crate::util::{atomic_write, io, put_str, Reader};

const STATE_AAD: &[u8] = b"midgard-heimdall-workflow-state-v1";
const AUDIT_FILE: &str = "audit.ledger";
const STATE_FILE: &str = "workflow.state";

/// A workflow engine whose audit lineage and in-flight state both survive a restart.
pub struct DurableWorkflow {
    engine: WorkflowEngine<FileLedger>,
    state_path: PathBuf,
    state_sealer: Box<dyn Sealer>,
}

impl DurableWorkflow {
    /// Open (creating if absent) the durable workflow rooted at `dir`.
    ///
    /// `audit_sealer` encrypts the audit `FileLedger` at rest; `state_sealer`
    /// encrypts the in-flight-state snapshot (both KMS envelopes over the same trust
    /// root). The signing key/scheme sign the audit chain and MUST be stable across
    /// restarts so the chain continues and verifies.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        dir: impl AsRef<Path>,
        audit_sealer: Box<dyn Sealer>,
        state_sealer: Box<dyn Sealer>,
        scheme: Box<dyn SignatureScheme>,
        signing_key: SigningKey,
        verifying_key: VerifyingKey,
        ledger_id: LedgerId,
    ) -> Result<Self, StoreError> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir).map_err(io)?;
        let state_path = dir.join(STATE_FILE);

        // 1. Reopen the durable, PQ-signed audit ledger (fails closed on tamper).
        let ledger =
            FileLedger::open_or_create(dir.join(AUDIT_FILE), audit_sealer, ledger_id.clone())
                .map_err(|e| StoreError::Seal(e.to_string()))?;

        // 2. Recover the in-flight workflow state snapshot.
        let actions = if state_path.exists() {
            let sealed = std::fs::read(&state_path).map_err(io)?;
            let plaintext = state_sealer
                .unseal(STATE_AAD, &sealed)
                .map_err(|e| StoreError::Seal(e.to_string()))?;
            decode_actions(&plaintext)?
        } else {
            BTreeMap::new()
        };

        // 3. Rebuild the engine over both.
        let writer = AuditWriter::new(ledger, scheme, signing_key, verifying_key, ledger_id);
        let engine = WorkflowEngine::restore(writer, actions);

        Ok(Self {
            engine,
            state_path,
            state_sealer,
        })
    }

    /// Propose an action (audited), then durably snapshot the new state.
    pub fn propose(
        &mut self,
        proposer: &str,
        action: ProposedAction,
        ts: &str,
    ) -> Result<(), StoreError> {
        self.engine
            .propose(proposer, action, ts)
            .map_err(|e| StoreError::Workflow(e.to_string()))?;
        self.snapshot()
    }

    /// Authorise or deny a ticket (audited), then durably snapshot the new state.
    pub fn authorise(
        &mut self,
        ticket: &str,
        human: &str,
        approve: bool,
        ts: &str,
    ) -> Result<(), StoreError> {
        self.engine
            .authorise(ticket, human, approve, ts)
            .map_err(|e| StoreError::Workflow(e.to_string()))?;
        self.snapshot()
    }

    /// Execute an authorised ticket by invoking `actuator` (audited), then durably
    /// snapshot the new state. Returns the actuator's outcome. [RT-44]
    pub fn execute(
        &mut self,
        executor: &str,
        ticket: &str,
        actuator: &mut dyn Actuator,
        ts: &str,
    ) -> Result<ActuationOutcome, StoreError> {
        let outcome = self
            .engine
            .execute(executor, ticket, actuator, ts)
            .map_err(|e| StoreError::Workflow(e.to_string()))?;
        self.snapshot()?;
        Ok(outcome)
    }

    /// The underlying engine (read surface: `record`, `state_of`, `audit_entries`,
    /// `verifying_key`, `ledger_id`).
    pub fn engine(&self) -> &WorkflowEngine<FileLedger> {
        &self.engine
    }

    /// The sealed audit entries (for `audit_writer::verify_chain`).
    pub fn audit_entries(&self) -> &[SealedEntry] {
        self.engine.audit_entries()
    }

    fn snapshot(&mut self) -> Result<(), StoreError> {
        let plaintext = encode_actions(self.engine.records());
        let sealed = self
            .state_sealer
            .seal(STATE_AAD, &plaintext)
            .map_err(|e| StoreError::Seal(e.to_string()))?;
        atomic_write(&self.state_path, &sealed)
    }
}

// --- in-flight state codec (no serde; length-prefixed) --------------------

fn encode_actions(actions: &BTreeMap<String, ActionRecord>) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend_from_slice(&(actions.len() as u32).to_le_bytes());
    for (ticket, rec) in actions {
        put_str(&mut o, ticket);
        encode_record(&mut o, rec);
    }
    o
}

fn encode_record(o: &mut Vec<u8>, r: &ActionRecord) {
    // ProposedAction
    put_str(o, &r.action.id);
    put_str(o, &r.action.action_type);
    o.extend_from_slice(&(r.action.params.len() as u32).to_le_bytes());
    for (k, v) in &r.action.params {
        put_str(o, k);
        put_str(o, v);
    }
    put_str(o, &r.action.rationale);
    o.extend_from_slice(&(r.action.citations.len() as u32).to_le_bytes());
    for c in &r.action.citations {
        put_str(o, c);
    }
    o.push(r.action.consequential as u8);
    // WorkflowState
    match &r.state {
        WorkflowState::Proposed => o.push(0),
        WorkflowState::Authorised { actor } => {
            o.push(1);
            put_str(o, actor);
        }
        WorkflowState::Denied { actor } => {
            o.push(2);
            put_str(o, actor);
        }
        WorkflowState::Executed { actor, outcome } => {
            o.push(3);
            put_str(o, actor);
            put_str(o, outcome);
        }
    }
    // audit_seqs
    o.extend_from_slice(&(r.audit_seqs.len() as u32).to_le_bytes());
    for s in &r.audit_seqs {
        o.extend_from_slice(&s.to_le_bytes());
    }
}

fn decode_actions(b: &[u8]) -> Result<BTreeMap<String, ActionRecord>, StoreError> {
    let mut r = Reader::new(b);
    let n = r.u32()?;
    let mut out = BTreeMap::new();
    for _ in 0..n {
        let ticket = r.string()?;
        out.insert(ticket, decode_record(&mut r)?);
    }
    if !r.done() {
        return Err(StoreError::Decode("workflow state trailing bytes".into()));
    }
    Ok(out)
}

fn decode_record(r: &mut Reader) -> Result<ActionRecord, StoreError> {
    let id = r.string()?;
    let action_type = r.string()?;
    let nparams = r.u32()?;
    let mut params = BTreeMap::new();
    for _ in 0..nparams {
        let k = r.string()?;
        let v = r.string()?;
        params.insert(k, v);
    }
    let rationale = r.string()?;
    let ncit = r.u32()?;
    let mut citations = Vec::with_capacity(ncit);
    for _ in 0..ncit {
        citations.push(r.string()?);
    }
    let consequential = r.u8()? != 0;
    let action = ProposedAction {
        id,
        action_type,
        params,
        rationale,
        citations,
        consequential,
    };
    let state = match r.u8()? {
        0 => WorkflowState::Proposed,
        1 => WorkflowState::Authorised { actor: r.string()? },
        2 => WorkflowState::Denied { actor: r.string()? },
        3 => WorkflowState::Executed {
            actor: r.string()?,
            outcome: r.string()?,
        },
        t => {
            return Err(StoreError::Decode(format!(
                "unknown workflow state tag {t}"
            )))
        }
    };
    let nseq = r.u32()?;
    let mut audit_seqs = Vec::with_capacity(nseq);
    for _ in 0..nseq {
        audit_seqs.push(r.u64()?);
    }
    Ok(ActionRecord {
        action,
        state,
        audit_seqs,
    })
}
