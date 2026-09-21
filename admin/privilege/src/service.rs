//! The privilege & dual-control service.
//!
//! Drives an admin action through submit → (pending approval) → authorise / reject,
//! enforcing the risk → controls matrix and sealing every transition into a
//! PQ-signed audit chain. A high-risk action cannot reach the authorised state
//! without a second, distinct approver, and a requester can never approve their
//! own action. `[FR-AD-5]` `[AC-AD-3]` `[AC-AD-4]` `[AC-AD-13]`

use std::collections::BTreeMap;

use admin_identity::{
    AdminId, AdminSession, IdentityDirectory, ResourceScope, ROLE_SECURITY_OFFICER,
};
use audit_writer::{
    Action, Actor, AuditRecord, AuditWriter, LedgerId, LedgerStore, RecordId, Resource,
    SealedEntry, Timestamp,
};
use pqc_agility::VerifyingKey;

use crate::control::{controls_for, RequiredControls};
use crate::error::PrivError;
use crate::risk::{ActionClass, RiskLevel};

/// A requested administrative action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminActionRequest {
    /// Stable action id (also the audit resource).
    pub id: String,
    /// The admin requesting the action.
    pub requester: AdminId,
    /// The action's class (determines risk and required capability).
    pub action_class: ActionClass,
    /// The scope the action targets.
    pub scope: ResourceScope,
    /// Why the action is requested (recorded for audit / review).
    pub justification: String,
}

/// Where an action sits in its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionState {
    /// Awaiting a second, distinct approver (high-risk actions).
    PendingApproval,
    /// Authorised to proceed. For dual-control actions this records the approver;
    /// for single-actor (low/medium-risk) actions the approver is `None`.
    Authorised {
        /// The distinct second approver, if dual-control applied.
        approver: Option<AdminId>,
    },
    /// Rejected by an approver.
    Rejected {
        /// The approver who rejected it.
        approver: AdminId,
        /// The reason given.
        reason: String,
    },
}

/// The record of an action: the request, its computed risk/controls, its current
/// state, and the audit sequence numbers of its lineage (in order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionEntry {
    /// The originating request.
    pub request: AdminActionRequest,
    /// The classified risk level.
    pub risk: RiskLevel,
    /// The controls required at this risk.
    pub controls: RequiredControls,
    /// The current state.
    pub state: ActionState,
    /// Audit chain sequence numbers of every transition, in order.
    pub audit_seqs: Vec<u64>,
}

/// The disposition of a freshly-submitted action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Low/medium-risk: authorised immediately (single actor), still audited.
    Authorised,
    /// High-risk: parked awaiting a second, distinct approver. `[FR-AD-5]`
    NeedsApproval,
}

/// The privilege & dual-control service over a PQ-signed audit ledger.
pub struct PrivilegeService<S: LedgerStore> {
    writer: AuditWriter<S>,
    actions: BTreeMap<String, ActionEntry>,
}

impl<S: LedgerStore> PrivilegeService<S> {
    /// Build a service over an audit writer (its decision ledger).
    pub fn new(writer: AuditWriter<S>) -> Self {
        Self {
            writer,
            actions: BTreeMap::new(),
        }
    }

