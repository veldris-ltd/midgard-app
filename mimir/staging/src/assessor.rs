//! The A.9 model-intake licence/risk assessor — **advisory only**. [FR-MS-6]
//!
//! During staging, a human reviewer must accept a model's licence and risk before
//! it is re-signed. The A.9 prompt (`mimir_prompts::A9_MODEL_INTAKE`) is the
//! contract for an assistant that *extracts* licence facts and risk flags from the
//! model card and metadata to inform that reviewer. [`ModelIntakeAssessor`] is the
//! deterministic reference implementation of that prompt (the real A.9 model plugs
//! in behind the same shape).
//!
//! Two properties are load-bearing and enforced by construction:
//! 1. **It never approves or rejects.** [`IntakeAssessment`] has no verdict field
//!    and no method that returns a decision — it can only *describe*. The human
//!    gate in [`crate::intake`] is the only thing that approves. (EV-A9-noverdict)
//! 2. **It marks unknowns rather than guessing.** Every licence fact the card is
//!    silent on is [`Tri::Unknown`], not a fabricated value. (EV-A9-unknown)

/// A three-valued fact: the card stated it true, stated it false, or was silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tri {
    /// The card affirmatively states this is true.
    True,
    /// The card affirmatively states this is false.
    False,
    /// The card is silent — recorded as unknown, never guessed.
    Unknown,
}

impl Tri {
    /// Render as the A.9 JSON token: `true`, `false`, or `"unknown"`.
    fn as_json(self) -> &'static str {
        match self {
            Tri::True => "true",
            Tri::False => "false",
            Tri::Unknown => "\"unknown\"",
        }
    }
}

/// The licence facts the assessor could extract from the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssessedLicence {
    /// The licence name/SPDX id, or `"unknown"` if unidentifiable.
    pub name: String,
    /// Whether the named licence is OSI-approved.
    pub osi_approved: Tri,
    /// Whether the licence permits redistribution.
    pub redistribution_ok: Tri,
    /// Extracted use restrictions (empty if none stated).
    pub use_restrictions: Vec<String>,
}

/// The assessor's structured output — mirrors the A.9 JSON. Note the deliberate
/// absence of any approve/reject/verdict field: it informs, it does not decide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeAssessment {
    /// Extracted licence facts.
    pub licence: AssessedLicence,
    /// Provenance risk flags (e.g. "unsigned upstream", "unclear lineage").
    pub provenance_flags: Vec<String>,
    /// Safety risk flags.
    pub safety_flags: Vec<String>,
    /// Aspects the card explicitly addresses.
    pub stated: Vec<String>,
    /// Aspects the card is silent on.
    pub unstated: Vec<String>,
}

fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            _ => o.push(c),
        }
    }
    o
}

fn json_str_array(items: &[String]) -> String {
    let inner: Vec<String> = items
        .iter()
        .map(|i| format!("\"{}\"", json_escape(i)))
        .collect();
    format!("[{}]", inner.join(", "))
}

impl IntakeAssessment {
    /// Render the A.9 JSON contract exactly. The reviewer sees this; it is the
    /// artefact EV-A9-noverdict scans for any approve/reject decision. There is
    /// none to find.
    pub fn to_json(&self) -> String {
        format!(
            "{{\n  \"licence\": {{\"name\": \"{name}\", \"osi_approved\": {osi}, \
\"redistribution_ok\": {redist}, \"use_restrictions\": {restr}}},\n  \
\"provenance_flags\": {prov},\n  \"safety_flags\": {safety},\n  \
\"stated_vs_unstated\": {{\"stated\": {stated}, \"unstated\": {unstated}}}\n}}",
            name = json_escape(&self.licence.name),
            osi = self.licence.osi_approved.as_json(),
            redist = self.licence.redistribution_ok.as_json(),
            restr = json_str_array(&self.licence.use_restrictions),
            prov = json_str_array(&self.provenance_flags),
            safety = json_str_array(&self.safety_flags),
            stated = json_str_array(&self.stated),
            unstated = json_str_array(&self.unstated),
        )
    }
}

/// A known licence and the facts that follow from its *identity* (not from
/// guessing about a given card).
struct KnownLicence {
    /// Substrings that identify this licence in a card (lowercased).
    markers: &'static [&'static str],
    /// Canonical name.
    name: &'static str,
    /// OSI-approval is a property of the licence itself.
    osi_approved: Tri,
    /// Whether the licence itself permits redistribution.
    redistribution_ok: Tri,
    /// Whether it is a custom/non-OSI licence that typically carries use terms.
    custom_terms: bool,
}

