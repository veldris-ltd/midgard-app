//! Layer 8 of the assembly order: the input guardrail pass that screens the
//! fully-assembled prompt **before** it reaches the model.
//!
//! Prompt 2.c defines the seam and a permissive default; the self-defending
//! classifier (prompt A.3) is wired here in Prompt 2.e. Keeping it as the last
//! layer is the point — it inspects the final, authority-ordered text, so a
//! lower-authority layer cannot smuggle an instruction past it. [FR-MI-7]

/// The guardrail's verdict on an assembled prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardVerdict {
    /// Let the prompt through unchanged.
    Allow,
    /// Let it through, but only **after replacing the assembled text with
    /// `rewritten`** — the offending fragment is stripped/neutralised before the
    /// model ever sees it. The pipeline substitutes the rewritten text. [FR-MI-7]
    Sanitise {
        /// The rewritten prompt the model actually receives.
        rewritten: String,
        /// Why it was sanitised (never echoes the payload).
        reason: String,
    },
    /// Block it; the loop halts and logs (fail closed).
    Block {
        /// Why it was blocked.
        reason: String,
    },
}

/// Screens an assembled prompt before the model sees it.
pub trait InputGuardrail: Send + Sync {
    /// Inspect the final assembled text.
    fn inspect(&self, assembled: &str) -> GuardVerdict;
}

/// The permissive default (Prompt 2.c). Replaced by the A.3 classifier in 2.e.
#[derive(Debug, Default, Clone, Copy)]
pub struct AllowAll;

impl InputGuardrail for AllowAll {
    fn inspect(&self, _assembled: &str) -> GuardVerdict {
        GuardVerdict::Allow
    }
}
