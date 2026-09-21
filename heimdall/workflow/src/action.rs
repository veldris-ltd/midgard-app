//! Proposed actions. An action is *proposed* — never executed — until a human
//! authorises it through the [`crate::engine::WorkflowEngine`]. A consequential
//! action mutates real-world or system state and therefore requires that gate.
//! [FR-H-5]

use std::collections::BTreeMap;

use heimdall_decision_support::CourseOfAction;

/// An action proposed for the human-gated workflow. It carries the evidence it rests
/// on (feed/entity citations) so the audit lineage is self-describing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedAction {
    /// Stable action id (the workflow ticket).
    pub id: String,
    /// The action type (e.g. `"cue-sensor"`, `"dispatch-asset"`).
    pub action_type: String,
    /// Typed parameters.
    pub params: BTreeMap<String, String>,
    /// Why this action is proposed.
    pub rationale: String,
    /// The cited evidence backing it (raw `entity:`/`feed:`/`track:` tokens).
    pub citations: Vec<String>,
    /// Whether the action mutates state and so requires human authorisation.
    pub consequential: bool,
}

impl ProposedAction {
    /// A new consequential action with no params or citations.
    pub fn new(
        id: impl Into<String>,
        action_type: impl Into<String>,
        rationale: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            action_type: action_type.into(),
            params: BTreeMap::new(),
            rationale: rationale.into(),
            citations: Vec::new(),
            consequential: true,
        }
    }

    /// Set a parameter (builder).
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }

    /// Set the citations (builder).
    pub fn with_citations(mut self, citations: Vec<String>) -> Self {
        self.citations = citations;
        self
    }

    /// Mark whether the action is consequential (default `true`).
    pub fn set_consequential(mut self, consequential: bool) -> Self {
        self.consequential = consequential;
        self
    }

    /// Build a proposed action from an advisory A.8 option, carrying the option's
    /// rationale and cited evidence into the (later audited) action. The option is
    /// advisory; this is the operator turning a chosen option into a proposal that
    /// still requires their explicit authorisation. [FR-H-4] [FR-H-5]
    pub fn from_course_of_action(
        id: impl Into<String>,
        action_type: impl Into<String>,
        coa: &CourseOfAction,
    ) -> Self {
        Self {
            id: id.into(),
            action_type: action_type.into(),
            params: BTreeMap::new(),
            rationale: coa.summary.clone(),
            citations: coa.citations.iter().map(|c| c.raw.clone()).collect(),
            consequential: true,
        }
    }
}