/// The licence identification table. OSI licences are established facts;
/// custom vendor licences (Gemma/Llama/NVIDIA/RAIL) are non-OSI and carry terms
/// the card must be read for. Order matters: more specific markers first.
const KNOWN_LICENCES: &[KnownLicence] = &[
    KnownLicence {
        markers: &[
            "gemma terms",
            "gemma license",
            "gemma licence",
            "google gemma",
        ],
        name: "Gemma Terms of Use",
        osi_approved: Tri::False,
        redistribution_ok: Tri::Unknown,
        custom_terms: true,
    },
    KnownLicence {
        markers: &[
            "llama 3 community",
            "llama community",
            "meta llama",
            "llama license",
        ],
        name: "Llama Community License",
        osi_approved: Tri::False,
        redistribution_ok: Tri::Unknown,
        custom_terms: true,
    },
    KnownLicence {
        markers: &[
            "nvidia open model",
            "nvidia community",
            "nvidia ai foundation",
        ],
        name: "NVIDIA Community Model License",
        osi_approved: Tri::False,
        redistribution_ok: Tri::Unknown,
        custom_terms: true,
    },
    KnownLicence {
        markers: &[
            "openrail",
            "bigscience rail",
            "responsible ai license",
            "rail license",
        ],
        name: "OpenRAIL",
        osi_approved: Tri::False,
        redistribution_ok: Tri::Unknown,
        custom_terms: true,
    },
    KnownLicence {
        markers: &["apache-2.0", "apache 2.0", "apache license", "apache-2"],
        name: "Apache-2.0",
        osi_approved: Tri::True,
        redistribution_ok: Tri::True,
        custom_terms: false,
    },
    KnownLicence {
        markers: &[
            "mit license",
            "mit licence",
            "licensed under mit",
            "spdx: mit",
            "(mit)",
        ],
        name: "MIT",
        osi_approved: Tri::True,
        redistribution_ok: Tri::True,
        custom_terms: false,
    },
    KnownLicence {
        markers: &["bsd-3", "bsd 3-clause", "bsd-2", "bsd license"],
        name: "BSD",
        osi_approved: Tri::True,
        redistribution_ok: Tri::True,
        custom_terms: false,
    },
    KnownLicence {
        markers: &["mpl-2.0", "mozilla public license"],
        name: "MPL-2.0",
        osi_approved: Tri::True,
        redistribution_ok: Tri::True,
        custom_terms: false,
    },
];

/// Headings/cues that open a use-restriction section in a model card.
const RESTRICTION_CUES: &[&str] = &[
    "prohibited use",
    "prohibited uses",
    "use restriction",
    "usage restriction",
    "acceptable use",
    "you may not",
    "you agree not to",
    "restrictions:",
    "not permitted to",
];

/// The seven card aspects tracked in stated-vs-unstated.
const ASPECTS: &[(&str, &[&str])] = &[
    ("licence", &["license", "licence", "spdx", "terms of use"]),
    (
        "intended_use",
        &["intended use", "intended for", "use cases", "designed for"],
    ),
    (
        "training_data",
        &["training data", "trained on", "dataset", "corpus"],
    ),
    (
        "evaluation",
        &["evaluation", "benchmark", "eval results", "accuracy"],
    ),
    ("limitations", &["limitation", "known issues", "caveats"]),
    (
        "safety",
        &["safety", "bias", "toxicity", "red team", "red-team"],
    ),
    (
        "provenance",
        &["base model", "fine-tuned from", "lineage", "derived from"],
    ),
];

/// The deterministic reference A.9 assessor.
#[derive(Debug, Default, Clone, Copy)]
pub struct ModelIntakeAssessor;

impl ModelIntakeAssessor {
    /// Build the assessor.
    pub fn new() -> Self {
        Self
    }

    /// Assess a candidate from its model card and fetch metadata. Advisory: the
    /// result describes licence facts and risk flags and contains **no** decision.
    pub fn assess(&self, model_card: &str, metadata: &str) -> IntakeAssessment {
        let card_lower = model_card.to_ascii_lowercase();
        let meta_lower = metadata.to_ascii_lowercase();

        let licence = self.assess_licence(model_card, &card_lower);
        let provenance_flags = self.provenance_flags(&card_lower, &meta_lower);
        let safety_flags = self.safety_flags(&card_lower);
        let (stated, unstated) = self.stated_vs_unstated(&card_lower, &licence);

        IntakeAssessment {
            licence,
            provenance_flags,
            safety_flags,
            stated,
            unstated,
        }
    }

    fn assess_licence(&self, card: &str, card_lower: &str) -> AssessedLicence {
        let restrictions = extract_restrictions(card);
        match KNOWN_LICENCES
            .iter()
            .find(|k| k.markers.iter().any(|m| card_lower.contains(m)))
        {
            Some(known) => {
                // Redistribution: prefer what the card states. Absent that, an OSI
                // licence permits it by definition; a custom-terms licence stays
                // Unknown rather than guessing from the licence identity alone.
                let redistribution_ok = stated_redistribution(card_lower).unwrap_or({
                    if known.custom_terms {
                        Tri::Unknown
                    } else {
                        known.redistribution_ok
                    }
                });
                AssessedLicence {
                    name: known.name.to_string(),
                    osi_approved: known.osi_approved,
                    redistribution_ok,
                    use_restrictions: restrictions,
                }
            }
            // Card is silent on licence: mark everything unknown, do NOT guess.
            None => AssessedLicence {
                name: "unknown".to_string(),
                osi_approved: Tri::Unknown,
                redistribution_ok: stated_redistribution(card_lower).unwrap_or(Tri::Unknown),
                use_restrictions: restrictions,
            },
        }
    }

