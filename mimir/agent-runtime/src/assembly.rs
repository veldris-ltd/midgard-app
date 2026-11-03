//! The eight-layer prompt-assembly pipeline (BUILD_PLAYBOOK §0.1).
//!
//! No prompt reaches the model as authored. Assembly composes the final prompt
//! deterministically from the ordered layers so that **higher-authority context
//! cannot be overridden by lower-authority content** — the ordering is a security
//! control, not formatting. [FR-MI-7] [NFR-SEC-2]
//!
//! | # | Layer | Trimmable? |
//! |---|-------|-----------|
//! | 1 | System prompt (+ output contract) | never |
//! | 2 | Policy context | never |
//! | 3 | OAG grounding | **yes — by relevance rank** |
//! | 4 | Agent memory | yes — lowest relevance first |
//! | 5 | Tool schemas | never (capability integrity, INV-2) |
//! | 6 | Task history | yes — oldest first |
//! | 7 | User message | never (it is the request) |
//! | 8 | Input guardrail pass | (a screening step, not text) |
//!
//! Layer 8 screens each item of the untrusted layers (every grounding fact line,
//! memory item and history turn, and the user message) **before** it is composed
//! into its DATA block and before any token is counted. A block names the layer;
//! a sanitise verdict replaces only the offending item. The trusted layers
//! (system prompt, output contract, policy, tool schemas) are never passed to the
//! guardrail and never rewritten, so the control meant to protect the operator's
//! instructions cannot alter them. [VLD-ANB-MIDGARD-0078] [FR-MI-7]
//!
//! Budgeting rule: the non-trimmable core (system + policy + contract + tools +
//! user) must fit, or assembly is **rejected** rather than silently trimmed;
//! only the trimmable layers are truncated to fit, grounding first and by
//! relevance. [NFR-AI-1] [NFR-PERF-1]
//!
//! Untrusted layers are data, never instructions. Grounding, memory, history and
//! the user message are each rendered as a provenance-tagged DATA block
//! ([`mimir_prompts::delimit`]): every line indented, control and zero-width
//! characters removed, each item bounded, the per-request [`Boundary`] neutralised
//! inside the data. Headers are recognised only at column zero, so no tenant text
//! can forge a `=== SYSTEM ===` block, and the SYSTEM block carries
//! [`mimir_prompts::DATA_HANDLING_RULE`]. The SYSTEM block and the body are also
//! returned separately ([`AssembledPrompt::system_text`],
//! [`AssembledPrompt::body_text`]) so an adapter can send them with separate
//! roles. [VLD-ANB-MIDGARD-0080] [FR-MI-7]

use mimir_prompts::delimit;
use oag_retriever::Grounding;
use pqc_agility::{OsRng, Rng};

use crate::guardrail::{AllowAll, GuardVerdict, InputGuardrail};

// Layer headers. Each is a header only as a whole line at column zero; untrusted
// text is indented inside a DATA block, so a line that merely contains a header
// string is data. Downstream code that splits an assembled prompt by layer must
// match headers that way (`mimir_prompts::delimit::is_header_line`), never by
// substring. [VLD-ANB-MIDGARD-0080]

/// Layer 1 header (system prompt). Matched only as a whole column-zero line.
pub const H_SYSTEM: &str = "=== SYSTEM ===";
/// Layer 1 header (output contract). Matched only as a whole column-zero line.
pub const H_CONTRACT: &str = "=== OUTPUT_CONTRACT ===";
/// Layer 2 header. Matched only as a whole column-zero line.
pub const H_POLICY: &str = "=== POLICY ===";
/// Layer 3 header. Matched only as a whole column-zero line.
pub const H_GROUNDING: &str = "=== GROUNDING ===";
/// Layer 4 header. Matched only as a whole column-zero line.
pub const H_MEMORY: &str = "=== MEMORY ===";
/// Layer 5 header. Matched only as a whole column-zero line.
pub const H_TOOLS: &str = "=== TOOLS ===";
/// Layer 6 header. Matched only as a whole column-zero line.
pub const H_HISTORY: &str = "=== HISTORY ===";
/// Layer 7 header. Matched only as a whole column-zero line.
pub const H_USER: &str = "=== USER ===";

pub use mimir_prompts::delimit::Boundary;

/// A fresh random DATA boundary for one assembly: eight bytes from
/// `pqc_agility::OsRng`, as sixteen hex characters. [VLD-ANB-MIDGARD-0080]
pub fn random_boundary() -> Boundary {
    let mut bytes = [0u8; 8];
    OsRng.fill(&mut bytes);
    Boundary::from_random_bytes(bytes)
}

