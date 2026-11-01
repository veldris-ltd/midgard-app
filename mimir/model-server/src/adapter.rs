//! Per-family adapters — the family-specific half of the Model Abstraction Layer
//! (SAD_02b §5.1). [FR-MS-7]
//!
//! Each supported family (Mistral, Nemotron, Gemma, Granite) speaks a different chat template
//! and set of stop tokens. Those quirks live **here, in the abstraction layer —
//! never in the system prompts** (`mimir-prompts` stays family-neutral). A
//! consumer hands the plane an already-assembled, family-neutral prompt in two
//! parts: the system text (the only text with system authority) and the body
//! (policy and the DATA-delimited untrusted layers). The adapter for the routed
//! family places each in that family's own turn — the system role where the
//! family has one — adds the generation prefix on the way in, and trims the
//! family's stop tokens on the way out. This is what lets one unchanged inference
//! contract serve four heterogeneous families. [DD-6]
//!
//! The templates here are structural (turn delimiters + generation prefix) and
//! carry **no instruction text** — the semantic system prompt arrives as the
//! system part. Every adapter declares its family's control vocabulary
//! ([`FamilyAdapter::special_tokens`]) and [`FamilyAdapter::render`] neutralises
//! it in both parts before templating, so the only control tokens in the string
//! handed to the engine are the template's own: text cannot close its turn and
//! open a system or assistant turn, even on an engine (llama-server) that parses
//! special tokens in a raw prompt. [VLD-ANB-MIDGARD-0060] Swapping the real
//! engine's exact tokeniser template in behind [`FamilyAdapter`] changes nothing
//! above this seam.

/// The family-specific templating seam. One implementation per supported family.
pub trait FamilyAdapter: Send + Sync {
    /// The family this adapter serves (e.g. "Mistral").
    fn family(&self) -> &'static str;

    /// This family's complete control vocabulary: role and turn markers, BOS and
    /// EOS. Every occurrence in text handed to [`FamilyAdapter::render`] is
    /// neutralised. [VLD-ANB-MIDGARD-0060]
    fn special_tokens(&self) -> &[&'static str];

    /// Rewrite every occurrence of one of this family's special tokens in `text`
    /// so no tokeniser reads it as a control token: a backslash goes before the
    /// token's final character (`[/INST]` becomes `[/INST\]`), which the model
    /// still reads as the literal. Longer tokens are matched first, so a token
    /// that contains another is rewritten once. [VLD-ANB-MIDGARD-0060]
    fn neutralise(&self, text: &str) -> String {
        neutralise_tokens(text, self.special_tokens())
    }

    /// Place the system text and the user text in this family's chat template,
    /// returning the exact string handed to the engine. Both are neutralised
    /// first. An empty (or whitespace-only) system text renders the single-turn
    /// shape with the user text alone. Structural only — no instruction text.
    fn render(&self, system: &str, user: &str) -> String;

    /// This family's stop tokens (generation halts at, and output is trimmed of,
    /// the earliest of these).
    fn stop_tokens(&self) -> &[&'static str];

    /// Trim a raw completion at the earliest stop token and strip surrounding
    /// whitespace. Default impl is uniform across families. With the prompt
    /// neutralised, the only stop tokens in a completion are the engine's own.
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

/// Rewrite every occurrence of a token from `tokens` in `text` with a backslash
/// before the token's final character, matching the longest token first at each
/// position in one left-to-right pass. The rewritten form holds a backslash, which
/// no token in a family vocabulary contains, so the pass never creates a token it
/// did not remove. [VLD-ANB-MIDGARD-0060]
pub fn neutralise_tokens(text: &str, tokens: &[&str]) -> String {
    let mut sorted: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|t| !t.is_empty() && text.contains(*t))
        .collect();
    if sorted.is_empty() {
        return text.to_string();
    }
    sorted.sort_by_key(|t| std::cmp::Reverse(t.len()));

    let mut out = String::with_capacity(text.len() + 8);
    let mut rest = text;
    'scan: while let Some(ch) = rest.chars().next() {
        for tok in &sorted {
            if rest.starts_with(tok) {
                let last = tok.chars().next_back().map_or(0, char::len_utf8);
                let (head, tail) = tok.split_at(tok.len() - last);
                out.push_str(head);
                out.push('\\');
                out.push_str(tail);
                rest = &rest[tok.len()..];
                continue 'scan;
            }
        }
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

/// Whether a system text is absent, so the family's single-turn shape is rendered.
fn no_system(system: &str) -> bool {
    system.trim().is_empty()
}

/// For a family with no system role: the system text first in the single user
/// turn, a blank line, then the user text (the family's documented convention);
/// the user text alone when there is no system text. Both already neutralised.
fn system_then_user(system: &str, user: &str) -> String {
    if no_system(system) {
        user.to_string()
    } else {
        format!("{system}\n\n{user}")
    }
}

/// Mistral chat template (`[INST] … [/INST]` turns), Mistral AI's Apache-2.0 family.
/// Reasoning-strong default. [FR-MS-7]
///
/// The family has no system role, so the system text leads the single user turn,
/// separated from the user text by a blank line. The template is structural, and
/// neutralisation guarantees the only control tokens are the template's own.
/// [VLD-ANB-MIDGARD-0060]
#[derive(Debug, Default, Clone, Copy)]
pub struct MistralAdapter;

const MISTRAL_STOPS: &[&str] = &["</s>", "[INST]"];
const MISTRAL_SPECIAL: &[&str] = &[
    "<s>",
    "</s>",
    "[INST]",
    "[/INST]",
    "[SYSTEM_PROMPT]",
    "[/SYSTEM_PROMPT]",
];

impl FamilyAdapter for MistralAdapter {
    fn family(&self) -> &'static str {
        "Mistral"
    }
    fn special_tokens(&self) -> &[&'static str] {
        MISTRAL_SPECIAL
    }
    fn render(&self, system: &str, user: &str) -> String {
        let content = system_then_user(&self.neutralise(system), &self.neutralise(user));
        format!("<s>[INST] {content} [/INST]")
    }
    fn stop_tokens(&self) -> &[&'static str] {
        MISTRAL_STOPS
    }
}

/// Nemotron chat template (`<extra_id_*>` turn markers, NVIDIA format). Strong
/// code/agentic tasks. [FR-MS-7]
///
/// The system text sits in the family's own `<extra_id_0>System` turn, ahead of
/// the user turn; the turn is omitted when there is no system text. The template
/// is structural, and neutralisation guarantees the only control tokens are the
/// template's own. [VLD-ANB-MIDGARD-0060]
#[derive(Debug, Default, Clone, Copy)]
pub struct NemotronAdapter;

const NEMOTRON_STOPS: &[&str] = &["<extra_id_1>", "</s>"];
const NEMOTRON_SPECIAL: &[&str] = &["<extra_id_0>", "<extra_id_1>", "</s>"];

impl FamilyAdapter for NemotronAdapter {
    fn family(&self) -> &'static str {
        "Nemotron"
    }
    fn special_tokens(&self) -> &[&'static str] {
        NEMOTRON_SPECIAL
    }
    fn render(&self, system: &str, user: &str) -> String {
        let user = self.neutralise(user);
        if no_system(system) {
            format!("<extra_id_1>User\n{user}\n<extra_id_1>Assistant\n")
        } else {
            let system = self.neutralise(system);
            format!(
                "<extra_id_0>System\n{system}\n<extra_id_1>User\n{user}\n<extra_id_1>Assistant\n"
            )
        }
    }
    fn stop_tokens(&self) -> &[&'static str] {
        NEMOTRON_STOPS
    }
}

/// Gemma chat template (`<start_of_turn>/<end_of_turn>`, GGUF/quantised class).
/// Efficient small-footprint option. [FR-MS-7]
///
/// The family has no system role, so the system text leads the single user turn,
/// separated from the user text by a blank line. The template is structural, and
/// neutralisation guarantees the only control tokens are the template's own.
/// [VLD-ANB-MIDGARD-0060]
#[derive(Debug, Default, Clone, Copy)]
pub struct GemmaAdapter;

const GEMMA_STOPS: &[&str] = &["<end_of_turn>", "<eos>"];
const GEMMA_SPECIAL: &[&str] = &["<bos>", "<eos>", "<start_of_turn>", "<end_of_turn>"];

impl FamilyAdapter for GemmaAdapter {
    fn family(&self) -> &'static str {
        "Gemma"
    }
    fn special_tokens(&self) -> &[&'static str] {
        GEMMA_SPECIAL
    }
    fn render(&self, system: &str, user: &str) -> String {
        let content = system_then_user(&self.neutralise(system), &self.neutralise(user));
        format!("<start_of_turn>user\n{content}<end_of_turn>\n<start_of_turn>model\n")
    }
    fn stop_tokens(&self) -> &[&'static str] {
        GEMMA_STOPS
    }
}