    fn provenance_flags(&self, card_lower: &str, meta_lower: &str) -> Vec<String> {
        let mut flags = Vec::new();
        let both = |needle: &str| card_lower.contains(needle) || meta_lower.contains(needle);
        // Unsigned upstream: explicitly stated, or no signature evidence anywhere.
        let signed = both("signed") || both("gpg") || both("signature");
        if both("unsigned") || !signed {
            flags.push("unsigned upstream".to_string());
        }
        // Unclear lineage: stated, or no base-model/lineage statement at all.
        let has_lineage = both("base model")
            || both("fine-tuned from")
            || both("lineage")
            || both("derived from");
        if both("unclear lineage") || both("lineage unknown") || !has_lineage {
            flags.push("unclear lineage".to_string());
        }
        flags
    }

    fn safety_flags(&self, card_lower: &str) -> Vec<String> {
        let mut flags = Vec::new();
        if card_lower.contains("uncensored") || card_lower.contains("no guardrails") {
            flags.push("model advertises uncensored/no-guardrail behaviour".to_string());
        }
        let mentions_safety = ASPECTS
            .iter()
            .find(|(k, _)| *k == "safety")
            .map(|(_, kws)| kws.iter().any(|kw| card_lower.contains(kw)))
            .unwrap_or(false);
        if !mentions_safety {
            flags.push("no stated safety evaluation".to_string());
        }
        flags
    }

    fn stated_vs_unstated(
        &self,
        card_lower: &str,
        licence: &AssessedLicence,
    ) -> (Vec<String>, Vec<String>) {
        let mut stated = Vec::new();
        let mut unstated = Vec::new();
        for (aspect, keywords) in ASPECTS {
            let present = if *aspect == "licence" {
                // Licence counts as stated if we identified one OR a licence word appears.
                licence.name != "unknown" || keywords.iter().any(|kw| card_lower.contains(kw))
            } else {
                keywords.iter().any(|kw| card_lower.contains(kw))
            };
            if present {
                stated.push((*aspect).to_string());
            } else {
                unstated.push((*aspect).to_string());
            }
        }
        (stated, unstated)
    }
}

/// If the card affirmatively states a redistribution stance, return it; else None.
fn stated_redistribution(card_lower: &str) -> Option<Tri> {
    const DENY: &[&str] = &[
        "may not redistribute",
        "no redistribution",
        "redistribution is prohibited",
        "redistribution prohibited",
        "not be redistributed",
        "cannot be redistributed",
    ];
    const ALLOW: &[&str] = &[
        "may redistribute",
        "redistribution is permitted",
        "redistribution is allowed",
        "you may distribute",
        "free to redistribute",
    ];
    if DENY.iter().any(|p| card_lower.contains(p)) {
        Some(Tri::False)
    } else if ALLOW.iter().any(|p| card_lower.contains(p)) {
        Some(Tri::True)
    } else {
        None
    }
}

/// Extract use-restriction items from a model card: bullet/numbered items that
/// fall under a restriction heading, plus inline "you may not …" sentences.
/// Returns de-duplicated, order-preserving restriction strings.
fn extract_restrictions(card: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;

    let is_bullet = |t: &str| {
        t.starts_with("- ")
            || t.starts_with("* ")
            || t.starts_with("• ")
            || (t.len() >= 2 && t.as_bytes()[0].is_ascii_digit() && t[1..].starts_with(". "))
            || (t.len() >= 3 && t.as_bytes()[0].is_ascii_digit() && t[1..].starts_with(") "))
    };
    let strip_bullet = |t: &str| -> String {
        let t = t
            .trim_start_matches(['-', '*', '•', ' '])
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches(['.', ')', ' ']);
        t.trim().to_string()
    };

    for raw in card.lines() {
        let line = raw.trim();
        let lower = line.to_ascii_lowercase();

        if line.is_empty() {
            in_section = false;
            continue;
        }

        // A heading/label that opens a restriction section.
        let opens_section = RESTRICTION_CUES.iter().any(|cue| lower.contains(cue))
            && (line.ends_with(':')
                || line.starts_with('#')
                || lower.contains("prohibited")
                || lower.contains("restriction"));
        if opens_section {
            in_section = true;
            // If the heading itself is a "you may not …" sentence, capture it.
            if lower.starts_with("you may not") || lower.starts_with("you agree not to") {
                push_unique(&mut out, line.trim_end_matches(':').to_string());
                in_section = false;
            }
            continue;
        }

        if in_section && is_bullet(line) {
            let item = strip_bullet(line);
            if !item.is_empty() {
                push_unique(&mut out, item);
            }
            continue;
        }

        // Inline restriction sentences anywhere in the card.
        if lower.starts_with("you may not") || lower.starts_with("you agree not to") {
            push_unique(&mut out, line.trim_end_matches('.').to_string());
        }
    }
    out
}

fn push_unique(out: &mut Vec<String>, item: String) {
    if !out.contains(&item) {
        out.push(item);
    }
}
