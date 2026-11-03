//! # heimdall-workflow — human-gated, audited action workflows
//!
//! Heimdall never auto-executes an action. A [`ProposedAction`] enters the
//! [`WorkflowEngine`] in the `Proposed` state and can only reach `Executed` after an
//! explicit human authorisation; any action attempted without it, informational or
//! consequential, fails closed with [`WorkflowError::NotAuthorised`]. Every
//! transition —
//! proposed, authorised, denied, executed — is sealed into a PQ-signed, append-only
//! [`audit_writer`] chain, giving full, tamper-evident decision lineage. [FR-H-5]
//! [NFR-AI-1] [AC-H-4]
//!
//! ## The class is the deployment's, not the proposer's
//! Whether an action is consequential is looked up in the deployment's
//! [`ActionTypeCatalog`] when it is proposed: a type the catalogue does not list is
//! refused with [`WorkflowError::UnknownActionType`] before anything is audited, and
//! no public API lets a proposer set the class. An empty catalogue refuses every
//! proposal. The [`DispatchActuator`] re-checks the type and class against the same
//! catalogue before invoking an effector. A hosting service loads its catalogue
//! with [`ActionTypeCatalog::parse`] from a file such as
//! `deploy/compose/heimdall-actions.catalog` (the demo action types). No running
//! service hosts the workflow engine yet; it is implemented and tested as a library.
//! [VLD-ANB-MIDGARD-0045]
//!
//! ## AI proposes, humans dispose
//! A proposed action can be built from a grounded advisory A.8 option
//! ([`ProposedAction::from_course_of_action`] takes only a
//! [`GroundedOption`](heimdall_decision_support::GroundedOption), whose every
//! citation resolved against the picture), carrying that option's cited evidence
//! into the audit record — so the lineage runs from the feed evidence,
//! through the AI's cited option, to the human who authorised the action. The AI is
//! never in the execution path. [FR-H-4] [FR-H-5] [VLD-ANB-MIDGARD-0079]
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
pub mod catalog;
pub mod engine;
pub mod error;

pub use action::ProposedAction;
pub use actuator::{
    ActuationOutcome, ActuationStatus, Actuator, ActuatorError, DispatchActuator, Effector,
};
pub use catalog::{ActionClass, ActionTypeCatalog};
pub use engine::{ActionRecord, WorkflowEngine, WorkflowState};
pub use error::WorkflowError;
