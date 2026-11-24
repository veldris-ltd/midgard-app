//! Heimdall's actions in the sample mode, ported from the release's `HeimdallMock`:
//! the human gate refuses an authorisation without a token (`HumanGateRequiredError`'s
//! words) and applies it with one; every outcome is audited in `heimdall.audit` in the
//! mock's numbering, at the workflow's marking; a workflow above the persona is not
//! there for them; and the Feeds data lists the user's own feeds as unverified. Run
//! through a real session, so the core's own rules hold around them. (The happy path's
//! wording and numbering are also checked in `sample_source.rs`.) [FR-C-4] [AC-C-13]
//! [FR-H-3] [FR-H-4] [FR-H-5] [NFR-AI-1]
#![cfg(feature = "backend-fips")]

use midgard_client_core::consoles::common::AuditRecord;
use midgard_client_core::consoles::heimdall::{HeimdallFeed, HeimdallWorkflow};
use midgard_client_core::demo::actions::heimdall::human_gate_required;
use midgard_client_core::device_audit::DeviceAuditKey;
use midgard_client_core::keystore::SoftwareKeystore;
use midgard_client_core::marking::{MarkingLevel, Visible};
use midgard_client_core::sample_source::SampleSource;
use midgard_client_core::session::{ActionState, Session};
use midgard_client_core::source::{DataSource, Delivered};
use midgard_client_core::user_data::UserFeedRow;
use pqc_agility::backend::fips::provider;
use pqc_agility::OsRng;

type S = Session<SoftwareKeystore, SampleSource>;

fn session(persona: &str) -> S {
    let ks = SoftwareKeystore::generate(&provider(), "device-root", &mut OsRng).unwrap();
    let (device, _) = DeviceAuditKey::create(&ks, &mut OsRng).unwrap();
    Session::open(ks, device, SampleSource::new().unwrap(), persona, "dev-1").unwrap()
}

fn shown<T>(rows: Vec<Visible<T>>) -> Vec<T> {
    rows.into_iter()
        .filter_map(|v| match v {
            Visible::Shown { item, .. } => Some(item),
            Visible::Withheld { .. } => None,
        })
        .collect()
}

fn withheld<T>(rows: &[Visible<T>]) -> usize {
    rows.iter()
        .filter(|v| matches!(v, Visible::Withheld { .. }))
        .count()
}

/// Read the workflows (the session acts only on rows read), request, sync, and return
/// the action's state.
fn authorise(s: &mut S, id: &str, token: Option<&str>) -> ActionState {
    s.rows::<HeimdallWorkflow>("heimdall.workflows").unwrap();
    let params: Vec<(String, String)> = token
        .map(|t| vec![("humanGateToken".to_string(), t.to_string())])
        .unwrap_or_default();
    let action = s
        .request_action(
            "heimdall.authoriseWorkflow",
            &format!("heimdall.workflows/{id}"),
            &params,
        )
        .unwrap();
    s.sync().unwrap();
    s.action_state(&action)
}

#[test]
fn the_gate_refuses_without_a_token_and_a_blank_one() {
    let mut s = session("k.osei");
    for token in [None, Some(""), Some("   ")] {
        match authorise(&mut s, "W-0913", token) {
            ActionState::Refused { reason } => assert_eq!(
                reason,
                human_gate_required("Cross-cue maritime radar sweep on E-0871")
            ),
            other => panic!("{token:?}: not refused: {other:?}"),
        }
    }
    // Each refusal is audited, in the mock's numbering.
    let audit = shown(s.rows::<AuditRecord>("heimdall.audit").unwrap());
    let ids: Vec<&str> = audit.iter().take(3).map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["D-4475", "D-4474", "D-4473"]);
    assert!(audit
        .iter()
        .take(3)
        .all(|r| r.action == "refused · heimdall.authoriseWorkflow"
            && r.resource == "heimdall.workflows/W-0913"));
}

