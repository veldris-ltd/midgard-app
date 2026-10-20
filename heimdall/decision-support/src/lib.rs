//! # heimdall-decision-support — advisory, cited option generation (A.8)
//!
//! The decision-support adapter asks Mimir (via the verbatim A.8 prompt) for a
//! small set of **ranked courses of action**, each **citing the specific
//! feed/entity evidence** it rests on, and grounds those citations against the live
//! operating picture. It is **strictly advisory**: the returned [`AdvisoryOptions`]
//! has no code path that acts — a human authorises any consequential action through
//! the audited `heimdall-workflow` gate. [FR-H-4] [NFR-AI-1] [AC-H-3]
//!
//! ## Seam + deterministic mock
//! Inference is a trait, [`OptionProvider`] — production bridges to the Mimir model
//! server over PQ-mTLS, tests/Evals use a deterministic [`MockOptionProvider`]. The
//! same seam-with-mock pattern Forge (A.6) and Bifrost (A.7) use, so the whole
//! path is optional: with Mimir absent the rest of Heimdall (fusion, rules
//! classification, picture-sync) is untouched. [DD-6]
//!
//! ## Grounding
//! Every option's citations are resolved against the picture: `entity:`/`track:`
//! tokens must name a tracked entity, `feed:` tokens must name a feed that actually
//! contributed to it. An option that cites nothing, or cites invented evidence, is
//! reported ungrounded — which is exactly what the EV-A8 evals assert. [FR-H-4]

pub mod adapter;
pub mod ask;
pub mod error;
pub mod evals;
pub mod json;
pub mod mimir_bridge;
pub mod options;
pub mod prompt;
pub mod provider;

pub use adapter::{request_options, AdvisoryOptions, OptionGrounding};
pub use ask::{answer_question, Answer, AREA_SUMMARY_ID};
pub use error::DecisionError;
pub use evals::{
    ev_a8_advisory, ev_a8_cited, ev_a8_grounded, ev_a8_insuff, ev_a8_neutral, ev_a8_ranked,
    EvalResult, EvalScore,
};
pub use mimir_bridge::MimirOptionProvider;
pub use options::{Citation, CitationKind, CourseOfAction, OptionSet};
pub use prompt::{assemble, render_picture, AssembledPrompt};
pub use provider::{MockOptionProvider, OptionProvider, UnavailableOptionProvider};
