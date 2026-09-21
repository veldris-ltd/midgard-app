//! The decision returned by the PDP. It is rich and self-describing so the
//! caller (governance service / pq-mtls) can write it to the audit ledger —
//! giving AC-M-4 ("the decision is audited") without the PDP depending on the
//! audit crate.

use crate::model::{ClassificationLevel, Compartment, Effect, Purpose};
use crate::policy::RuleId;

/// Why a decision was reached (machine-readable, for audit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionReason {
    /// Clearance level below the resource marking (no read-up).
    ClearanceInsufficient {
        /// Level required by the marking.
        needed: ClassificationLevel,
        /// Level held by the subject.
        held: ClassificationLevel,
    },
    /// Subject lacks required compartments.
    CompartmentMissing {
        /// The compartments the subject lacks.
        missing: Vec<Compartment>,
    },
    /// The declared purpose is not permitted.
    PurposeNotPermitted {
        /// The offending purpose.
        purpose: Purpose,
    },
    /// An explicit deny rule matched.
    ExplicitDeny {
        /// The denying rule.
        rule: RuleId,
    },
    /// A permit rule matched.
    Permitted {
        /// The permitting rule.
        rule: RuleId,
    },
    /// No rule matched (deny-by-default).
    NoMatchingRule,
}

/// An obligation the caller must honour when acting on the decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Obligation {
    /// Write this decision to the audit ledger.
    Audit,
    /// Redact fields above the given level before returning data.
    RedactAboveLevel(ClassificationLevel),
}

/// The result of evaluating an access request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// Permit or deny.
    pub effect: Effect,
    /// The reasons, in evaluation order.
    pub reasons: Vec<DecisionReason>,
    /// Rules that matched (if any).
    pub matched_rules: Vec<RuleId>,
    /// Obligations for the caller.
    pub obligations: Vec<Obligation>,
}

impl Decision {
    /// Whether the effect is permit.
    pub fn is_permit(&self) -> bool {
        matches!(self.effect, Effect::Permit)
    }
}
