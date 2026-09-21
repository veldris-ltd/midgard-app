//! The workflow engine. Drives a proposed action through propose → authorise/deny →
//! execute, sealing every transition into a PQ-signed audit chain and refusing to
//! execute a consequential action that a human has not authorised. [FR-H-5]
//! [NFR-AI-1]

use std::collections::BTreeMap;

use audit_writer::{
    Action, Actor, AuditRecord, AuditWriter, LedgerId, LedgerStore, RecordId, Resource,
    SealedEntry, Timestamp,
};
use pqc_agility::VerifyingKey;

use crate::action::ProposedAction;
use crate::actuator::{ActuationOutcome, Actuator};
use crate::error::WorkflowError;

/// Where an action sits in its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowState {
    /// Proposed, awaiting a human decision.
    Proposed,
    /// A human authorised it (records who).
    Authorised {
        /// The authorising human.
        actor: String,
    },
    /// A human denied it (records who).
    Denied {
        /// The denying human.
        actor: String,
    },
    /// Executed after authorisation (records who executed and the outcome).
    Executed {
        /// The executing actor.
        actor: String,
        /// The recorded outcome.
        outcome: String,
    },
}

/// A workflow's current action, state, and the audit sequence numbers of its
/// lineage (in order: proposed, [authorised|denied], [executed]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRecord {
    /// The proposed action.
    pub action: ProposedAction,
    /// Its current state.
    pub state: WorkflowState,
    /// The audit chain sequence numbers of every transition, in order.
    pub audit_seqs: Vec<u64>,
}

/// The human-gated, audited action-workflow engine.
pub struct WorkflowEngine<S: LedgerStore> {
    writer: AuditWriter<S>,
    actions: BTreeMap<String, ActionRecord>,
}

impl<S: LedgerStore> WorkflowEngine<S> {
    /// Build an engine over an audit writer (the workflow's ledger).
    pub fn new(writer: AuditWriter<S>) -> Self {
        Self {
            writer,
            actions: BTreeMap::new(),
        }
    }

    /// Propose an action. Records a `propose-action` audit entry and parks the
    /// action in `Proposed` — nothing executes here. Fails if the id is reused.
    pub fn propose(
        &mut self,
        proposer: &str,
        action: ProposedAction,
        ts: &str,
    ) -> Result<(), WorkflowError> {
        let id = action.id.clone();
        if self.actions.contains_key(&id) {
            return Err(WorkflowError::DuplicateAction(id));
        }
        let seq = self.audit(proposer, "propose-action", &id, ts)?;
        self.actions.insert(
            id,
            ActionRecord {
                action,
                state: WorkflowState::Proposed,
                audit_seqs: vec![seq],
            },
        );
        Ok(())
    }

    /// **The human gate.** A human authorises or denies a proposed action; the
    /// decision and its actor are audited. Only valid while the action is `Proposed`.
    /// [FR-H-5] [NFR-AI-1]
    pub fn authorise(
        &mut self,
        ticket: &str,
        human: &str,
        approve: bool,
        ts: &str,
    ) -> Result<(), WorkflowError> {
        match self.actions.get(ticket) {
            None => return Err(WorkflowError::UnknownAction(ticket.to_string())),
            Some(r) if !matches!(r.state, WorkflowState::Proposed) => {
                return Err(WorkflowError::NotPending(ticket.to_string()))
            }
            Some(_) => {}
        }
        let phase = if approve {
            "authorise-action"
        } else {
            "deny-action"
        };
        let seq = self.audit(human, phase, ticket, ts)?;
        let r = self.actions.get_mut(ticket).expect("checked above");
        r.state = if approve {
            WorkflowState::Authorised {
                actor: human.to_string(),
            }
        } else {
            WorkflowState::Denied {
                actor: human.to_string(),
            }
        };
        r.audit_seqs.push(seq);
        Ok(())
    }

