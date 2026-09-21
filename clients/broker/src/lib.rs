//! # midgard-client-broker — the minimal-privilege extension broker
//!
//! The browser extension is the most exposed surface, so it is deliberately the
//! **least-capable** component (KDD-2). This crate is that broker: it holds **no
//! key material**, performs **no cryptography**, reads **only the page selection
//! the user explicitly shared**, and forwards every privileged operation to the
//! enclosure (or a companion app) as a *brokered request* — it never performs one
//! in-page. `[FR-C-8]` `[AC-C-8]`
//!
//! Least-privilege is **structural**, not merely documented:
//! - The crate has no crypto/keystore dependency, so it *cannot* hold a key.
//! - [`assist`] reads only [`PageContext::shared_selection`]; the other-tab and
//!   form-field data on [`PageContext`] exist so tests can prove they are **never
//!   read** (EV-AC3-noscrape), and are otherwise ignored.
//! - Any [`PrivilegedOp`] becomes a [`BrokeredRequest`]; nothing secret is ever put
//!   in the output (EV-AC3-nosecret).
//!
//! The assistant text is the verbatim A.C3 prompt in [`mimir_prompts`]; the actual
//! generation is delegated (brokered) to the enclosure via the [`assistant`] seam.

#![forbid(unsafe_code)]

pub mod assistant;
pub mod broker;
pub mod error;
pub mod evals;
pub mod prompt;

pub use assistant::{InPageAssistant, MockInPageAssistant};
pub use broker::{
    assist, BrokeredRequest, ExtensionOutput, PageContext, PrivilegedOp, Refusal, Task,
};
pub use error::BrokerError;