    /// Submit an admin action. Verifies the requester's session (MFA + step-up),
    /// that they hold the capability the class requires within scope, and that the
    /// action's risk is within their role ceiling; then classifies risk, seals a
    /// `request-action` audit entry, and either parks the action for approval
    /// (dual-control) or authorises it (single actor). `[FR-AD-5]`
    pub fn submit(
        &mut self,
        dir: &IdentityDirectory,
        session: &AdminSession,
        req: AdminActionRequest,
        now: u64,
        step_up_max_age: u64,
        ts: &str,
    ) -> Result<Disposition, PrivError> {
        if session.admin() != &req.requester {
            return Err(PrivError::SessionMismatch);
        }
        if !session.has_mfa() {
            return Err(PrivError::MfaRequired);
        }
        if self.actions.contains_key(&req.id) {
            return Err(PrivError::DuplicateAction(req.id));
        }

        let risk = req.action_class.risk();
        let controls = controls_for(risk);

        // The requester must hold the capability that gates this action class.
        let cap = req.action_class.required_capability();
        if !dir.has_capability(&req.requester, cap, &req.scope, now) {
            return Err(PrivError::NotAuthorisedToRequest {
                capability: cap.to_string(),
            });
        }
        // The action's risk must be within the requester's role ceiling.
        if dir.risk_ceiling(&req.requester, &req.scope, now) < risk.level() {
            return Err(PrivError::RiskCeilingExceeded { risk: risk.level() });
        }
        // Step-up into the action's scope (risk ≥ 2).
        if controls.step_up {
            session.authorize_scope(&req.scope, now, step_up_max_age)?;
        }

        let id = req.id.clone();
        let requester = req.requester.clone();
        let seq0 = self.audit(&requester.0, "request-action", &id, ts)?;

        let (state, disposition, seqs) = if controls.dual_control {
            (
                ActionState::PendingApproval,
                Disposition::NeedsApproval,
                vec![seq0],
            )
        } else {
            // Single-actor path: authorised on submission, but still audited.
            let seq1 = self.audit(&requester.0, "authorise-action", &id, ts)?;
            (
                ActionState::Authorised { approver: None },
                Disposition::Authorised,
                vec![seq0, seq1],
            )
        };

        self.actions.insert(
            id,
            ActionEntry {
                request: req,
                risk,
                controls,
                state,
                audit_seqs: seqs,
            },
        );
        Ok(disposition)
    }

    /// Approve a pending high-risk action. Enforces separation of duties (the
    /// approver must differ from the requester), a fresh approver re-authentication,
    /// and that the approver is qualified for the action's class — a panel
    /// privilege-logic change requires a security-officer approver. Seals an
    /// `approve-action` audit entry. `[FR-AD-5]` `[AC-AD-3]` `[AC-AD-4]` `[AC-AD-13]`
    pub fn approve(
        &mut self,
        dir: &IdentityDirectory,
        approver_session: &AdminSession,
        id: &str,
        now: u64,
        reauth_max_age: u64,
        ts: &str,
    ) -> Result<(), PrivError> {
        let (requester, scope, class, fresh_reauth) = self.pending_context(id)?;
        let approver = approver_session.admin().clone();

        // Separation of duties: the requester can never approve their own action.
        if approver == requester {
            return Err(PrivError::SelfApprovalDenied);
        }
        if !approver_session.has_mfa() {
            return Err(PrivError::MfaRequired);
        }
        if fresh_reauth && !approver_session.has_fresh_reauth(now, reauth_max_age) {
            return Err(PrivError::StaleReauth);
        }

        // The approver must be qualified for this class.
        match class {
            ActionClass::PanelPrivilegeChange => {
                if !dir.holds_role(&approver, ROLE_SECURITY_OFFICER, &scope, now) {
                    return Err(PrivError::SecurityOfficerRequired);
                }
            }
            ActionClass::BreakGlass => {
                if !dir.has_capability(&approver, "breakglass.approve", &scope, now) {
                    return Err(PrivError::ApproverNotQualified {
                        capability: "breakglass.approve".to_string(),
                    });
                }
            }
            other => {
                let cap = other.required_capability();
                if !dir.has_capability(&approver, cap, &scope, now) {
                    return Err(PrivError::ApproverNotQualified {
                        capability: cap.to_string(),
                    });
                }
            }
        }

        let seq = self.audit(&approver.0, "approve-action", id, ts)?;
        let entry = self
            .actions
            .get_mut(id)
            .expect("checked in pending_context");
        entry.state = ActionState::Authorised {
            approver: Some(approver),
        };
        entry.audit_seqs.push(seq);
        Ok(())
    }

