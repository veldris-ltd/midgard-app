//! Layer 8 of the assembly order: the input guardrail pass that screens untrusted
//! text **before** it reaches the model.
//!
//! Prompt 2.c defines the seam and a permissive default; the self-defending
//! classifier (prompt A.3) is wired here in Prompt 2.e. The pipeline calls
//! [`InputGuardrail::inspect`] once per item of the untrusted layers (each
//! grounding fact line, memory item and history turn, and the user message)
//! before composition, so a classifier sees exactly the text that could carry an
//! attack and a sanitise rewrite replaces only that item. The trusted layers are
//! never inspected or rewritten. The governed loop also uses the trait as its
//! output guardrail, over the model's whole contract output, actions included.
//! [VLD-ANB-MIDGARD-0077] [VLD-ANB-MIDGARD-0078] [FR-MI-7]

/// The guardrail's verdict on one inspected text (an untrusted item, or the
/// model's output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardVerdict {
    /// Let the text through unchanged.
    Allow,
    /// Let it through, but only **after replacing the inspected item with
    /// `rewritten`** — the offending fragment is stripped/neutralised before the
    /// model ever sees it. The pipeline substitutes the rewrite for that item
    /// alone; the governed loop treats a sanitise of model output as a block,
    /// because output cannot be rewritten. [FR-MI-7] [VLD-ANB-MIDGARD-0078]
    Sanitise {
        /// The rewritten item the model actually receives.
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

/// Screens untrusted text before the model sees it.
pub trait InputGuardrail: Send + Sync {
    /// Inspect one untrusted item (or, as the output guardrail, the model's
    /// contract output).
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
