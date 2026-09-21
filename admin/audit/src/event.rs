//! The admin audit event and its injective encoding into a signed audit record.

use crate::error::AdminAuditError;

/// The reserved separator (ASCII Unit Separator) between the packed detail fields.
/// It must not appear in any encoded field, which keeps the encoding injective.
const US: char = '\u{1f}';

/// The outcome of an admin action. Denied and simulated outcomes are recorded just
/// like successful ones — no admin action escapes the ledger. `[AC-AD-6]`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminOutcome {
    /// A change was committed.
    Committed,
    /// A prior version was restored (a forward, append-only rollback).
    RolledBack,
    /// The action was denied (policy, separation-of-duties, validation, …).
    Denied,
    /// A dry-run / simulation was performed (no state changed).
    Simulated,
    /// Emergency break-glass elevation was granted.
    BreakGlassGranted,
    /// A break-glass grant reached its time-box and expired.
    BreakGlassExpired,
    /// A break-glass grant received its mandatory post-hoc review.
    BreakGlassReviewed,
}

impl AdminOutcome {
    /// The stable wire token for this outcome.
    pub fn as_str(self) -> &'static str {
        match self {
            AdminOutcome::Committed => "committed",
            AdminOutcome::RolledBack => "rolled-back",
            AdminOutcome::Denied => "denied",
            AdminOutcome::Simulated => "simulated",
            AdminOutcome::BreakGlassGranted => "break-glass-granted",
            AdminOutcome::BreakGlassExpired => "break-glass-expired",
            AdminOutcome::BreakGlassReviewed => "break-glass-reviewed",
        }
    }
}

/// An admin action to be recorded. `who`/`what`/`when` map to the native audit
/// fields; `why`/`approver`/`outcome` are packed into the signed detail. `[AC-AD-6]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminEvent {
    /// The actor who performed (or attempted) the action.
    pub who: String,
    /// The action performed (a stable verb, e.g. `config-commit`, `simulate`).
    pub what: String,
    /// The resource the action targeted.
    pub resource: String,
    /// Why the action was taken (justification).
    pub why: String,
    /// The distinct approver, where one applied (dual-control / break-glass).
    pub approver: Option<String>,
    /// The outcome (committed, denied, simulated, …).
    pub outcome: AdminOutcome,
}

impl AdminEvent {
    /// Construct an event; the builder-free path for common cases.
    pub fn new(
        who: impl Into<String>,
        what: impl Into<String>,
        resource: impl Into<String>,
        why: impl Into<String>,
        approver: Option<String>,
        outcome: AdminOutcome,
    ) -> Self {
        Self {
            who: who.into(),
            what: what.into(),
            resource: resource.into(),
            why: why.into(),
            approver,
            outcome,
        }
    }

    /// Validate the event, then encode `resource ‖ why ‖ approver ‖ outcome` into a
    /// single injective detail string for the signed `resource` audit field.
    pub(crate) fn encode(&self) -> Result<String, AdminAuditError> {
        if self.who.is_empty() {
            return Err(AdminAuditError::MissingWho);
        }
        if self.what.is_empty() {
            return Err(AdminAuditError::MissingWhat);
        }
        if self.resource.contains(US) {
            return Err(AdminAuditError::InvalidField { field: "resource" });
        }
        if self.why.contains(US) {
            return Err(AdminAuditError::InvalidField { field: "why" });
        }
        let approver = match &self.approver {
            None => String::new(),
            Some(a) if a.is_empty() || a.contains(US) => {
                return Err(AdminAuditError::InvalidField { field: "approver" })
            }
            Some(a) => a.clone(),
        };
        Ok(format!(
            "{}{US}{}{US}{}{US}{}",
            self.resource,
            self.why,
            approver,
            self.outcome.as_str()
        ))
    }
}

/// The decoded detail packed into an audit record's `resource` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminDetail {
    /// The resource the action targeted.
    pub resource: String,
    /// The justification.
    pub why: String,
    /// The approver, if any.
    pub approver: Option<String>,
    /// The outcome token (see [`AdminOutcome::as_str`]).
    pub outcome: String,
}

/// Reverse [`AdminEvent::encode`]: recover the packed detail from a sealed audit
/// record's `resource` field. Returns `None` if the string is not well-formed.
pub fn decode_detail(s: &str) -> Option<AdminDetail> {
    let mut parts = s.split(US);
    let resource = parts.next()?.to_string();
    let why = parts.next()?.to_string();
    let approver = parts.next()?;
    let outcome = parts.next()?.to_string();
    if parts.next().is_some() {
        return None;
    }
    Some(AdminDetail {
        resource,
        why,
        approver: if approver.is_empty() {
            None
        } else {
            Some(approver.to_string())
        },
        outcome,
    })
}
