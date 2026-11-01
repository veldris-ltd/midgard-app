//! The release context: the evidence a risk assessment (and the deterministic
//! rollout gate) reason over. All fields are facts about the release — no clock, no
//! ambient state — so gating is reproducible. [FR-B-6]

/// Structured facts about a proposed release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseContext {
    /// A short human summary of the change.
    pub change_summary: String,
    /// Whether the release's test suite passed.
    pub tests_passed: bool,
    /// Number of newly-introduced critical CVEs in the SBOM delta.
    pub sbom_new_critical_cves: u32,
    /// Number of prior incidents attributed to the changed components.
    pub prior_incidents: u32,
    /// The proposed ring strategy, e.g. `canary->broad`.
    pub ring_strategy: String,
}

impl ReleaseContext {
    /// Render the context as the injected `RELEASE_CONTEXT` block for A.7. The
    /// author-written fields go through `mimir_prompts::delimit::inline`, so each
    /// fact stays on its one line. [VLD-ANB-MIDGARD-0080]
    pub fn render(&self) -> String {
        format!(
            "- change_summary: {}\n\
             - tests_passed: {}\n\
             - sbom_new_critical_cves: {}\n\
             - prior_incidents: {}\n\
             - ring_strategy: {}",
            mimir_prompts::delimit::inline(&self.change_summary),
            self.tests_passed,
            self.sbom_new_critical_cves,
            self.prior_incidents,
            mimir_prompts::delimit::inline(&self.ring_strategy)
        )
    }
}
