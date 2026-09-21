//! # admin-sim — policy simulation / access dry-run
//!
//! Answers "what would this change grant or deny?" **before** anything is
//! committed. `[FR-AD-6]` `[AC-AD-5]`
//!
//! The key property is fidelity: the simulation is not a separate approximation of
//! the policy — it runs the *same* deny-by-default [`policy::DefaultPdp`] the data
//! plane enforces, over a chosen set of access probes, once against the current
//! policy and once against the proposed policy, and reports every decision that
//! flips. Because it is the real PDP, the simulated effect is exactly the effect a
//! commit would have (the sim-accuracy guarantee behind AC-AD-5).
//!
//! It is pure logic with no side effects and no crypto — trivially air-gapped. The
//! orchestrator records the simulation to the admin audit ledger (outcome
//! `Simulated`) so even a dry-run leaves a signed trace. `[AC-AD-6]`

pub mod probe;
pub mod report;
pub mod simulate;

pub use probe::AccessProbe;
pub use report::SimulationReport;
pub use simulate::{simulate, HIGH_BLAST_GRANTS};
