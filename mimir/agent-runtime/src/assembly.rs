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
//! Budgeting rule: the non-trimmable core (system + policy + contract + tools +
//! user) must fit, or assembly is **rejected** rather than silently trimmed;
//! only the trimmable layers are truncated to fit, grounding first and by
//! relevance. [NFR-AI-1] [NFR-PERF-1]

use oag_retriever::Grounding;

use crate::guardrail::{AllowAll, GuardVerdict, InputGuardrail};

const H_SYSTEM: &str = "=== SYSTEM ===";
const H_CONTRACT: &str = "=== OUTPUT_CONTRACT ===";
const H_POLICY: &str = "=== POLICY ===";
const H_GROUNDING: &str = "=== GROUNDING ===";
const H_MEMORY: &str = "=== MEMORY ===";
const H_TOOLS: &str = "=== TOOLS ===";
const H_HISTORY: &str = "=== HISTORY ===";
const H_USER: &str = "=== USER ===";

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
    /// The final, authority-ordered prompt text (layers 1–7).
    pub text: String,
    /// Citations of the grounding facts that were included (the evidence
    /// actually presented to the model).
    pub included_citations: Vec<String>,
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
    /// The layer-8 guardrail blocked the assembled prompt.
    Blocked {
        /// Why.
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
        }
    }

    /// Use a specific token counter.
    pub fn with_counter(mut self, counter: Box<dyn TokenCounter>) -> Self {
        self.counter = counter;
        self
    }

    /// Wire the layer-8 input guardrail (the A.3 classifier in Prompt 2.e).
    pub fn with_guardrail(mut self, guardrail: Box<dyn InputGuardrail>) -> Self {
        self.guardrail = guardrail;
        self
    }

    fn count(&self, s: &str) -> usize {
        self.counter.count(s)
    }

    /// Fill a trimmable section greedily under the remaining budget, returning the
    /// rendered block, how many items were included, and the updated running
    /// total. Items are consumed in the order given (callers pre-sort by priority).
    fn fill(
        &self,
        header: &str,
        items: &[String],
        budget: usize,
        mut running: usize,
    ) -> (String, usize, usize) {
        let header_tokens = self.count(header);
        let mut included: Vec<&str> = Vec::new();
        for item in items {
            let add = if included.is_empty() {
                header_tokens + self.count(item)
            } else {
                self.count(item)
            };
            if running + add <= budget {
                running += add;
                included.push(item);
            } else {
                break;
            }
        }
        let block = if included.is_empty() {
            String::new()
        } else {
            format!("{header}\n{}", included.join("\n"))
        };
        (block, included.len(), running)
    }

    /// Assemble the eight layers. Rejects if the non-trimmable core cannot fit;
    /// trims grounding (by relevance), then memory, then history to fit; then runs
    /// the layer-8 guardrail.
    pub fn assemble(
        &self,
        layers: &Layers,
        cfg: &AssemblyConfig,
    ) -> Result<AssembledPrompt, AssemblyError> {
        let budget = cfg.budget_tokens;

        // Non-trimmable core.
        let sys_block = format!(
            "{H_SYSTEM}\n{}\n{H_CONTRACT}\n{}",
            layers.system, layers.output_contract
        );
        let policy_block = format!("{H_POLICY}\n{}", layers.policy);
        let tools_block = if layers.tools.is_empty() {
            String::new()
        } else {
            format!("{H_TOOLS}\n{}", layers.tools.join("\n"))
        };
        let user_block = format!("{H_USER}\n{}", layers.user_message);

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

        // Layer 3: grounding, highest relevance first (facts are pre-sorted).
        let fact_lines: Vec<String> = layers.grounding.facts.iter().map(|f| f.line()).collect();
        let (grounding_block, g_incl, running_after_g) =
            self.fill(H_GROUNDING, &fact_lines, budget, running);
        running = running_after_g;
        let included_citations: Vec<String> = layers
            .grounding
            .facts
            .iter()
            .take(g_incl)
            .map(|f| f.citation.clone())
            .collect();
        let dropped_facts = fact_lines.len() - g_incl;

        // Layer 4: memory (lowest relevance is dropped — caller supplies best first).
        let (memory_block, m_incl, running_after_m) =
            self.fill(H_MEMORY, &layers.memory, budget, running);
        running = running_after_m;
        let dropped_memory = layers.memory.len() - m_incl;

        // Layer 6: history, most-recent-first for inclusion (oldest dropped).
        let mut recent: Vec<String> = layers.history.clone();
        recent.reverse();
        let (history_block_rev, h_incl, running_after_h) =
            self.fill(H_HISTORY, &recent, budget, running);
        running = running_after_h;
        let dropped_history = layers.history.len() - h_incl;
        // Restore chronological order within the included window for the prompt.
        let history_block = restore_history_order(&history_block_rev);

        // Canonical layer order 1→7.
        let mut parts = vec![sys_block, policy_block];
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
        let mut text = parts.join("\n\n");

        // Layer 8: input guardrail pass over the fully-assembled text. A block halts
        // the loop; a sanitise verdict REPLACES the text with its rewritten form —
        // the offending fragment never reaches the model. [FR-MI-7]
        match self.guardrail.inspect(&text) {
            GuardVerdict::Block { reason } => return Err(AssemblyError::Blocked { reason }),
            GuardVerdict::Sanitise { rewritten, .. } => text = rewritten,
            GuardVerdict::Allow => {}
        }

        let token_count = self.count(&text);
        debug_assert_eq!(token_count, running, "token accounting must be exact");

        Ok(AssembledPrompt {
            text,
            included_citations,
            dropped_facts,
            dropped_memory,
            dropped_history,
            token_count,
        })
    }
}

/// Restore chronological order of history lines that were included most-recent
/// first, keeping the header on top.
fn restore_history_order(reversed_block: &str) -> String {
    if reversed_block.is_empty() {
        return String::new();
    }
    let mut lines = reversed_block.lines();
    let header = lines.next().unwrap_or("");
    let mut body: Vec<&str> = lines.collect();
    body.reverse();
    if body.is_empty() {
        header.to_string()
    } else {
        format!("{header}\n{}", body.join("\n"))
    }
}
