//! # forge-health — data-health expectations & the promotion gate
//!
//! Health is enforced on **every build**. A [`HealthSuite`] of declarative
//! [`Expectation`]s is evaluated against a build's output; [`run_gated`] promotes
//! (commits) the output only if every expectation passes, and otherwise **blocks
//! promotion** and returns a [`HealthReport`] explaining the failure. Nothing is
//! written to the promotion branch on failure. [FR-F-6] [AC-F-3]
//!
//! The gate sits between the pipeline's `build` (compile + execute, uncommitted)
//! and `commit_build` (seal + promote) steps, so a failed check cannot leave a
//! partially-promoted dataset behind.

pub mod expectation;
pub mod gate;
pub mod suite;

pub use expectation::{Expectation, ExpectationOutcome};
pub use gate::{run_gated, GateOutcome};
pub use suite::{HealthReport, HealthSuite};
