//! # oag-retriever — Ontology-Augmented Generation grounding
//!
//! Grounds Mimir's generation in the Midgard object fabric: it formulates a typed
//! retrieval plan from a task (the A.2 planner), executes it through the fabric's
//! governed query path, and binds an `obj:<id>` citation to every retrieved
//! object. Two invariants hold by construction:
//! - **Only existing types are referenced** — the planner draws exclusively from
//!   the live [`summary::OntologySummary`]. [FR-MI-2]
//! - **Never above clearance** — retrieval runs through
//!   [`fabric::Fabric::query`], which authorises each object against the caller's
//!   clearance/purpose. [NFR-SEC-2]
//!
//! The cited evidence it produces is what lets a downstream answer cite exactly
//! what it used (AC-MI-2).

pub mod ground;
pub mod plan;
pub mod summary;

pub use ground::{ground, Grounding, GroundingFact};
pub use plan::{formulate_plan, Retrieval, RetrievalPlan, DEFAULT_MAX_RESULTS};
pub use summary::{LinkCard, OntologySummary, TypeCard};

// The governance context type the fabric authorises against — re-exported so
// callers use one surface.
pub use fabric::PolicyContext;

use fabric::{Fabric, LedgerStore, ObjectInstance, ObjectStore, Query};

/// The result of one fabric retrieval: authorised objects + the audit seq.
#[derive(Debug, Clone)]
pub struct Retrieved {
    /// Clearance-authorised objects.
    pub objects: Vec<ObjectInstance>,
    /// The PQ-signed audit entry recording the read.
    pub audit_seq: u64,
}

/// The seam the grounder retrieves through. Implemented for the real
/// [`fabric::Fabric`]; a test double can stand in without a full fabric.
pub trait GroundingSource {
    /// The ontology the planner may reference.
    fn ontology_summary(&self) -> OntologySummary;
    /// Execute one governed, audited retrieval.
    fn retrieve(&mut self, ctx: &PolicyContext, q: &Query, now: u64) -> Result<Retrieved, String>;
}

impl<S: ObjectStore, L: LedgerStore> GroundingSource for Fabric<S, L> {
    fn ontology_summary(&self) -> OntologySummary {
        OntologySummary::from_registry(self.registry())
    }
    fn retrieve(&mut self, ctx: &PolicyContext, q: &Query, now: u64) -> Result<Retrieved, String> {
        // Disambiguate the inherent method (this is not a recursive call).
        let resp = Fabric::query(self, ctx, q, now).map_err(|e| e.to_string())?;
        Ok(Retrieved {
            objects: resp.results,
            audit_seq: resp.audit_seq,
        })
    }
}