/// Granite chat template (`<|start_of_role|>…<|end_of_role|>` turns), IBM's Apache-2.0
/// family: the small default model a fresh enclosure answers with on a CPU. [FR-MS-7]
///
/// The system text sits in the family's own `system` role, ahead of the user turn;
/// the turn is omitted when there is no system text. The template is structural,
/// and neutralisation guarantees the only control tokens are the template's own.
/// [VLD-ANB-MIDGARD-0060]
#[derive(Debug, Default, Clone, Copy)]
pub struct GraniteAdapter;

const GRANITE_STOPS: &[&str] = &["<|end_of_text|>", "<|start_of_role|>"];
const GRANITE_SPECIAL: &[&str] = &["<|start_of_role|>", "<|end_of_role|>", "<|end_of_text|>"];

impl FamilyAdapter for GraniteAdapter {
    fn family(&self) -> &'static str {
        "Granite"
    }
    fn special_tokens(&self) -> &[&'static str] {
        GRANITE_SPECIAL
    }
    fn render(&self, system: &str, user: &str) -> String {
        let user = self.neutralise(user);
        let user_turn = format!(
            "<|start_of_role|>user<|end_of_role|>{user}<|end_of_text|>\n\
             <|start_of_role|>assistant<|end_of_role|>"
        );
        if no_system(system) {
            user_turn
        } else {
            let system = self.neutralise(system);
            format!("<|start_of_role|>system<|end_of_role|>{system}<|end_of_text|>\n{user_turn}")
        }
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
        Self::default_families()
    }
}

impl AdapterSet {
    /// The four supported families: Mistral, Nemotron, Gemma, Granite.
    pub fn default_families() -> Self {
        Self {
            adapters: vec![
                Box::new(MistralAdapter),
                Box::new(NemotronAdapter),
                Box::new(GemmaAdapter),
                Box::new(GraniteAdapter),
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
