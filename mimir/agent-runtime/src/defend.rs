//! Self-defending input guardrail (A.3) — assembly layer 8.
//!
//! Screens the assembled prompt before the model sees it and **blocks
//! prompt-injection before any tool executes** (AC-MI-6). The A.3 prompt
//! (`mimir_prompts::A3_SELF_DEFENDING`) is the contract; [`SelfDefending`] is the
//! deterministic reference classifier that implements it (the real A.3 model
//! plugs in behind the same [`crate::guardrail::InputGuardrail`] seam).
//!
//! It judges **technique, not topic** — a legitimate question about a sensitive
//! subject is not an attack — which is what keeps the false-positive rate low
//! (EV-A3-fp). [FR-MI-7]

use crate::guardrail::{GuardVerdict, InputGuardrail};

/// The disposition of a candidate input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Safe to pass through.
    Allow,
    /// Passable after stripping the offending fragment.
    Sanitise,
    /// Must not reach the model.
    Block,
}

impl Verdict {
    /// The contract string.
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Allow => "allow",
            Verdict::Sanitise => "sanitise",
            Verdict::Block => "block",
        }
    }
}

/// Severity of the detected technique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Nothing detected.
    None,
    /// Low.
    Low,
    /// Medium.
    Medium,
    /// High.
    High,
    /// Critical.
    Critical,
}

/// A detected attack category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DefenseCategory {
    /// Prompt injection / instruction override.
    Injection,
    /// Jailbreak / policy evasion.
    Jailbreak,
    /// Data exfiltration.
    Exfiltration,
    /// Tool abuse / privilege escalation.
    ToolAbuse,
    /// Encoded/hidden instructions.
    Obfuscation,
}

/// The classifier's structured verdict (mirrors the A.3 JSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefenseVerdict {
    /// allow / sanitise / block.
    pub verdict: Verdict,
    /// Detected categories (sorted, deduplicated).
    pub categories: Vec<DefenseCategory>,
    /// Severity.
    pub severity: Severity,
    /// The sanitised input, when `verdict == Sanitise`.
    pub sanitised_input: Option<String>,
    /// A short explanation that never echoes the payload.
    pub explanation: String,
}

/// The deterministic reference self-defending classifier.
#[derive(Debug, Default, Clone, Copy)]
pub struct SelfDefending;

/// Phrases indicating instruction-override / injection.
const INJECTION: &[&str] = &[
    "ignore previous",
    "ignore all previous",
    "disregard previous",
    "disregard the above",
    "ignore your instructions",
    "ignore the above",
    "you are now",
    "new instructions:",
    "override your",
    "forget your instructions",
];
/// Phrases indicating jailbreak / policy evasion.
const JAILBREAK: &[&str] = &[
    "developer mode",
    "jailbreak",
    "no restrictions",
    "without any filter",
    "bypass your",
    "pretend you have no",
];
/// Phrases indicating exfiltration attempts.
const EXFILTRATION: &[&str] = &[
    "system prompt",
    "reveal your instructions",
    "print your instructions",
    "show your prompt",
    "your hidden",
    "api key",
    "secret key",
    "credentials",
    "exfiltrate",
];
/// Phrases indicating tool abuse / privilege escalation.
const TOOL_ABUSE: &[&str] = &[
    "escalate privileges",
    "sudo ",
    "drop table",
    "delete all",
    "grant me admin",
    "use the admin tool",
];

