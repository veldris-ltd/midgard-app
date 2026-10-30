//! The break-glass controller: the composed transaction that makes an emergency
//! grant real. It wires the [`BreakGlassService`] (validation + real-time alert), the
//! [`BreakGlassAuthority`] (the scoped, time-boxed capability that actually elevates
//! access), and a PQ-signed audit ledger, so every transition — grant, expiry,
//! review — is sealed. A grant therefore: raises a delivered alert, issues a
//! capability that elevates the grantee within scope, auto-expires, and is audited
//! throughout. [FR-AD-8] [RC-32]
//!
//! A grant takes two passkey-authenticated sessions, the requester's and a different
//! approver's, and seals both identities: a `breakglass-approve` entry whose actor is
//! the approver, then the `breakglass-grant` entry whose actor is the requester.
//! [MG-SEC-07]

use admin_identity::AdminSession;
use audit_writer::{
    Action, Actor, AuditRecord, AuditWriter, LedgerId, LedgerStore, RecordId, Resource,
    SealedEntry, Timestamp,
};
use pqc_agility::VerifyingKey;

use crate::alert::Alerter;
use crate::error::BreakGlassError;
use crate::grant::GrantRequest;
use crate::service::BreakGlassService;
use crate::token::{BreakGlassAuthority, CapabilityToken};

/// Composes break-glass grant + real-time alert + capability elevation + audit.
pub struct BreakGlassController<A: Alerter, S: LedgerStore> {
    service: BreakGlassService<A>,
    authority: BreakGlassAuthority,
    audit: AuditWriter<S>,
}

impl<A: Alerter, S: LedgerStore> BreakGlassController<A, S> {
    /// Build the controller over an alerting break-glass service and an audit writer.
    pub fn new(service: BreakGlassService<A>, audit: AuditWriter<S>) -> Self {
        Self {
            service,
            authority: BreakGlassAuthority::new(),
            audit,
        }
    }

    /// Open a break-glass grant elevating `capability` within the request's scope:
    /// validate both sessions and the request + raise a real-time alert (via the
    /// service), issue a scoped time-boxed [`CapabilityToken`] and register it so the
    /// PDP honours it, and seal a `breakglass-approve` entry (actor: the approver)
    /// and a `breakglass-grant` entry (actor: the requester). Returns the issued
    /// token. [RC-32] [MG-SEC-07]
    #[allow(clippy::too_many_arguments)]
    pub fn grant(
        &mut self,
        req: GrantRequest,
        requester: &AdminSession,
        approver: &AdminSession,
        approval_max_age: u64,
        capability: &str,
        now: u64,
        ts: &str,
    ) -> Result<CapabilityToken, BreakGlassError> {
        // Validates both sessions and the request, then alerts.
        let grant = self
            .service
            .grant(req, requester, approver, now, approval_max_age)?;
        let approver = grant.approver.clone();
        let token = CapabilityToken {
            grant_id: grant.id.clone(),
            principal: grant.principal.clone(),
            scope: grant.scope.clone(),
            capability: capability.to_string(),
            granted_at: grant.granted_at,
            expires_at: grant.expires_at,
        };
        let (grant_id, principal) = (token.grant_id.clone(), token.principal.clone());
        self.authority.register(token.clone());
        self.audit(&approver, "breakglass-approve", &grant_id, ts)?;
        self.audit(&principal, "breakglass-grant", &grant_id, ts)?;
        Ok(token)
    }

    /// The PDP / subsystem gate: is `principal` elevated for `(scope, capability)` at
    /// `now` by an active break-glass token? [RC-32]
    pub fn elevates(&self, principal: &str, scope: &str, capability: &str, now: u64) -> bool {
        self.authority.elevates(principal, scope, capability, now)
    }

    /// Sweep expired grants: raise the one-time `ExpiredUnreviewed` alert (via the
    /// service), revoke each expired token, and seal a `breakglass-expired` audit
    /// entry per grant. Returns the ids that newly expired. [RC-32]
    pub fn sweep(&mut self, now: u64, ts: &str) -> Result<Vec<String>, BreakGlassError> {
        let expired = self.service.sweep(now);
        for id in &expired {
            let principal = self
                .service
                .get(id)
                .map(|g| g.principal.clone())
                .unwrap_or_default();
            self.authority.revoke(id);
            self.audit(&principal, "breakglass-expired", id, ts)?;
        }
        Ok(expired)
    }

    /// Record the mandatory post-hoc review and seal a `breakglass-review` entry.
    pub fn review(
        &mut self,
        id: &str,
        reviewer: &str,
        note: &str,
        now: u64,
        ts: &str,
    ) -> Result<(), BreakGlassError> {
        self.service.review(id, reviewer, note, now)?;
        self.audit(reviewer, "breakglass-review", id, ts)?;
        Ok(())
    }

    fn audit(
        &mut self,
        actor: &str,
        action: &str,
        grant_id: &str,
        ts: &str,
    ) -> Result<u64, BreakGlassError> {
        let record = AuditRecord {
            id: RecordId(format!("{grant_id}:{action}")),
            actor: Actor(actor.to_string()),
            action: Action(action.to_string()),
            resource: Resource(grant_id.to_string()),
            ts: Timestamp(ts.to_string()),
        };
        self.audit
            .append(record)
            .map_err(|e| BreakGlassError::Audit(format!("{e:?}")))
    }

    /// The underlying grant service (state, pending reviews, alerter).
    pub fn service(&self) -> &BreakGlassService<A> {
        &self.service
    }

    /// Mutable access to the grant service (e.g. to flush the durable alerter).
    pub fn service_mut(&mut self) -> &mut BreakGlassService<A> {
        &mut self.service
    }

    /// The elevation authority (for PDP wiring / inspection).
    pub fn authority(&self) -> &BreakGlassAuthority {
        &self.authority
    }

    /// The sealed audit trail of break-glass transitions.
    pub fn audit_entries(&self) -> &[SealedEntry] {
        self.audit.entries()
    }

    /// The audit verifying key.
    pub fn verifying_key(&self) -> &VerifyingKey {
        self.audit.verifying_key()
    }

    /// The audit ledger id.
    pub fn ledger_id(&self) -> &LedgerId {
        self.audit.ledger_id()
    }
}
