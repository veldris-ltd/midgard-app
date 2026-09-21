//! # policy — role / marking / purpose policy-decision point
//!
//! Deny-by-default access decisions combining Bell–LaPadula mandatory access
//! control (level + compartments), a purpose gate, and discretionary role rules
//! (explicit-deny-overrides-permit). `[NFR-SEC-2]` `[STD:NIST AC-3/AC-16]`
//!
//! Pure logic with **zero dependencies**: it depends on neither the crypto nor
//! the audit crate, yet every [`Decision`] is self-describing enough for the
//! caller to write to the audit ledger — the clean seam behind AC-M-4.

pub mod decision;
pub mod error;
pub mod lattice;
pub mod model;
pub mod pdp;
pub mod policy;

pub use crate::policy::{PolicySet, Rule, RuleId, Selector};
pub use decision::{Decision, DecisionReason, Obligation};
pub use error::PolicyError;
pub use lattice::{dominates, missing_compartments};
pub use model::{
    AccessRequest, Action, ClassificationLevel, Clearance, Compartment, Effect, Marking, Purpose,
    Resource, ResourceId, Role, Subject, SubjectId,
};
pub use pdp::{DefaultPdp, PolicyDecisionPoint};
