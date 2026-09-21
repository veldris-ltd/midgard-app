//! AC-AD-8 — break-glass elevation is time-boxed, justified, alerted in real time,
//! and forces a post-hoc review. `[FR-AD-8]`

use admin_breakglass::{
    AlertKind, BreakGlassError, BreakGlassService, BreakGlassState, GrantRequest, RecordingAlerter,
};

fn req(id: &str, justification: &str, ttl: u64) -> GrantRequest {
    GrantRequest {
        id: id.into(),
        principal: "mallory".into(),
        approver: "secoff".into(),
        scope: "kms".into(),
        justification: justification.into(),
        ttl_secs: ttl,
    }
}

#[test]
fn ac_ad_8_grant_is_time_boxed_justified_and_alerted() {
    let mut svc = BreakGlassService::new(RecordingAlerter::new());

    // A justification is mandatory.
    assert_eq!(
        svc.grant(req("g0", "  ", 60), 1000).unwrap_err(),
        BreakGlassError::JustificationRequired
    );
    // The time-box must be non-zero.
    assert_eq!(
        svc.grant(req("g0", "prod incident", 0), 1000).unwrap_err(),
        BreakGlassError::InvalidTtl
    );

    // A valid grant: active within its box, and a real-time alert was raised.
    svc.grant(req("g1", "prod incident #42", 60), 1000).unwrap();
    assert!(svc.is_active("g1", 1000));
    assert!(svc.is_active("g1", 1059));
    assert_eq!(svc.state("g1", 1000), Some(BreakGlassState::Active));
    assert_eq!(svc.alerter().count(AlertKind::Granted), 1);

    // Past the time-box it no longer confers access.
    assert!(!svc.is_active("g1", 1060));
    assert_eq!(svc.state("g1", 1060), Some(BreakGlassState::Expired));
}

#[test]
fn ac_ad_8_expiry_forces_a_reviewed_post_hoc() {
    let mut svc = BreakGlassService::new(RecordingAlerter::new());
    svc.grant(req("g1", "prod incident #42", 60), 1000).unwrap();

    // Before expiry there is nothing to review.
    assert!(svc.pending_reviews(1030).is_empty());
    assert!(svc.all_reviewed(1030));

    // After expiry, a sweep raises a one-time alert and the grant enters the review
    // backlog — review cannot be skipped.
    let need = svc.sweep(1100);
    assert_eq!(need, vec!["g1".to_string()]);
    assert_eq!(svc.alerter().count(AlertKind::ExpiredUnreviewed), 1);
    assert_eq!(svc.pending_reviews(1100).len(), 1);
    assert!(!svc.all_reviewed(1100));

    // Sweeping again does not re-alert (idempotent).
    assert!(svc.sweep(1200).is_empty());
    assert_eq!(svc.alerter().count(AlertKind::ExpiredUnreviewed), 1);

    // The mandatory review clears the backlog.
    svc.review("g1", "auditor", "justified; scope appropriate", 1300)
        .unwrap();
    assert_eq!(svc.state("g1", 1300), Some(BreakGlassState::Reviewed));
    assert!(svc.pending_reviews(1300).is_empty());
    assert!(svc.all_reviewed(1300));

    // It cannot be reviewed twice.
    assert_eq!(
        svc.review("g1", "auditor", "again", 1400).unwrap_err(),
        BreakGlassError::AlreadyReviewed("g1".into())
    );
}

#[test]
fn ac_ad_8_break_glass_is_never_unilateral() {
    let mut svc = BreakGlassService::new(RecordingAlerter::new());
    let mut r = req("g1", "prod incident", 60);
    r.approver = "  ".into();
    assert_eq!(
        svc.grant(r, 1000).unwrap_err(),
        BreakGlassError::ApproverRequired
    );
}
