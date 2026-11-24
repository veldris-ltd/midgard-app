//! UniFFI mirror of `MimirStudioExchange`, Mimir's Agent Studio exchange
//! (`midgard_client_core::consoles::studio`). Written by hand, as the core type is: the
//! TypeScript contract has no type for it. `[FR-MI-2]` `[FR-C-1]` `[DD-6]`

use midgard_client_core::consoles::studio as core;

/// One answer: its text and the live object references it cites.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MimirStudioAnswer {
    /// The answer.
    pub text: String,
    /// The citations.
    pub cites: Vec<String>,
}

/// The Agent Studio exchange.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MimirStudioExchange {
    /// The row id.
    pub id: String,
    /// The operator's opening question.
    pub prompt: String,
    /// The grounded answer to it.
    pub answer: MimirStudioAnswer,
    /// The state mutation the answer proposes, behind the human gate.
    pub proposal: String,
    /// The answer every later question gets.
    pub follow_up: MimirStudioAnswer,
}

impl From<core::MimirStudioAnswer> for MimirStudioAnswer {
    fn from(x: core::MimirStudioAnswer) -> Self {
        Self {
            text: x.text,
            cites: x.cites,
        }
    }
}

impl From<core::MimirStudioExchange> for MimirStudioExchange {
    fn from(x: core::MimirStudioExchange) -> Self {
        Self {
            id: x.id,
            prompt: x.prompt,
            answer: x.answer.into(),
            proposal: x.proposal,
            follow_up: x.follow_up.into(),
        }
    }
}