#[test]
fn authorised_with_a_token_after_a_refusal_takes_the_next_number() {
    let mut s = session("k.osei");
    assert!(matches!(
        authorise(&mut s, "W-0912", None),
        ActionState::Refused { .. }
    ));
    match authorise(&mut s, "W-0912", Some("HG-1")) {
        ActionState::Applied { audit, result } => {
            assert_eq!(audit, "D-4474");
            assert_eq!(result, r#"{"applied":true}"#);
        }
        other => panic!("not applied: {other:?}"),
    }
    let audit = shown(s.rows::<AuditRecord>("heimdall.audit").unwrap());
    assert_eq!(audit[0].id, "D-4474");
    assert_eq!(audit[0].ts, "14:39:06");
    assert_eq!(audit[0].actor, "k.osei");
    assert_eq!(audit[0].pq_signature, "device");
    // The mock's five seed records stay under the new ones.
    assert_eq!(audit.len(), 7);
    assert_eq!(audit[2].id, "D-4472");
}

#[test]
fn the_audit_row_carries_the_workflows_marking_and_is_withheld_below_it() {
    let mut s = session("k.osei");
    assert!(matches!(
        authorise(&mut s, "W-0912", Some("HG-1")),
        ActionState::Applied { .. }
    ));
    // At the source: the new row is CONFIDENTIAL // OPS-N, as its workflow is.
    let src = s.source();
    let analyst = src.grant("k.rao").unwrap();
    let delivered = src.fetch(&analyst, "heimdall.audit").unwrap();
    assert_eq!(delivered.len(), 6);
    assert!(delivered.iter().all(
        |d| matches!(d, Delivered::Withheld { level } if *level == MarkingLevel::Confidential)
    ));
    let osei = src.grant("k.osei").unwrap();
    match &src.fetch(&osei, "heimdall.audit").unwrap()[0] {
        Delivered::Row { id, marking, .. } => {
            assert_eq!(id, "D-4473");
            assert_eq!(marking.level(), MarkingLevel::Confidential);
            assert_eq!(marking.compartments(), ["OPS-N".to_string()]);
        }
        other => panic!("not a row: {other:?}"),
    }
}

#[test]
fn a_workflow_above_the_persona_is_not_there_for_them() {
    // A. Holt (internal) sees only W-0910 of the four; the rest are withheld by count.
    let mut s = session("a.holt");
    let rows = s.rows::<HeimdallWorkflow>("heimdall.workflows").unwrap();
    assert_eq!(withheld(&rows), 3);
    let wf = shown(rows);
    assert_eq!(wf.len(), 1);
    assert_eq!(wf[0].id, "W-0910");
    // The session refuses to queue an action on a row it did not deliver.
    assert!(s
        .request_action(
            "heimdall.authoriseWorkflow",
            "heimdall.workflows/W-0912",
            &[("humanGateToken".into(), "HG".into())],
        )
        .is_err());
    // The one it may see needs no gate, and is applied without a token.
    assert!(matches!(
        authorise(&mut s, "W-0910", None),
        ActionState::Applied { .. }
    ));
}

#[test]
fn the_feeds_list_the_users_own_feeds_as_unverified() {
    let mut s = session("k.osei");
    s.source_mut().set_user_feeds(&[UserFeedRow {
        id: "obs-1".into(),
        name: "Harbour log".into(),
        kind: "observations file".into(),
        rate: "file".into(),
        status_label: "UNVERIFIED FEED".into(),
        failing: false,
    }]);
    let feeds = shown(s.rows::<HeimdallFeed>("heimdall.feeds").unwrap());
    assert_eq!(feeds.len(), 7);
    let mine = feeds.iter().find(|f| f.name == "Harbour log").unwrap();
    assert!(!mine.auth, "never authenticated");
    assert_eq!(mine.status_label, "UNVERIFIED FEED");
    assert_eq!(mine.weight, "low");
    // Every sample feed but the quarantined LEGACY-L16 is authenticated.
    let failed: Vec<&str> = feeds
        .iter()
        .filter(|f| !f.auth)
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(failed, ["LEGACY-L16", "Harbour log"]);
}