    /// Execute an authorised action by **invoking a real [`Actuator`]** and sealing
    /// the outcome into the audit chain. [RT-44] [RC-44]
    ///
    /// A **consequential action can only execute from the `Authorised` state** —
    /// from `Proposed` it fails closed with [`WorkflowError::NotAuthorised`]; a
    /// denied or already-executed action cannot run. Only after those gates pass is
    /// the actuator called. The audit lineage is sealed in two phases so even a
    /// crash mid-actuation leaves evidence of the attempt:
    /// 1. an `execute-action` **intent** entry before the effector is touched, then
    /// 2. an `actuation-succeeded` / `actuation-failed` entry with the effector's
    ///    real outcome — or, if the actuator could not be invoked at all, an
    ///    `actuation-error` entry, after which the action stays `Authorised`
    ///    (retryable) and the call returns [`WorkflowError::Actuation`].
    ///
    /// [FR-H-5] [NFR-AI-1] [AC-H-4]
    pub fn execute(
        &mut self,
        executor: &str,
        ticket: &str,
        actuator: &mut dyn Actuator,
        ts: &str,
    ) -> Result<ActuationOutcome, WorkflowError> {
        match self.actions.get(ticket) {
            None => return Err(WorkflowError::UnknownAction(ticket.to_string())),
            Some(r) => match &r.state {
                WorkflowState::Executed { .. } => {
                    return Err(WorkflowError::AlreadyExecuted(ticket.to_string()))
                }
                WorkflowState::Denied { .. } => {
                    return Err(WorkflowError::Denied(ticket.to_string()))
                }
                // The core invariant: a consequential action requires authorisation.
                WorkflowState::Proposed if r.action.consequential => {
                    return Err(WorkflowError::NotAuthorised(ticket.to_string()))
                }
                WorkflowState::Proposed | WorkflowState::Authorised { .. } => {}
            },
        }

        // Phase 1: seal the execution intent BEFORE touching the effector.
        let seq_intent = self.audit(executor, "execute-action", ticket, ts)?;

        // Phase 2: invoke the real actuator on a clone of the action (no borrow of
        // self is held across the call).
        let action = self
            .actions
            .get(ticket)
            .expect("checked above")
            .action
            .clone();
        match actuator.actuate(&action) {
            Ok(outcome) => {
                let phase = match outcome.status {
                    crate::actuator::ActuationStatus::Succeeded => "actuation-succeeded",
                    crate::actuator::ActuationStatus::Failed => "actuation-failed",
                };
                let seq_out = self.audit(executor, phase, ticket, ts)?;
                let r = self.actions.get_mut(ticket).expect("checked above");
                r.state = WorkflowState::Executed {
                    actor: executor.to_string(),
                    outcome: format!("{}: {}", outcome.status.label(), outcome.detail),
                };
                r.audit_seqs.push(seq_intent);
                r.audit_seqs.push(seq_out);
                Ok(outcome)
            }
            Err(e) => {
                // Could not invoke the effector at all: audit the failed attempt and
                // leave the action Authorised so it can be retried. Fail closed.
                let seq_err = self.audit(executor, "actuation-error", ticket, ts)?;
                let r = self.actions.get_mut(ticket).expect("checked above");
                r.audit_seqs.push(seq_intent);
                r.audit_seqs.push(seq_err);
                Err(WorkflowError::Actuation(e.to_string()))
            }
        }
    }

    /// The current state of an action.
    pub fn state_of(&self, ticket: &str) -> Option<&WorkflowState> {
        self.actions.get(ticket).map(|r| &r.state)
    }

    /// The full record (action + state + audit lineage) for a ticket.
    pub fn record(&self, ticket: &str) -> Option<&ActionRecord> {
        self.actions.get(ticket)
    }

    /// Every in-flight workflow record, keyed by ticket. The durable store snapshots
    /// this to persist in-flight workflow state across a restart. [RC-13]
    pub fn records(&self) -> &BTreeMap<String, ActionRecord> {
        &self.actions
    }

    /// Rebuild an engine from a durable audit writer and a previously-snapshotted
    /// set of in-flight records — the recovery path for [RC-13]. The audit chain is
    /// the writer's (a durable `FileLedger`); `actions` is the recovered state.
    pub fn restore(writer: AuditWriter<S>, actions: BTreeMap<String, ActionRecord>) -> Self {
        Self { writer, actions }
    }

    /// The sealed audit entries (the tamper-evident decision lineage). [FR-H-5]
    pub fn audit_entries(&self) -> &[SealedEntry] {
        self.writer.entries()
    }

    /// The verifying key for the audit chain (for `audit_writer::verify_chain`).
    pub fn verifying_key(&self) -> &VerifyingKey {
        self.writer.verifying_key()
    }

    /// The audit ledger id.
    pub fn ledger_id(&self) -> &LedgerId {
        self.writer.ledger_id()
    }

    /// Seal one transition into the audit chain, returning its sequence number.
    fn audit(
        &mut self,
        actor: &str,
        action_name: &str,
        resource_id: &str,
        ts: &str,
    ) -> Result<u64, WorkflowError> {
        let record = AuditRecord {
            id: RecordId(format!("{resource_id}:{action_name}")),
            actor: Actor(actor.to_string()),
            action: Action(action_name.to_string()),
            resource: Resource(resource_id.to_string()),
            ts: Timestamp(ts.to_string()),
        };
        self.writer
            .append(record)
            .map_err(|e| WorkflowError::Audit(format!("{e:?}")))
    }
}
