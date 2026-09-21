//! # governance — Governance & Audit (the enforcement + lineage spine)
//!
//! The single point where role/marking/purpose policy is enforced and where every
//! decision is written to the append-only, PQ-signed audit ledger. Products don't
//! re-implement enforcement or audit; they call this. [NFR-SEC-2] [NFR-PQ-3]
//!
//! Two entry points:
//! - [`Governance::authorize`] — decide **and** audit: the full envelope. Every
//!   call (permit *or* deny) writes one PQ-signed lineage entry. This is what a
//!   query touching marked data goes through. [AC-M-4]
//! - [`Governance::permits`] — decide only, no audit. For high-cardinality
//!   per-object marking filters where a single query-level audit already records
//!   the access decision.

use std::collections::BTreeSet;

use audit_writer::{
    Action as AuditAction, Actor, AuditRecord, AuditWriter, InMemoryLedger, LedgerId, LedgerStore,
    RecordId, Resource as AuditResource, SealedEntry, Timestamp,
};
use policy::{
    AccessRequest, Action, Clearance, Decision, DefaultPdp, PolicyDecisionPoint, PolicySet,
    Purpose, Resource, Role, Subject, SubjectId,
};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{CryptoError, Rng, SigId, VerifyingKey};

/// The algorithm governance signs audit lineage with (ML-DSA-65 / FIPS 204).
pub const GOV_SIG_ALG: SigId = SigId::MlDsa65;

const GOV_LEDGER: &str = "midgard-governance";

/// Errors from the governance service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GovernanceError {
    /// Writing audit lineage failed.
    Audit(String),
}

impl std::fmt::Display for GovernanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GovernanceError::Audit(s) => write!(f, "audit error: {s}"),
        }
    }
}

impl std::error::Error for GovernanceError {}

/// The caller's governance context (asserted upstream from a PQ-signed token).
#[derive(Debug, Clone)]
pub struct GovContext {
    /// Authenticated principal.
    pub subject: String,
    /// Roles held.
    pub roles: BTreeSet<Role>,
    /// Clearance (level + compartments).
    pub clearance: Clearance,
    /// Declared purpose of access.
    pub purpose: Purpose,
}

impl GovContext {
    fn to_subject(&self) -> Subject {
        let mut purposes = BTreeSet::new();
        purposes.insert(self.purpose.clone());
        Subject {
            id: SubjectId(self.subject.clone()),
            roles: self.roles.clone(),
            clearance: self.clearance.clone(),
            purposes,
        }
    }
}

/// The outcome of an authorised (and audited) decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GovDecision {
    /// Whether access was permitted.
    pub permitted: bool,
    /// The sequence number of the PQ-signed audit entry recording the decision.
    pub audit_seq: u64,
}

/// The governance & audit service. Generic over the audit [`LedgerStore`]: it
/// defaults to the in-memory ledger (tests/dev), but a durable, KMS-sealed
/// [`FileLedger`](audit_writer::FileLedger) can be injected via
/// [`Governance::with_audit`] so lineage survives a restart (RC-09). [AC-M-4]
pub struct Governance<S: LedgerStore = InMemoryLedger> {
    pdp: DefaultPdp,
    audit: AuditWriter<S>,
}

impl Governance<InMemoryLedger> {
    /// Build a governance service over a policy set, generating a fresh ML-DSA
    /// audit signing key from `rng`. The audit trail is **in memory** (non-durable);
    /// for a persistent trail use [`Governance::with_audit`] with a `FileLedger`.
    pub fn new(pdp: DefaultPdp, rng: &mut dyn Rng) -> Result<Self, CryptoError> {
        let provider = fips_provider();
        let (verifying_key, signing_key) = provider.sig_for(GOV_SIG_ALG)?.keypair(rng)?;
        let audit = AuditWriter::new(
            InMemoryLedger::new(),
            Box::new(FipsSig::new(GOV_SIG_ALG)),
            signing_key,
            verifying_key,
            LedgerId(GOV_LEDGER.to_string()),
        );
        Ok(Self { pdp, audit })
    }
}

