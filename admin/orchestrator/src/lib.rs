//! # admin-orchestrator — the guarded-action transaction [FR-AD-5]
//!
//! Every mutating admin action traverses **one enforced transaction**, composed from
//! the admin subsystems:
//!
//! 1. **authenticate + submit** — [`admin_privilege::PrivilegeService::submit`]
//!    verifies the requester's WebAuthn-gated session (MFA + step-up), their
//!    capability within scope, and the action's risk ceiling, and seals a
//!    `request-action` audit entry;
//! 2. **simulate** — the real deny-by-default PDP dry-run ([`admin_sim::simulate`])
//!    over the current vs proposed policy; a commit cannot proceed without it;
//! 3. **dual-control** — for risk ≥ 4 the action is parked until a *distinct*,
//!    qualified approver approves ([`admin_privilege::PrivilegeService::approve`],
//!    which enforces separation of duties + fresh re-auth + audit);
//! 4. **single-use time-boxed token** — only an authorised action is granted a
//!    [`CommitToken`], burnt on first use and valid only within its window;
//! 5. **commit + audit** — the config change is committed to the append-only,
//!    PQ-signed [`admin_config::ConfigStore`], and the commit is sealed into the
//!    *same* audit chain, linked to the action's lineage.
//!
//! A high-risk config change therefore cannot commit with one author, without a
//! simulation, or without being audited as one linked transaction. [RC-31]
//!
//! This is the **only** sanctioned commit path for admin config mutations; the raw
//! [`admin_config::ConfigStore::commit`] is the low-level primitive it wraps (and
//! that durable reload uses), not an entry point for operators.

pub mod error;
pub mod token;

use admin_config::{Commit, ConfigState, ConfigStore};
use admin_identity::{AdminSession, IdentityDirectory};
use admin_privilege::{AdminActionRequest, Disposition, PrivilegeService};
use admin_sim::{simulate, AccessProbe, SimulationReport};
use audit_writer::LedgerStore;
use policy::PolicySet;
use std::collections::BTreeMap;

pub use error::OrchestratorError;
pub use token::{CommitToken, TokenStore};

/// The outcome of submitting a guarded action.
pub struct Submission {
    /// Whether the action was authorised immediately (single actor) or parked for a
    /// distinct approver (dual-control).
    pub disposition: Disposition,
    /// The policy dry-run report for the proposed change.
    pub report: SimulationReport,
    /// A commit token — present only when the action is already authorised (low/
    /// medium risk). High-risk actions receive theirs from [`GuardedActionOrchestrator::approve`].
    pub token: Option<CommitToken>,
}

/// Composes identity + privilege + sim + config + audit into the single enforced
/// admin-action transaction. [RC-31]
pub struct GuardedActionOrchestrator<S: LedgerStore> {
    privilege: PrivilegeService<S>,
    config: ConfigStore,
    tokens: TokenStore,
    token_ttl: u64,
    /// Actions that have been simulated (action id → dry-run report). Presence is the
    /// "was simulated" gate the commit step enforces.
    simulated: BTreeMap<String, SimulationReport>,
}

impl<S: LedgerStore> GuardedActionOrchestrator<S> {
    /// Build the orchestrator over the dual-control service and the config store.
    /// `token_ttl` is the commit-token time-box (seconds).
    pub fn new(privilege: PrivilegeService<S>, config: ConfigStore, token_ttl: u64) -> Self {
        Self {
            privilege,
            config,
            tokens: TokenStore::new(),
            token_ttl,
            simulated: BTreeMap::new(),
        }
    }

    /// Step 1–2: authenticate + submit the action, then simulate the proposed policy
    /// change. A single-actor (low/medium-risk) action is authorised here and gets a
    /// commit token; a high-risk action is parked for approval (no token yet). The
    /// dry-run report is retained so the commit step can enforce "must be simulated".
    /// [RC-31]
    #[allow(clippy::too_many_arguments)]
    pub fn submit(
        &mut self,
        dir: &IdentityDirectory,
        session: &AdminSession,
        req: AdminActionRequest,
        current: &PolicySet,
        proposed: &PolicySet,
        probes: &[AccessProbe],
        now: u64,
        step_up_max_age: u64,
        ts: &str,
    ) -> Result<Submission, OrchestratorError> {
        let id = req.id.clone();
        // 1. Authenticate + risk-classify + audit the request (fails closed on a bad
        //    session, missing capability, or over-ceiling risk).
        let disposition = self
            .privilege
            .submit(dir, session, req, now, step_up_max_age, ts)?;
        // 2. Simulate the proposed change with the real PDP and retain the report.
        let report = simulate(current, proposed, probes);
        self.simulated.insert(id.clone(), report.clone());
        // A single-actor action is already authorised → issue its commit token now.
        let token = match disposition {
            Disposition::Authorised => Some(self.tokens.issue(&id, now, self.token_ttl)),
            Disposition::NeedsApproval => None,
        };
        Ok(Submission {
            disposition,
            report,
            token,
        })
    }

    /// Step 3–4: a distinct, qualified approver approves a parked (high-risk) action;
    /// on success — and only if the action was simulated — a single-use time-boxed
    /// commit token is granted. Separation of duties, approver qualification, and
    /// fresh re-auth are enforced by the privilege service (and audited). [RC-31]
    pub fn approve(
        &mut self,
        dir: &IdentityDirectory,
        approver_session: &AdminSession,
        id: &str,
        now: u64,
        reauth_max_age: u64,
        ts: &str,
    ) -> Result<CommitToken, OrchestratorError> {
        self.privilege
            .approve(dir, approver_session, id, now, reauth_max_age, ts)?;
        if !self.simulated.contains_key(id) {
            return Err(OrchestratorError::NotSimulated);
        }
        Ok(self.tokens.issue(id, now, self.token_ttl))
    }

    /// Step 5: commit. Validates the single-use time-boxed token, re-checks that the
    /// action is authorised and was simulated, commits the new config state to the
    /// append-only PQ-signed store, and seals the commit into the *same* audit chain,
    /// linked to the action's lineage. Any failure leaves the store unchanged. [RC-31]
    pub fn commit(
        &mut self,
        token: &CommitToken,
        state: ConfigState,
        message: &str,
        now: u64,
        ts: &str,
    ) -> Result<Commit, OrchestratorError> {
        let id = &token.action_id;
        // Single-use + time-box + action binding.
        self.tokens.consume(&token.secret, id, now)?;
        // Dual-control result must hold (a high-risk action needs a distinct approver).
        if !self.privilege.is_authorised(id) {
            return Err(OrchestratorError::NotAuthorised);
        }
        // Must have been simulated.
        if !self.simulated.contains_key(id) {
            return Err(OrchestratorError::NotSimulated);
        }
        // The committer/author is the action's requester.
        let committer = self
            .privilege
            .record(id)
            .map(|e| e.request.requester.0.clone())
            .ok_or(OrchestratorError::NotAuthorised)?;
        // Commit the mutation to the append-only signed config history.
        let commit = self.config.commit(state, &committer, message)?.clone();
        // Seal the commit into the SAME audit chain, linked to the action lineage.
        self.privilege
            .record_commit(id, &committer, commit.version, ts)?;
        Ok(commit)
    }

    /// The dual-control service (audit chain, action records).
    pub fn privilege(&self) -> &PrivilegeService<S> {
        &self.privilege
    }

    /// The config store (commit history).
    pub fn config(&self) -> &ConfigStore {
        &self.config
    }

    /// The retained dry-run report for an action, if it was simulated.
    pub fn simulation(&self, id: &str) -> Option<&SimulationReport> {
        self.simulated.get(id)
    }
}
