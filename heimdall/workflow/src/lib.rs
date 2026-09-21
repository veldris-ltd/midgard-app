//! # heimdall-workflow — human-gated, audited action workflows
//!
//! Heimdall never auto-executes a consequential action. A [`ProposedAction`] enters
//! the [`WorkflowEngine`] in the `Proposed` state and can only reach `Executed`
//! after an explicit human authorisation; a consequential action attempted without
//! it fails closed with [`WorkflowError::NotAuthorised`]. Every transition —
//! proposed, authorised, denied, executed — is sealed into a PQ-signed, append-only
//! [`audit_writer`] chain, giving full, tamper-evident decision lineage. [FR-H-5]
//! [NFR-AI-1] [AC-H-4]
//!
//! ## AI proposes, humans dispose
//! A proposed action can be built from an advisory A.8 option
//! ([`ProposedAction::from_course_of_action`]), carrying that option's cited
//! evidence into the audit record — so the lineage runs from the feed evidence,
//! through the AI's cited option, to the human who authorised the action. The AI is
//! never in the execution path. [FR-H-4] [FR-H-5]
//!
//! ## Real, audited actuation
//! Execution is not a recorded string: an authorised action invokes a real
//! [`Actuator`](actuator::Actuator) behind a seam
//! ([`DispatchActuator`](actuator::DispatchActuator) routes to registered
//! [`Effector`](actuator::Effector)s), and both the execution intent and the
//! effector's outcome are sealed into the audit chain. An action the actuator
//! cannot invoke fails closed and stays authorised for retry. [RT-44] [RC-44]

pub mod action;
pub mod actuator;
pub mod engine;
pub mod error;

pub use action::ProposedAction;
pub use actuator::{
    ActuationOutcome, ActuationStatus, Actuator, ActuatorError, DispatchActuator, Effector,
};
pub use engine::{ActionRecord, WorkflowEngine, WorkflowState};
pub use error::WorkflowError;