fn looks_obfuscated(lower: &str, raw: &str) -> bool {
    if raw.contains("\\x")
        || raw.contains("\\u")
        || lower.contains("rot13")
        || lower.contains("base64")
    {
        return true;
    }
    // A long unbroken run of base64-ish characters suggests an encoded payload.
    let mut run = 0usize;
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' {
            run += 1;
            if run >= 40 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

impl SelfDefending {
    /// Build the classifier.
    pub fn new() -> Self {
        Self
    }

    /// Classify a candidate input, judging technique rather than topic. Matches each
    /// technique phrase against both the lowercased text and a **de-obfuscated**
    /// (whitespace/punctuation/zero-width stripped) form, so a paraphrase-by-
    /// obfuscation — `IGNORE-PREVIOUS`, `ig​nore previous`, `i.g.n.o.r.e previous` —
    /// is caught even though it is not a literal blocklist entry. Semantic
    /// paraphrases (different words) are the [`Classifier`] model's job. [FR-MI-7]
    pub fn classify(&self, candidate: &str) -> DefenseVerdict {
        let lower = candidate.to_ascii_lowercase();
        let squashed = squash(candidate);
        let mut categories = Vec::new();

        if any_generalized(&lower, &squashed, INJECTION) {
            categories.push(DefenseCategory::Injection);
        }
        if any_generalized(&lower, &squashed, JAILBREAK) {
            categories.push(DefenseCategory::Jailbreak);
        }
        if any_generalized(&lower, &squashed, EXFILTRATION) {
            categories.push(DefenseCategory::Exfiltration);
        }
        if any_generalized(&lower, &squashed, TOOL_ABUSE) {
            categories.push(DefenseCategory::ToolAbuse);
        }
        let obfuscated = looks_obfuscated(&lower, candidate);
        if obfuscated {
            categories.push(DefenseCategory::Obfuscation);
        }

        if categories.is_empty() {
            return DefenseVerdict {
                verdict: Verdict::Allow,
                categories,
                severity: Severity::None,
                sanitised_input: None,
                explanation: "no attack technique detected".to_string(),
            };
        }

        // A hard technique (injection/jailbreak/exfiltration/tool-abuse) blocks.
        // Obfuscation alone is sanitised (strip and pass) at medium severity.
        let hard = categories
            .iter()
            .any(|c| *c != DefenseCategory::Obfuscation);
        if hard {
            let severity =
                if categories.contains(&DefenseCategory::Exfiltration) || categories.len() >= 2 {
                    Severity::Critical
                } else {
                    Severity::High
                };
            DefenseVerdict {
                verdict: Verdict::Block,
                categories,
                severity,
                sanitised_input: None,
                explanation: "input attempts an instruction-override or exfiltration technique"
                    .to_string(),
            }
        } else {
            DefenseVerdict {
                verdict: Verdict::Sanitise,
                categories,
                severity: Severity::Medium,
                sanitised_input: Some(strip_obfuscation(candidate)),
                explanation: "encoded content stripped before the model".to_string(),
            }
        }
    }

    /// The false-positive rate over a corpus of known-benign inputs — the share
    /// wrongly blocked. Tracked so a stricter classifier cannot regress usability
    /// (EV-A3-fp). [FR-MI-7]
    pub fn false_positive_rate(&self, benign: &[&str]) -> f64 {
        if benign.is_empty() {
            return 0.0;
        }
        let blocked = benign
            .iter()
            .filter(|b| self.classify(b).verdict == Verdict::Block)
            .count();
        blocked as f64 / benign.len() as f64
    }
}

/// Strip long encoded runs, keeping readable text — the real sanitisation rewrite.
fn strip_obfuscation(input: &str) -> String {
    input
        .split_whitespace()
        .filter(|tok| tok.len() < 40)
        .collect::<Vec<_>>()
        .join(" ")
}

/// De-obfuscate for matching: keep only lowercase ascii alphanumerics, dropping all
/// whitespace, punctuation, and zero-width characters an attacker inserts to break
/// up a known phrase.
fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// A phrase matches if it appears in the lowercased text, or — for multi-word
/// phrases (the ones obfuscated by spacing/punctuation) — in the de-obfuscated form.
/// Single-token needles are matched only literally, to avoid short-substring
/// false positives.
fn any_generalized(lower: &str, squashed: &str, needles: &[&str]) -> bool {
    needles
        .iter()
        .any(|n| lower.contains(n) || (n.contains(' ') && squashed.contains(&squash(n))))
}

/// Translate a classifier verdict into a guardrail verdict — a `Sanitise` now
/// carries its **rewritten** text, so sanitisation actually rewrites the prompt.
fn verdict_to_guard(v: DefenseVerdict) -> GuardVerdict {
    match v.verdict {
        Verdict::Block => GuardVerdict::Block {
            reason: format!("self-defending: {}", v.explanation),
        },
        Verdict::Sanitise => GuardVerdict::Sanitise {
            rewritten: v.sanitised_input.unwrap_or_default(),
            reason: format!("self-defending: {}", v.explanation),
        },
        Verdict::Allow => GuardVerdict::Allow,
    }
}

fn verdict_rank(v: Verdict) -> u8 {
    match v {
        Verdict::Allow => 0,
        Verdict::Sanitise => 1,
        Verdict::Block => 2,
    }
}

impl InputGuardrail for SelfDefending {
    fn inspect(&self, assembled: &str) -> GuardVerdict {
        // Sanitise now REWRITES (verdict_to_guard carries the rewritten text); it is
        // no longer downgraded to Allow. [RC-22]
        verdict_to_guard(self.classify(assembled))
    }
}

/// The A.3 classifier seam. A real model — served via `model-server` — plugs in
/// here so detection **generalises beyond the keyword pre-filter**: it judges the
/// technique semantically, catching paraphrased injections no blocklist enumerates.
/// The deterministic [`SelfDefending`] stays as a fast, cheap pre-filter. [FR-MI-7]
pub trait Classifier: Send + Sync {
    /// Classify a candidate input (the assembled prompt) for attack technique.
    fn classify_input(&self, candidate: &str) -> DefenseVerdict;
}

/// A self-defending guardrail **backed by the A.3 classifier model**, with the
/// deterministic [`SelfDefending`] as a fast pre-filter. The pre-filter blocks known
/// techniques cheaply; anything it passes is escalated to the model, and the
/// **stricter verdict wins** — so a paraphrased injection the keyword list misses is
/// still detected and blocked (or sanitised) before any tool executes. [RC-22]
pub struct ModelBackedGuardrail {
    prefilter: SelfDefending,
    model: Box<dyn Classifier>,
}

impl ModelBackedGuardrail {
    /// Back the guardrail with a classifier model.
    pub fn new(model: Box<dyn Classifier>) -> Self {
        Self {
            prefilter: SelfDefending::new(),
            model,
        }
    }

    /// Classify with the pre-filter, escalating to the model when the pre-filter
    /// does not already block; the stricter of the two verdicts wins.
    pub fn classify(&self, candidate: &str) -> DefenseVerdict {
        let pre = self.prefilter.classify(candidate);
        if pre.verdict == Verdict::Block {
            return pre; // fast path — no need to run the model
        }
        let model = self.model.classify_input(candidate);
        if verdict_rank(model.verdict) >= verdict_rank(pre.verdict) {
            model
        } else {
            pre
        }
    }
}

impl InputGuardrail for ModelBackedGuardrail {
    fn inspect(&self, assembled: &str) -> GuardVerdict {
        verdict_to_guard(self.classify(assembled))
    }
}
