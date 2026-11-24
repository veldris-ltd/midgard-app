//! `MimirStudioExchange`: Mimir's Agent Studio exchange (`mimir.studioExchange`), the
//! canned, grounded answer with its object citations and its human-gated proposal, and
//! the answer every later question gets. Written by hand: the web console writes the
//! exchange into `AgentStudioScreen.tsx` itself, not into `MimirMock`, so the
//! TypeScript contract has no type for it; the row is copied once into
//! `clients/demo/studio.json` and served by `clients/demo/tools/extract-sample.mjs`.
//! `[FR-MI-2]` `[NFR-AI-1]` `[FR-C-1]`

use attest::json::Json;

use crate::consoles::decode::{field, object, DecodeError, FromJson};

/// One answer: its text and the live object references it cites.
#[derive(Debug, Clone, PartialEq)]
pub struct MimirStudioAnswer {
    /// The answer.
    pub text: String,
    /// The citations, e.g. `Aircraft/AC-2214`.
    pub cites: Vec<String>,
}

/// The Agent Studio exchange.
#[derive(Debug, Clone, PartialEq)]
pub struct MimirStudioExchange {
    /// The row id (the agent the exchange is with, `dispatch`).
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

impl FromJson for MimirStudioAnswer {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        let o = object(v, path, &["text", "cites"])?;
        Ok(Self {
            text: field(o, "text", path)?,
            cites: field(o, "cites", path)?,
        })
    }
}

impl FromJson for MimirStudioExchange {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        let o = object(v, path, &["id", "prompt", "answer", "proposal", "followUp"])?;
        Ok(Self {
            id: field(o, "id", path)?,
            prompt: field(o, "prompt", path)?,
            answer: field(o, "answer", path)?,
            proposal: field(o, "proposal", path)?,
            follow_up: field(o, "followUp", path)?,
        })
    }
}