impl<S: LedgerStore> Governance<S> {
    /// Build a governance service over an explicit audit writer — the seam for a
    /// **durable** ledger: pass an `AuditWriter<FileLedger>` whose `FileLedger` was
    /// opened over a `KmsSealer`, and every decision is persisted, encrypted at
    /// rest, and recoverable after a restart. [RC-09] [AC-M-4]
    pub fn with_audit(pdp: DefaultPdp, audit: AuditWriter<S>) -> Self {
        Self { pdp, audit }
    }

    /// Decide without auditing (for per-object marking filters).
    pub fn permits(&self, ctx: &GovContext, resource: &Resource, action: &str) -> bool {
        self.decide(ctx, resource, action, &ctx.purpose).is_permit()
    }

    /// Decide without auditing, returning the PDP's full, self-describing
    /// [`Decision`] — effect, reasons, matched rules.
    ///
    /// The subject is always the caller's own context. `purpose` is the purpose the
    /// request is made under, which a policy dry-run may set to one the session does
    /// not hold, so the purpose gate reports exactly what a real request would meet.
    pub fn decide(
        &self,
        ctx: &GovContext,
        resource: &Resource,
        action: &str,
        purpose: &Purpose,
    ) -> Decision {
        let subject = ctx.to_subject();
        let req = AccessRequest {
            subject: &subject,
            resource,
            action: Action(action.to_string()),
            purpose: purpose.clone(),
        };
        self.pdp.decide(&req)
    }

    /// The policy set this spine enforces (read-only, for listing).
    pub fn policies(&self) -> &PolicySet {
        self.pdp.policies()
    }

    /// Decide **and** audit. Writes one PQ-signed lineage entry recording the
    /// outcome, whether permit or deny — the governance envelope. [AC-M-4]
    pub fn authorize(
        &mut self,
        ctx: &GovContext,
        resource: &Resource,
        action: &str,
        now: u64,
    ) -> Result<GovDecision, GovernanceError> {
        let permitted = self.permits(ctx, resource, action);
        let outcome = if permitted { "PERMIT" } else { "DENY" };
        let audit_seq = self.write_audit(
            &ctx.subject,
            &format!("{outcome} {action}"),
            &resource.id.0,
            now,
        )?;
        Ok(GovDecision {
            permitted,
            audit_seq,
        })
    }

    /// Record an event that is not a policy decision — a login, say — in the same
    /// PQ-signed ledger every decision is written to, so the lineage has no gaps.
    /// [AC-M-4]
    pub fn record_event(
        &mut self,
        actor: &str,
        action: &str,
        resource: &str,
        now: u64,
    ) -> Result<u64, GovernanceError> {
        self.write_audit(actor, action, resource, now)
    }

    fn write_audit(
        &mut self,
        actor: &str,
        action: &str,
        resource: &str,
        now: u64,
    ) -> Result<u64, GovernanceError> {
        let record = AuditRecord {
            id: RecordId(format!("{action}:{resource}:{now}")),
            actor: Actor(actor.to_string()),
            action: AuditAction(action.to_string()),
            resource: AuditResource(resource.to_string()),
            ts: Timestamp(now.to_string()),
        };
        self.audit
            .append(record)
            .map_err(|e| GovernanceError::Audit(e.to_string()))
    }

    /// The sealed audit entries (for external verification).
    pub fn audit_entries(&self) -> &[SealedEntry] {
        self.audit.entries()
    }
    /// The audit verifying key.
    pub fn audit_verifying_key(&self) -> &VerifyingKey {
        self.audit.verifying_key()
    }
    /// The audit ledger id.
    pub fn audit_ledger_id(&self) -> &LedgerId {
        self.audit.ledger_id()
    }
}
