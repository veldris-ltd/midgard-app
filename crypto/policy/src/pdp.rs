//! The policy-decision point. Evaluation is a fixed pipeline where each stage can
//! only DENY, and the whole thing is deny-by-default:
//!
//! 1. Mandatory (marking/clearance) dominance — a discretionary permit can never
//!    override this.
//! 2. Purpose gate.
//! 3. Discretionary role rules; explicit deny overrides permit.
//! 4. No match → deny.
//!
//! `decide` is total (returns `Decision`, never `Result`) so no caller can
//! mistake an error for a permit.

use crate::decision::{Decision, DecisionReason, Obligation};
use crate::lattice::{dominates, missing_compartments};
use crate::model::{AccessRequest, Effect};
use crate::policy::{rule_matches, PolicySet, RuleId};

/// A component that decides access requests. Object-safe.
pub trait PolicyDecisionPoint {
    /// Decide an access request.
    fn decide(&self, req: &AccessRequest) -> Decision;
}

/// The default deny-by-default PDP over a [`PolicySet`].
pub struct DefaultPdp {
    policies: PolicySet,
}

impl DefaultPdp {
    /// Build a PDP over a policy set.
    pub fn new(policies: PolicySet) -> Self {
        Self { policies }
    }

    /// The policy set this PDP evaluates (read-only, for listing).
    pub fn policies(&self) -> &PolicySet {
        &self.policies
    }
}

fn deny(reasons: Vec<DecisionReason>, matched: Vec<RuleId>) -> Decision {
    Decision {
        effect: Effect::Deny,
        reasons,
        matched_rules: matched,
        obligations: vec![Obligation::Audit],
    }
}

impl PolicyDecisionPoint for DefaultPdp {
    fn decide(&self, req: &AccessRequest) -> Decision {
        // Stage 1 — mandatory access control, always first.
        if !dominates(&req.subject.clearance, &req.resource.marking) {
            let mut reasons = Vec::new();
            if req.subject.clearance.level < req.resource.marking.level {
                reasons.push(DecisionReason::ClearanceInsufficient {
                    needed: req.resource.marking.level,
                    held: req.subject.clearance.level,
                });
            }
            let missing = missing_compartments(&req.subject.clearance, &req.resource.marking);
            if !missing.is_empty() {
                reasons.push(DecisionReason::CompartmentMissing { missing });
            }
            return deny(reasons, Vec::new());
        }

        // Stage 2 — purpose gate.
        let purpose_ok = req.subject.purposes.contains(&req.purpose)
            && req.resource.allowed_purposes.contains(&req.purpose);
        if !purpose_ok {
            return deny(
                vec![DecisionReason::PurposeNotPermitted {
                    purpose: req.purpose.clone(),
                }],
                Vec::new(),
            );
        }

        // Stage 3 — discretionary role rules.
        let mut permits: Vec<RuleId> = Vec::new();
        let mut denies: Vec<RuleId> = Vec::new();
        for rule in self.policies.rules() {
            if rule_matches(rule, req) {
                match rule.effect {
                    Effect::Permit => permits.push(rule.id.clone()),
                    Effect::Deny => denies.push(rule.id.clone()),
                }
            }
        }

        // Stage 4 — explicit deny overrides any permit.
        if let Some(deny_id) = denies.first() {
            return deny(
                vec![DecisionReason::ExplicitDeny {
                    rule: deny_id.clone(),
                }],
                denies,
            );
        }

        if let Some(permit_id) = permits.first() {
            return Decision {
                effect: Effect::Permit,
                reasons: vec![DecisionReason::Permitted {
                    rule: permit_id.clone(),
                }],
                matched_rules: permits,
                obligations: vec![Obligation::Audit],
            };
        }

        // Deny-by-default.
        deny(vec![DecisionReason::NoMatchingRule], Vec::new())
    }
}
