//! The **deterministic** rollout gate. It decides go/no-go from hard rules over the
//! release context, optionally tightened by an advisory [`RiskAssessment`]. The
//! hard rules run whether or not Mimir is present, so a safe-rollout policy is
//! always enforced (AC-B-6); when an assessment is supplied, its blocking concerns
//! and score can additionally gate the rollout (AC-B-5). The AI never decides — it
//! only informs this gate. [FR-B-6] [NFR-AI-1]

use crate::assessment::RiskAssessment;
use crate::context::ReleaseContext;

/// The hard-rule policy the deterministic gate enforces regardless of Mimir.
#[derive(Debug, Clone, Copy)]
pub struct RiskRules {
    /// Block if the release's tests did not pass.
    pub block_if_tests_failed: bool,
    /// Block if the SBOM delta introduces any critical CVE.
    pub block_if_critical_cve: bool,
    /// Require the ring strategy to begin with a canary.
    pub require_canary: bool,
    /// Block if an advisory score is at least this value (only applied when an
    /// assessment is supplied).
    pub advisory_block_threshold: u8,
}

impl Default for RiskRules {
    fn default() -> Self {
        Self {
            block_if_tests_failed: true,
            block_if_critical_cve: true,
            require_canary: true,
            advisory_block_threshold: 80,
        }
    }
}

/// The gate's decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateDecision {
    /// Whether the rollout may proceed.
    pub allow: bool,
    /// The reasons a rollout was blocked (empty when allowed).
    pub reasons: Vec<String>,
    /// Whether an advisory assessment contributed to the decision.
    pub used_advisory: bool,
}

impl GateDecision {
    /// Whether the rollout was blocked.
    pub fn blocked(&self) -> bool {
        !self.allow
    }
}

/// Decide whether a rollout may proceed. Hard rules always apply; `assessment`, when
/// present, can add advisory blocks. [FR-B-6] [AC-B-5] [AC-B-6]
pub fn decide(
    context: &ReleaseContext,
    rules: &RiskRules,
    assessment: Option<&RiskAssessment>,
) -> GateDecision {
    let mut reasons = Vec::new();

    // --- Hard rules: enforced with or without Mimir. [AC-B-6] ---
    if rules.block_if_tests_failed && !context.tests_passed {
        reasons.push("release tests did not pass".to_string());
    }
    if rules.block_if_critical_cve && context.sbom_new_critical_cves > 0 {
        reasons.push(format!(
            "{} new critical CVE(s) in the SBOM delta",
            context.sbom_new_critical_cves
        ));
    }
    if rules.require_canary && !context.ring_strategy.contains("canary") {
        reasons.push("ring strategy must begin with a canary".to_string());
    }

    // --- Advisory tightening: only when Mimir supplied an assessment. [AC-B-5] ---
    let used_advisory = assessment.is_some();
    if let Some(a) = assessment {
        for concern in &a.blocking_concerns {
            reasons.push(format!("advisory blocking concern: {concern}"));
        }
        if a.risk_score >= rules.advisory_block_threshold {
            reasons.push(format!(
                "advisory risk score {} >= threshold {}",
                a.risk_score, rules.advisory_block_threshold
            ));
        }
    }

    GateDecision {
        allow: reasons.is_empty(),
        reasons,
        used_advisory,
    }
}
