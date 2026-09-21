//! # forge-ai-assist — NL-to-transform, preview-before-commit
//!
//! The Forge AI-assist adapter turns a plain-English request into a *previewable,
//! typed* pipeline transform by calling Mimir with the verbatim A.6 system prompt.
//! Its defining guarantee (AC-F-5 / NFR-AI-1): the output is **always a preview
//! diff** and the AI **never commits** — [`suggest_transform`] returns a
//! [`TransformPreview`] whose `committed` flag is structurally always `false`, and
//! whose stages only leave the adapter via an explicit human
//! [`TransformPreview::approved_stages`] call. [FR-F-7] [NFR-AI-1]
//!
//! ## Design
//! - [`prompt`] assembles A.6 (schema + transform library + request).
//! - [`suggester`] is the Mimir inference seam ([`TransformSuggester`]); production
//!   bridges to the model server over PQ-mTLS, tests use a deterministic
//!   [`MockSuggester`]. Because Forge depends on this trait, not on Mimir, the whole
//!   AI path is optional — absent it, Forge's non-AI features are untouched (AC-F-6).
//! - [`proposal`] parses the response ([`json`]) into typed [`forge_engine`] stages.
//! - [`adapter`] type-checks them into the preview.
//! - [`evals`] is the EV-A6-* suite.

pub mod adapter;
pub mod error;
pub mod evals;
pub mod json;
pub mod mimir_bridge;
pub mod prompt;
pub mod proposal;
pub mod suggester;

pub use adapter::{suggest_transform, TransformPreview, Validity};
pub use error::AssistError;
pub use evals::{ev_a6_nocolumn, ev_a6_preview, ev_a6_valid, EvalResult, EvalScore};
pub use mimir_bridge::MimirSuggester;
pub use prompt::{assemble, AssembledPrompt};
pub use proposal::{parse_proposal, RawProposal};
pub use suggester::{MockSuggester, TransformSuggester, UnavailableSuggester};