/// One DATA-delimited section: its layer header and its block's marker lines.
struct Section {
    header: &'static str,
    open: String,
    close: String,
}

impl Section {
    fn new(header: &'static str, label: &str, source: &str, boundary: &Boundary) -> Self {
        // Labels and sources are constants and a Boundary is always a valid
        // token, so the marker line cannot be refused.
        let open = delimit::open_line(label, source, boundary.as_str())
            .expect("constant label and source, and a Boundary is a token");
        Self {
            header,
            open,
            close: delimit::close_line(boundary.as_str()),
        }
    }

    /// The header and marker lines' token cost, paid once the first item is in.
    fn cost(&self, counter: &dyn TokenCounter) -> usize {
        counter.count(self.header) + counter.count(&self.open) + counter.count(&self.close)
    }

    /// The section's text around already-rendered items.
    fn render(&self, items: &[String]) -> String {
        let mut lines: Vec<&str> = Vec::with_capacity(items.len() + 3);
        lines.push(self.header);
        lines.push(&self.open);
        lines.extend(items.iter().map(String::as_str));
        lines.push(&self.close);
        lines.join("\n")
    }
}

/// Counts tokens in a string. The default is a deterministic word proxy; a real
/// per-family tokenizer swaps in behind this trait (Phase 3 adapters).
pub trait TokenCounter: Send + Sync {
    /// Token count of `s`.
    fn count(&self, s: &str) -> usize;
}

/// Whitespace-word token counter — deterministic and dependency-free.
#[derive(Debug, Default, Clone, Copy)]
pub struct WordCounter;

impl TokenCounter for WordCounter {
    fn count(&self, s: &str) -> usize {
        s.split_whitespace().count()
    }
}

/// The layer inputs for one assembly.
pub struct Layers<'a> {
    /// Layer 1: system prompt (A.1), highest authority.
    pub system: &'a str,
    /// Layer 1: the output contract, part of the system-authority block.
    pub output_contract: &'a str,
    /// Layer 2: the pre-rendered policy context.
    pub policy: String,
    /// Layer 3: OAG grounding (trimmed by relevance rank if needed).
    pub grounding: &'a Grounding,
    /// Layer 4: agent memory items (trimmable).
    pub memory: Vec<String>,
    /// Layer 5: tool schemas (never trimmed).
    pub tools: Vec<String>,
    /// Layer 6: task history, oldest-first as supplied (trimmed oldest-first).
    pub history: Vec<String>,
    /// Layer 7: the user/task message (never trimmed).
    pub user_message: &'a str,
}

/// Assembly configuration.
#[derive(Debug, Clone, Copy)]
pub struct AssemblyConfig {
    /// The total token budget for the assembled prompt.
    pub budget_tokens: usize,
}

/// The assembled prompt and its accounting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledPrompt {
    /// The final, authority-ordered prompt text (layers 1–7):
    /// `system_text + "\n\n" + body_text`.
    pub text: String,
    /// The SYSTEM block (system prompt, data-handling rule, output contract) —
    /// the only text with system authority. Always exactly what the trusted inputs
    /// render: layer 8 never inspects or rewrites it. [VLD-ANB-MIDGARD-0078]
    pub system_text: String,
    /// Everything after the SYSTEM block: policy, tools and the DATA-delimited
    /// grounding, memory, history and user layers.
    pub body_text: String,
    /// The boundary this assembly's DATA blocks carry.
    pub boundary: Boundary,
    /// Citations of the grounding facts that were included (the evidence
    /// actually presented to the model).
    pub included_citations: Vec<String>,
    /// The highest classification level (`marking_level`) among the included
    /// grounding facts, 0 when none was included: the level of the fabric data in
    /// the prompt, which the inference request's marking carries.
    /// [VLD-ANB-MIDGARD-0034]
    pub max_marking_level: u8,
    /// Grounding facts dropped to fit the budget.
    pub dropped_facts: usize,
    /// Memory items dropped to fit the budget.
    pub dropped_memory: usize,
    /// History turns dropped to fit the budget.
    pub dropped_history: usize,
    /// Total token count of the assembled text.
    pub token_count: usize,
}

/// Why assembly could not produce a prompt (both fail closed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssemblyError {
    /// The non-trimmable core exceeds the budget — reject, never silently trim.
    CoreExceedsBudget {
        /// Tokens the core requires.
        core_tokens: usize,
        /// The budget.
        budget: usize,
    },
    /// The layer-8 guardrail blocked an item of an untrusted layer.
    Blocked {
        /// Which layer, and why.
        reason: String,
    },
}

