//! Per-family adapters — the family-specific half of the Model Abstraction Layer
//! (SAD_02b §5.1). [FR-MS-7]
//!
//! Each supported family (Mistral, Nemotron, Gemma, Granite) speaks a different chat template
//! and set of stop tokens. Those quirks live **here, in the abstraction layer —
//! never in the system prompts** (`mimir-prompts` stays family-neutral). A
//! consumer hands the plane one already-assembled, family-neutral prompt; the
//! adapter for the routed family wraps it in that family's turn delimiters and
//! generation prefix on the way in, and trims the family's stop tokens on the way
//! out. This is what lets one unchanged inference contract serve three
//! heterogeneous families. [DD-6]
//!
//! The templates here are structural (turn delimiters + generation prefix) and
//! carry **no instruction text** — the semantic system prompt is already inside
//! the neutral prompt. Swapping the real engine's exact tokeniser template in
//! behind [`FamilyAdapter`] changes nothing above this seam.

/// The family-specific templating seam. One implementation per supported family.
pub trait FamilyAdapter: Send + Sync {
    /// The family this adapter serves (e.g. "Mistral").
    fn family(&self) -> &'static str;

    /// Wrap a family-neutral prompt in this family's chat template, returning the
    /// exact string handed to the engine. Structural only — no instruction text.
    fn render(&self, neutral_prompt: &str) -> String;

    /// This family's stop tokens (generation halts at, and output is trimmed of,
    /// the earliest of these).
    fn stop_tokens(&self) -> &[&'static str];

    /// Trim a raw completion at the earliest stop token and strip surrounding
    /// whitespace. Default impl is uniform across families.
    fn postprocess(&self, raw: &str) -> String {
        let mut end = raw.len();
        for tok in self.stop_tokens() {
            if let Some(i) = raw.find(tok) {
                end = end.min(i);
            }
        }
        raw[..end].trim().to_string()
    }
}

/// Mistral chat template (`<|system|>/<|user|>/<|assistant|>` turns). Reasoning-strong
/// default. [FR-MS-7]
#[derive(Debug, Default, Clone, Copy)]
pub struct MistralAdapter;

const MISTRAL_STOPS: &[&str] = &["<|user|>", "<|observation|>", "<|endoftext|>"];

impl FamilyAdapter for MistralAdapter {
    fn family(&self) -> &'static str {
        "Mistral"
    }
    fn render(&self, neutral_prompt: &str) -> String {
        format!("[gMASK]<sop><|user|>\n{neutral_prompt}<|assistant|>\n")
    }
    fn stop_tokens(&self) -> &[&'static str] {
        MISTRAL_STOPS
    }
}

/// Nemotron chat template (`<extra_id_*>` turn markers, NVIDIA format). Strong
/// code/agentic tasks. [FR-MS-7]
#[derive(Debug, Default, Clone, Copy)]
pub struct NemotronAdapter;

const NEMOTRON_STOPS: &[&str] = &["<extra_id_1>", "</s>"];

impl FamilyAdapter for NemotronAdapter {
    fn family(&self) -> &'static str {
        "Nemotron"
    }
    fn render(&self, neutral_prompt: &str) -> String {
        format!("<extra_id_1>User\n{neutral_prompt}\n<extra_id_1>Assistant\n")
    }
    fn stop_tokens(&self) -> &[&'static str] {
        NEMOTRON_STOPS
    }
}

/// Gemma chat template (`<start_of_turn>/<end_of_turn>`, GGUF/quantised class).
/// Efficient small-footprint option. [FR-MS-7]
#[derive(Debug, Default, Clone, Copy)]
pub struct GemmaAdapter;

const GEMMA_STOPS: &[&str] = &["<end_of_turn>", "<eos>"];

impl FamilyAdapter for GemmaAdapter {
    fn family(&self) -> &'static str {
        "Gemma"
    }
    fn render(&self, neutral_prompt: &str) -> String {
        format!("<start_of_turn>user\n{neutral_prompt}<end_of_turn>\n<start_of_turn>model\n")
    }
    fn stop_tokens(&self) -> &[&'static str] {
        GEMMA_STOPS
    }
}

/// Granite chat template (`<|start_of_role|>…<|end_of_role|>` turns), IBM's Apache-2.0
/// family: the small default model a fresh enclosure answers with on a CPU. [FR-MS-7]
#[derive(Debug, Default, Clone, Copy)]
pub struct GraniteAdapter;

const GRANITE_STOPS: &[&str] = &["<|end_of_text|>", "<|start_of_role|>"];

impl FamilyAdapter for GraniteAdapter {
    fn family(&self) -> &'static str {
        "Granite"
    }
    fn render(&self, neutral_prompt: &str) -> String {
        format!(
            "<|start_of_role|>user<|end_of_role|>{neutral_prompt}<|end_of_text|>\n\
             <|start_of_role|>assistant<|end_of_role|>"
        )
    }
    fn stop_tokens(&self) -> &[&'static str] {
        GRANITE_STOPS
    }
}

/// The set of family adapters the plane knows. Routing looks one up by family
/// name; a family with no adapter cannot be served (fail closed).
pub struct AdapterSet {
    adapters: Vec<Box<dyn FamilyAdapter>>,
}

impl Default for AdapterSet {
    fn default() -> Self {
        Self::default_three()
    }
}

impl AdapterSet {
    /// The three supported families: Mistral, Nemotron, Gemma, Granite.
    pub fn default_three() -> Self {
        Self {
            adapters: vec![
                Box::new(MistralAdapter),
                Box::new(NemotronAdapter),
                Box::new(GemmaAdapter),
            ],
        }
    }

    /// Build from an explicit adapter list (e.g. to add a family).
    pub fn from_adapters(adapters: Vec<Box<dyn FamilyAdapter>>) -> Self {
        Self { adapters }
    }

    /// The adapter for `family`, if one is registered.
    pub fn get(&self, family: &str) -> Option<&dyn FamilyAdapter> {
        self.adapters
            .iter()
            .find(|a| a.family() == family)
            .map(|b| b.as_ref())
    }

    /// The families this set can serve.
    pub fn families(&self) -> Vec<&'static str> {
        self.adapters.iter().map(|a| a.family()).collect()
    }
}
