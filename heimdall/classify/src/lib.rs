//! # heimdall-classify — entity classification behind a seam
//!
//! Classification is a trait, [`Classifier`], so the label source is swappable. This
//! phase ships the [`RulesClassifier`]: a deterministic rule engine over an entity's
//! kind and attributes that needs no model, no network, and no clock — the
//! always-available path that keeps the picture classified when Mimir is absent and
//! when the site is air-gapped. [FR-H-2] [AC-H-6] [AC-H-7]
//!
//! Prompt 6.c adds a Mimir-backed classifier behind this same [`Classifier`] trait;
//! the rules engine remains the fallback and the correctness oracle. Every label
//! carries its [`heimdall_picture::ClassificationSource`] and a rationale, so a
//! classification is never anonymous. [FR-H-2]

pub mod rules;

pub use rules::{Condition, Rule, RulesClassifier};

use heimdall_picture::{Classification, TrackedEntity};

/// A source of entity classifications. Deterministic implementations (the rules
/// engine) return the same label for the same entity on every host, offline.
///
/// `Send + Sync`, so a fusion engine that holds one can be served from the Heimdall
/// feed service's connection threads. [FR-H-2]
pub trait Classifier: Send + Sync {
    /// Classify an entity, or return `None` if no label applies.
    fn classify(&self, entity: &TrackedEntity) -> Option<Classification>;
}
