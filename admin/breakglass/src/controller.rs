//! The break-glass controller: the composed transaction that makes an emergency
//! grant real. It wires a [`GrantStore`] (validation + real-time alert), the
//! [`BreakGlassAuthority`] (the scoped, time-boxed capability that actually elevates
//! access), and a PQ-signed audit ledger, so every transition — grant, expiry,
//! review — is sealed. A grant therefore: raises a delivered alert, issues a
//! capability that elevates the grantee within scope, auto-expires, and is audited
//! throughout. [FR-AD-8] [RC-32]
//!
//! A grant takes two passkey-authenticated sessions, the requester's and a different
//! approver's, and seals both identities: a `breakglass-approve` entry whose actor is
//! the approver, then the `breakglass-grant` entry whose actor is the requester.
//! Both must hold their capability (`breakglass.request`, `breakglass.approve`) over
//! the grant's scope in the identity directory, or nothing is issued. [MG-SEC-07]
//!
//! Review and close revoke the grant's capability first, before anything that can
//! fail: the store's journal write and the audit entry come after. A grant that has
//! been reviewed, or ended early, no longer elevates anyone even though its time-box
//! has not run out, and a storage or audit failure on the way leaves the access off,
//! not on. Both can be retried. [VLD-ANB-MIDGARD-0055]
//!
//! In a deployment the store is the [`DurableBreakGlass`](crate::DurableBreakGlass)
//! journal, so grants, closes, expiries and reviews survive a restart. The
//! capability tokens that elevate do not: they are held in memory, so a restart
//! drops every elevation, even for a grant still within its time-box. That fails
//! closed. Emergency access needed after a restart is granted again, with a fresh
//! justification and approval. [RC-14] [VLD-ANB-MIDGARD-0055]

use admin_identity::{AdminSession, IdentityDirectory};
use audit_writer::{
    Action, Actor, AuditRecord, AuditWriter, LedgerId, LedgerStore, RecordId, Resource,
    SealedEntry, Timestamp,
};
use pqc_agility::VerifyingKey;

use crate::alert::Alerter;
use crate::durable::DurableBreakGlass;
use crate::error::BreakGlassError;
use crate::grant::GrantRequest;
use crate::service::{BreakGlassService, GrantStore};
use crate::token::{BreakGlassAuthority, CapabilityToken};

/// Composes break-glass grant + real-time alert + capability elevation + audit.
/// The grant store `G` is the durable journal unless a test names the in-memory
/// [`BreakGlassService`].
pub struct BreakGlassController<A: Alerter, S: LedgerStore, G: GrantStore<A> = DurableBreakGlass<A>>
{
    service: G,
    authority: BreakGlassAuthority,
    audit: AuditWriter<S>,
    _alerter: std::marker::PhantomData<A>,
}

impl<A: Alerter, S: LedgerStore, G: GrantStore<A>> BreakGlassController<A, S, G> {
    /// Build the controller over an alerting grant store and an audit writer. A
    /// store reopened after a restart brings its grants back, but no elevation: see
    /// the module documentation.
    pub fn new(service: G, audit: AuditWriter<S>) -> Self {
        Self {
            service,
            authority: BreakGlassAuthority::new(),
            audit,
            _alerter: std::marker::PhantomData,
        }
    }

    /// Open a break-glass grant elevating `capability` within the request's scope:
    /// validate both sessions, their role bindings in `dir`, and the request + raise a real-time alert (via the
    /// service), issue a scoped time-boxed [`CapabilityToken`] and register it so the
    /// PDP honours it, and seal a `breakglass-approve` entry (actor: the approver)
    /// and a `breakglass-grant` entry (actor: the requester). Returns the issued
    /// token. [RC-32] [MG-SEC-07]
    #[allow(clippy::too_many_arguments)]
    pub fn grant(
        &mut self,
        dir: &IdentityDirectory,
        req: GrantRequest,
        requester: &AdminSession,
        approver: &AdminSession,
        approval_max_age: u64,
        capability: &str,
        now: u64,
        ts: &str,
    ) -> Result<CapabilityToken, BreakGlassError> {
        // Validates both sessions, their entitlement and the request, then alerts.
        let grant = self
            .service
            .grant(dir, req, requester, approver, now, approval_max_age)?;
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
        let expired = self.service.sweep(now)?;
        for id in &expired {
            let principal = self
                .service
                .service()
                .get(id)
                .map(|g| g.principal.clone())
                .unwrap_or_default();
            self.authority.revoke(id);
            self.audit(&principal, "breakglass-expired", id, ts)?;
        }
        Ok(expired)
    }

    /// Revoke the grant's capability, record the mandatory post-hoc review, and seal
    /// a `breakglass-review` entry. A grant reviewed before its time-box runs out
    /// stops elevating at once, even if the review cannot be recorded: the
    /// revocation comes first and cannot fail, and a review whose write failed can
    /// be retried. [VLD-ANB-MIDGARD-0055]
    pub fn review(
        &mut self,
        id: &str,
        reviewer: &str,
        note: &str,
        now: u64,
        ts: &str,
    ) -> Result<(), BreakGlassError> {
        self.authority.revoke(id);
        self.service.review(id, reviewer, note, now)?;
        self.audit(reviewer, "breakglass-review", id, ts)?;
        Ok(())
    }

    /// End a grant early: revoke its capability, cut its time-box to `now` (via the
    /// store, so it enters the review backlog at once and stays closed after a
    /// restart), and seal a `breakglass-closed` entry whose actor is `closer`. The
    /// post-hoc review is still required.
    ///
    /// The revocation comes first and cannot fail, so a journal or audit failure
    /// leaves the grant unable to elevate, and the error says the close was not
    /// recorded. Closing again is safe: a grant already closed at or before `now`
    /// keeps its time-box, and the record is written again. [VLD-ANB-MIDGARD-0055]
    pub fn close(
        &mut self,
        id: &str,
        closer: &str,
        now: u64,
        ts: &str,
    ) -> Result<(), BreakGlassError> {
        self.authority.revoke(id);
        self.service.close(id, now)?;
        self.audit(closer, "breakglass-closed", id, ts)?;
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
            context: Default::default(),
        };
        self.audit
            .append(record)
            .map_err(|e| BreakGlassError::Audit(format!("{e:?}")))
    }

    /// The underlying grant service (state, pending reviews, alerter).
    pub fn service(&self) -> &BreakGlassService<A> {
        self.service.service()
    }

    /// Mutable access to the alert sink (e.g. to flush the durable alerter). The
    /// grants themselves change only through the controller, so each change is
    /// journalled and audited.
    pub fn alerter_mut(&mut self) -> &mut A {
        self.service.alerter_mut()
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
