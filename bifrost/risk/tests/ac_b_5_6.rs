//! AC-B-5 — Mimir present: a risk score is produced and can gate the rollout.
//! AC-B-6 — Mimir absent: rule-based gates still enforce safe rollout.
//! Plus the EV-A7-* suite (cve / clean / evidence). [FR-B-6] [NFR-AI-1]

use bifrost_risk::{
    assess, decide, ev_a7_clean, ev_a7_cve, ev_a7_evidence, MockAdvisor, ReleaseContext, RiskRules,
    UnavailableAdvisor,
};

fn clean_release() -> ReleaseContext {
    ReleaseContext {
        change_summary: "docs + minor logging".into(),
        tests_passed: true,
        sbom_new_critical_cves: 0,
        prior_incidents: 0,
        ring_strategy: "canary->broad".into(),
    }
}

// A.7 response for a risky release (critical CVE).
const RISKY_RESPONSE: &str = r#"
{
  "risk_score": 92,
  "band": "critical",
  "top_factors": [
    {"factor": "critical CVE in dependency", "evidence": "SBOM delta adds openssl 3.0.0 with CVE-2024-XXXX"}
  ],
  "recommended_ring_strategy": "hold",
  "blocking_concerns": ["known critical CVE in the SBOM delta"]
}
"#;

// A.7 response for a clean release.
const CLEAN_RESPONSE: &str = r#"
{
  "risk_score": 12,
  "band": "low",
  "top_factors": [
    {"factor": "small diff", "evidence": "3 files changed, tests passing"}
  ],
  "recommended_ring_strategy": "canary->broad",
  "blocking_concerns": []
}
"#;

#[test]
fn ac_b_5_mimir_present_produces_a_score_that_can_gate() {
    // A risky release: Mimir yields a high score + blocking concern; the gate blocks.
    let ctx = ReleaseContext {
        change_summary: "bump openssl".into(),
        tests_passed: true,
        sbom_new_critical_cves: 0, // hard rules alone wouldn't catch it here
        prior_incidents: 1,
        ring_strategy: "canary->broad".into(),
    };
    let advisor = MockAdvisor::new(RISKY_RESPONSE);
    let assessment = assess(&ctx, &advisor).expect("advisory assessment");
    assert_eq!(assessment.risk_score, 92);

    let decision = decide(&ctx, &RiskRules::default(), Some(&assessment));
    assert!(
        decision.blocked(),
        "high advisory risk should gate the rollout"
    );
    assert!(decision.used_advisory);
    assert!(decision
        .reasons
        .iter()
        .any(|r| r.contains("blocking concern") || r.contains("risk score")));
}

#[test]
fn ac_b_6_mimir_absent_rule_based_gate_still_enforces_safe_rollout() {
    // No advisory at all (Mimir unreachable). The deterministic hard rules must
    // still block an unsafe release (failed tests) and allow a clean one.
    let advisor = UnavailableAdvisor;

    let unsafe_ctx = ReleaseContext {
        tests_passed: false,
        ..clean_release()
    };
    // Advisory unavailable → we proceed with no assessment.
    assert!(assess(&unsafe_ctx, &advisor).is_err());
    let decision = decide(&unsafe_ctx, &RiskRules::default(), None);
    assert!(
        decision.blocked(),
        "failed tests must block even without Mimir"
    );
    assert!(!decision.used_advisory);
    assert!(decision
        .reasons
        .iter()
        .any(|r| r.contains("tests did not pass")));

    // A clean release with no advisory is allowed by the rule-based gate.
    let clean = decide(&clean_release(), &RiskRules::default(), None);
    assert!(clean.allow);
    assert!(!clean.used_advisory);
}

#[test]
fn hard_rules_block_critical_cve_and_missing_canary_without_mimir() {
    let cve_ctx = ReleaseContext {
        sbom_new_critical_cves: 2,
        ..clean_release()
    };
    assert!(decide(&cve_ctx, &RiskRules::default(), None).blocked());

    let no_canary = ReleaseContext {
        ring_strategy: "broad".into(),
        ..clean_release()
    };
    assert!(decide(&no_canary, &RiskRules::default(), None).blocked());
}

#[test]
fn ev_a7_suite_scores_the_assessment() {
    let risky = bifrost_risk::RiskAssessment::parse(RISKY_RESPONSE).unwrap();
    let clean = bifrost_risk::RiskAssessment::parse(CLEAN_RESPONSE).unwrap();

    assert!(ev_a7_cve(&risky).passed(), "critical CVE must be flagged");
    assert!(
        ev_a7_clean(&clean).passed(),
        "clean release must not be over-flagged"
    );
    assert!(ev_a7_evidence(&risky).passed());
    assert!(ev_a7_evidence(&clean).passed());

    // The evals catch violations too: a "clean" verdict on the risky release fails.
    assert!(!ev_a7_clean(&risky).passed());
    assert!(!ev_a7_cve(&clean).passed());
}

#[test]
fn malformed_or_out_of_range_response_fails_closed() {
    assert!(bifrost_risk::RiskAssessment::parse("{ not json").is_err());
    // risk_score out of range.
    assert!(bifrost_risk::RiskAssessment::parse(r#"{"risk_score": 150, "band": "low"}"#).is_err());
    // unknown band.
    assert!(
        bifrost_risk::RiskAssessment::parse(r#"{"risk_score": 10, "band": "unknown"}"#).is_err()
    );
}
