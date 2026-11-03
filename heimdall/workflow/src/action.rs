//! Proposed actions. An action is *proposed* — never executed — until a human
//! authorises it through the [`crate::engine::WorkflowEngine`]. Every action needs
//! that decision before it runs; whether it is consequential is looked up in the
//! deployment's [`ActionTypeCatalog`](crate::catalog::ActionTypeCatalog) when it is
//! proposed, never asserted by the proposer. [FR-H-5] [VLD-ANB-MIDGARD-0045]

use std::collections::BTreeMap;

use heimdall_decision_support::GroundedOption;

use crate::catalog::ActionClass;

/// An action proposed for the human-gated workflow. It carries the evidence it rests
/// on (feed/entity citations) so the audit lineage is self-describing.
///
/// A proposer cannot assert the class: there is no public setter, and the class
/// field is private:
///
/// ```compile_fail,E0616
/// use heimdall_workflow::{ActionClass, ProposedAction};
/// let mut a = ProposedAction::new("t", "dispatch-asset", "why");
/// a.class = ActionClass::Informational;
/// ```
///
/// [AC-64.1] [VLD-ANB-MIDGARD-0045]
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
    /// The action's class. Private: the engine sets it from the catalogue when the
    /// action is proposed, so a proposer cannot label a consequential type
    /// informational. Consequential until classified. [VLD-ANB-MIDGARD-0045]
    class: ActionClass,
}

impl ProposedAction {
    /// A new action with no params or citations. Its class is consequential until
    /// the engine classifies it from the catalogue at proposal.
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
            class: ActionClass::Consequential,
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

    /// The action's class, as the catalogue gave it at proposal.
    pub fn class(&self) -> ActionClass {
        self.class
    }

    /// Whether the action's class is [`ActionClass::Consequential`].
    pub fn is_consequential(&self) -> bool {
        self.class == ActionClass::Consequential
    }

    /// Set the class from the deployment's catalogue. Only the engine (at proposal
    /// and when given a catalogue) calls this. [VLD-ANB-MIDGARD-0045]
    pub(crate) fn classify(&mut self, class: ActionClass) {
        self.class = class;
    }

    /// Build a proposed action from a **grounded** advisory A.8 option, carrying the
    /// option's rationale and cited evidence into the (later audited) action. Only
    /// the decision-support adapter can make a [`GroundedOption`], and only from an
    /// option whose every citation resolved against the picture, so the audit
    /// lineage cannot carry invented evidence. The option is advisory; this is the
    /// operator turning a chosen option into a proposal that still requires their
    /// explicit authorisation. [FR-H-4] [FR-H-5] [VLD-ANB-MIDGARD-0079]
    pub fn from_course_of_action(
        id: impl Into<String>,
        action_type: impl Into<String>,
        option: &GroundedOption,
    ) -> Self {
        Self {
            id: id.into(),
            action_type: action_type.into(),
            params: BTreeMap::new(),
            rationale: option.summary().to_string(),
            citations: option.citations().iter().map(|c| c.raw.clone()).collect(),
            class: ActionClass::Consequential,
        }
    }
}