impl std::fmt::Display for AssemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssemblyError::CoreExceedsBudget {
                core_tokens,
                budget,
            } => write!(
                f,
                "system+policy+contract+tools+user require {core_tokens} tokens > budget {budget}"
            ),
            AssemblyError::Blocked { reason } => write!(f, "input guardrail blocked: {reason}"),
        }
    }
}

impl std::error::Error for AssemblyError {}

/// The assembly pipeline (with an injected token counter and layer-8 guardrail).
pub struct Pipeline {
    counter: Box<dyn TokenCounter>,
    guardrail: Box<dyn InputGuardrail>,
    /// A fixed DATA boundary; `None` draws a random one per assembly.
    boundary: Option<Boundary>,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline {
    /// A pipeline with the default word counter and permissive guardrail.
    pub fn new() -> Self {
        Self {
            counter: Box::new(WordCounter),
            guardrail: Box::new(AllowAll),
            boundary: None,
        }
    }

    /// Use a specific token counter.
    pub fn with_counter(mut self, counter: Box<dyn TokenCounter>) -> Self {
        self.counter = counter;
        self
    }

    /// Wire the layer-8 input guardrail (the A.3 classifier in Prompt 2.e). It is
    /// called once per untrusted item, never on a trusted layer.
    pub fn with_guardrail(mut self, guardrail: Box<dyn InputGuardrail>) -> Self {
        self.guardrail = guardrail;
        self
    }

    /// Use a fixed DATA boundary (tests asserting exact layout). Without one, every
    /// [`Pipeline::assemble`] call draws a fresh [`random_boundary`].
    pub fn with_boundary(mut self, boundary: Boundary) -> Self {
        self.boundary = Some(boundary);
        self
    }

    fn count(&self, s: &str) -> usize {
        self.counter.count(s)
    }

    /// Layer 8: screen each item of one untrusted layer before it is composed. A
    /// block fails the assembly naming the layer; a sanitise verdict replaces only
    /// that item with its rewrite; an allow keeps it. Trusted layers never come
    /// here. [VLD-ANB-MIDGARD-0078] [FR-MI-7]
    fn screen(&self, label: &str, items: &[String]) -> Result<Vec<String>, AssemblyError> {
        items
            .iter()
            .map(|item| match self.guardrail.inspect(item) {
                GuardVerdict::Allow => Ok(item.clone()),
                GuardVerdict::Sanitise { rewritten, .. } => Ok(rewritten),
                GuardVerdict::Block { reason } => Err(AssemblyError::Blocked {
                    reason: format!("{label}: {reason}"),
                }),
            })
            .collect()
    }

    /// Fill a trimmable section greedily under the remaining budget, returning how
    /// many of the already-rendered `items` were included and the updated running
    /// total. `section_cost` (the header and DATA marker lines) is paid with the
    /// first item, so the accounting matches the rendered block exactly. Items are
    /// consumed in the order given (callers pre-sort by priority).
    fn fill(
        &self,
        section_cost: usize,
        items: &[String],
        budget: usize,
        mut running: usize,
    ) -> (usize, usize) {
        let mut included = 0;
        for item in items {
            let add = if included == 0 {
                section_cost + self.count(item)
            } else {
                self.count(item)
            };
            if running + add <= budget {
                running += add;
                included += 1;
            } else {
                break;
            }
        }
        (included, running)
    }

    /// Assemble the eight layers. Screens every untrusted item first (layer 8);
    /// rejects if the non-trimmable core cannot fit; trims grounding (by
    /// relevance), then memory, then history to fit.
    pub fn assemble(
        &self,
        layers: &Layers,
        cfg: &AssemblyConfig,
    ) -> Result<AssembledPrompt, AssemblyError> {
        let budget = cfg.budget_tokens;
        let boundary = self.boundary.clone().unwrap_or_else(random_boundary);
        let b = boundary.as_str();

        // Layer 8, run first: each untrusted item is screened on its own, before it
        // is rendered into a DATA block and before any token is counted, so the
        // accounting below sees exactly the text that is composed. The trusted
        // layers are never screened. [VLD-ANB-MIDGARD-0078]
        let user_message = self
            .screen("user", &[layers.user_message.to_string()])?
            .remove(0);
        let fact_lines: Vec<String> = layers.grounding.facts.iter().map(|f| f.line()).collect();
        let fact_lines = self.screen("grounding", &fact_lines)?;
        let memory = self.screen("memory", &layers.memory)?;
        let history = self.screen("history", &layers.history)?;

        // Non-trimmable core. The SYSTEM block is trusted text only; the
        // data-handling rule tells the model what the DATA blocks below are.
        let sys_block = format!(
            "{H_SYSTEM}\n{}\n{}\n{H_CONTRACT}\n{}",
            layers.system,
            mimir_prompts::delimit::DATA_HANDLING_RULE,
            layers.output_contract
        );
        let policy_block = format!("{H_POLICY}\n{}", layers.policy);
        let tools_block = if layers.tools.is_empty() {
            String::new()
        } else {
            format!("{H_TOOLS}\n{}", layers.tools.join("\n"))
        };
        let user_block = Section::new(H_USER, "USER", "user", &boundary)
            .render(&[delimit::render_item(b, &user_message)]);

        let core = self.count(&sys_block)
            + self.count(&policy_block)
            + self.count(&tools_block)
            + self.count(&user_block);
        if core > budget {
            return Err(AssemblyError::CoreExceedsBudget {
                core_tokens: core,
                budget,
            });
        }

        let mut running = core;

        // Layer 3: grounding, highest relevance first (facts are pre-sorted). Each
        // fact line is already one line with its fields cleaned (ground.rs).
        let fact_lines: Vec<String> = fact_lines
            .iter()
            .map(|line| delimit::render_item(b, line))
            .collect();
        let g_section = Section::new(H_GROUNDING, "GROUNDING", "fabric", &boundary);
        let (g_incl, running_after_g) = self.fill(
            g_section.cost(self.counter.as_ref()),
            &fact_lines,
            budget,
            running,
        );
        running = running_after_g;
        let grounding_block = if g_incl == 0 {
            String::new()
        } else {
            g_section.render(&fact_lines[..g_incl])
        };
        let included_citations: Vec<String> = layers
            .grounding
            .facts
            .iter()
            .take(g_incl)
            .map(|f| f.citation.clone())
            .collect();
        let max_marking_level = layers
            .grounding
            .facts
            .iter()
            .take(g_incl)
            .map(|f| f.marking_level)
            .max()
            .unwrap_or(0);
        let dropped_facts = fact_lines.len() - g_incl;

        // Layer 4: memory (lowest relevance is dropped — caller supplies best first).
        let memory_items: Vec<String> = memory.iter().map(|m| delimit::render_item(b, m)).collect();
        let m_section = Section::new(H_MEMORY, "MEMORY", "agent-memory", &boundary);
        let (m_incl, running_after_m) = self.fill(
            m_section.cost(self.counter.as_ref()),
            &memory_items,
            budget,
            running,
        );
        running = running_after_m;
        let dropped_memory = memory.len() - m_incl;
        let memory_block = if m_incl == 0 {
            String::new()
        } else {
            m_section.render(&memory_items[..m_incl])
        };

        // Layer 6: history, most-recent-first for inclusion (oldest dropped).
        let recent: Vec<String> = history
            .iter()
            .rev()
            .map(|h| delimit::render_item(b, h))
            .collect();
        let h_section = Section::new(H_HISTORY, "HISTORY", "task-history", &boundary);
        let (h_incl, running_after_h) = self.fill(
            h_section.cost(self.counter.as_ref()),
            &recent,
            budget,
            running,
        );
        running = running_after_h;
        let dropped_history = history.len() - h_incl;
        // Restore chronological order within the included window for the prompt.
        let history_block = if h_incl == 0 {
            String::new()
        } else {
            let mut window = recent[..h_incl].to_vec();
            window.reverse();
            h_section.render(&window)
        };

        // Canonical layer order 1→7: the SYSTEM block, then the body.
        let mut parts = vec![policy_block];
        if !grounding_block.is_empty() {
            parts.push(grounding_block);
        }
        if !memory_block.is_empty() {
            parts.push(memory_block);
        }
        if !tools_block.is_empty() {
            parts.push(tools_block);
        }
        if !history_block.is_empty() {
            parts.push(history_block);
        }
        parts.push(user_block);
        let body_text = parts.join("\n\n");
        // Nothing rewrites the composed text, so the SYSTEM block is always its
        // exact prefix and the role split never degrades. [VLD-ANB-MIDGARD-0078]
        let text = format!("{sys_block}\n\n{body_text}");

        let token_count = self.count(&text);
        debug_assert_eq!(token_count, running, "token accounting must be exact");

        Ok(AssembledPrompt {
            text,
            system_text: sys_block,
            body_text,
            boundary,
            included_citations,
            max_marking_level,
            dropped_facts,
            dropped_memory,
            dropped_history,
            token_count,
        })
    }
}