    /// Reject a pending action. Also subject to separation of duties (the requester
    /// cannot dispose of their own action). Seals a `reject-action` audit entry.
    pub fn reject(
        &mut self,
        approver_session: &AdminSession,
        id: &str,
        reason: &str,
        ts: &str,
    ) -> Result<(), PrivError> {
        let (requester, _, _, _) = self.pending_context(id)?;
        let approver = approver_session.admin().clone();
        if approver == requester {
            return Err(PrivError::SelfApprovalDenied);
        }
        let seq = self.audit(&approver.0, "reject-action", id, ts)?;
        let entry = self
            .actions
            .get_mut(id)
            .expect("checked in pending_context");
        entry.state = ActionState::Rejected {
            approver: approver.clone(),
            reason: reason.to_string(),
        };
        entry.audit_seqs.push(seq);
        Ok(())
    }

    /// Seal the **commit** of an authorised action into the same audit chain, linking
    /// it to the action's lineage (request → approve → commit as one transaction).
    /// Fails closed unless the action is authorised. Returns the audit sequence.
    /// [FR-AD-5] [RC-31]
    pub fn record_commit(
        &mut self,
        id: &str,
        committer: &str,
        config_version: u64,
        ts: &str,
    ) -> Result<u64, PrivError> {
        match self.actions.get(id).map(|e| &e.state) {
            Some(ActionState::Authorised { .. }) => {}
            Some(_) => return Err(PrivError::NotPending(id.to_string())),
            None => return Err(PrivError::UnknownAction(id.to_string())),
        }
        let seq = self.audit(
            committer,
            &format!("commit-config:v{config_version}"),
            id,
            ts,
        )?;
        let entry = self.actions.get_mut(id).expect("checked above");
        entry.audit_seqs.push(seq);
        Ok(seq)
    }

    /// Whether the action is authorised to proceed.
    pub fn is_authorised(&self, id: &str) -> bool {
        matches!(
            self.actions.get(id).map(|e| &e.state),
            Some(ActionState::Authorised { .. })
        )
    }

    /// Every action record, keyed by id. The durable store snapshots these to
    /// persist in-flight dual-control state across a restart. [RC-14]
    pub fn records(&self) -> &BTreeMap<String, ActionEntry> {
        &self.actions
    }

    /// Rebuild a service from a durable audit writer and a recovered set of action
    /// records — the recovery path for [RC-14]. The audit chain is the writer's
    /// (a durable `FileLedger`); `actions` is the recovered in-flight state.
    pub fn restore(writer: AuditWriter<S>, actions: BTreeMap<String, ActionEntry>) -> Self {
        Self { writer, actions }
    }

    /// The current state of an action.
    pub fn state_of(&self, id: &str) -> Option<&ActionState> {
        self.actions.get(id).map(|e| &e.state)
    }

    /// The full record for an action.
    pub fn record(&self, id: &str) -> Option<&ActionEntry> {
        self.actions.get(id)
    }

    /// The sealed audit entries (the tamper-evident decision lineage).
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

    /// Extract the fields needed for an approval/rejection decision, asserting the
    /// action exists and is awaiting approval. Returns owned copies so the caller
    /// can subsequently take a mutable borrow to seal audit + update state.
    fn pending_context(
        &self,
        id: &str,
    ) -> Result<(AdminId, ResourceScope, ActionClass, bool), PrivError> {
        let entry = self
            .actions
            .get(id)
            .ok_or_else(|| PrivError::UnknownAction(id.to_string()))?;
        if !matches!(entry.state, ActionState::PendingApproval) {
            return Err(PrivError::NotPending(id.to_string()));
        }
        Ok((
            entry.request.requester.clone(),
            entry.request.scope.clone(),
            entry.request.action_class,
            entry.controls.fresh_reauth,
        ))
    }

    /// Seal one transition into the audit chain, returning its sequence number.
    fn audit(
        &mut self,
        actor: &str,
        action_name: &str,
        resource_id: &str,
        ts: &str,
    ) -> Result<u64, PrivError> {
        let record = AuditRecord {
            id: RecordId(format!("{resource_id}:{action_name}")),
            actor: Actor(actor.to_string()),
            action: Action(action_name.to_string()),
            resource: Resource(resource_id.to_string()),
            ts: Timestamp(ts.to_string()),
        };
        self.writer
            .append(record)
            .map_err(|e| PrivError::Audit(format!("{e:?}")))
    }
}
