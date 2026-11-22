//! The sample mode: the compiled-in sample data is the release's mock data, served
//! under the core's governance (markings, clearance, withheld rows, audited actions),
//! with Heimdall's authorise action behaving as `HeimdallMock` does. [FR-C-5]
//! [AC-C-13] [FR-C-4]
#![cfg(feature = "backend-fips")]

use attest::json::{parse, Json};
use midgard_client_core::consoles::common::AuditRecord;
use midgard_client_core::consoles::heimdall::{HeimdallTrack, HeimdallWorkflow};
use midgard_client_core::device_audit::DeviceAuditKey;
use midgard_client_core::keystore::SoftwareKeystore;
use midgard_client_core::marking::{MarkingLevel, Visible};
use midgard_client_core::sample_source::{SampleSource, SAMPLE_JSON, SAMPLE_RELEASE};
use midgard_client_core::session::{ActionState, Session};
use midgard_client_core::source::{DataSource, Delivered, SourceError};
use midgard_client_core::CORE_VERSION;
use pqc_agility::backend::fips::provider;
use pqc_agility::OsRng;

fn session(persona: &str) -> Session<SoftwareKeystore, SampleSource> {
    let ks = SoftwareKeystore::generate(&provider(), "device-root", &mut OsRng).unwrap();
    let (device, _) = DeviceAuditKey::create(&ks, &mut OsRng).unwrap();
    Session::open(ks, device, SampleSource::new().unwrap(), persona, "dev-1").unwrap()
}

fn shown<T>(rows: Vec<Visible<T>>) -> Vec<(String, T)> {
    rows.into_iter()
        .filter_map(|v| match v {
            Visible::Shown { id, item, .. } => Some((id, item)),
            Visible::Withheld { .. } => None,
        })
        .collect()
}

#[test]
fn the_sample_is_this_releases_mock_data_and_nothing_else() {
    assert_eq!(SAMPLE_RELEASE, CORE_VERSION);
    let body = parse(SAMPLE_JSON).unwrap();
    assert_eq!(
        body.get("format").and_then(Json::as_str),
        Some("midgard-sample-data")
    );
    let s = SampleSource::new().unwrap();
    let names: Vec<String> = s.collections().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names.len(), 52, "{names:?}");
    assert!(
        !names.iter().any(|n| n == "heimdall.motion"),
        "no synthetic hour"
    );
    for console in ["midgard", "admin", "heimdall", "forge", "mimir", "bifrost"] {
        assert!(names.iter().any(|n| n.starts_with(&format!("{console}."))));
    }
    // The personas named in the mocks.
    let ids: Vec<String> = s.personas().into_iter().map(|p| p.id).collect();
    assert_eq!(ids, ["k.rao", "j.benjamin", "k.osei", "a.holt"]);
}

#[test]
fn heimdalls_watch_officer_sees_the_web_picture() {
    // k.osei is HeimdallMock's own subject (confidential): the five tracks, as the web
    // console shows them.
    let mut s = session("k.osei");
    let tracks = shown(s.rows::<HeimdallTrack>("heimdall.tracks").unwrap());
    let ids: Vec<&str> = tracks.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["E-2214", "E-2231", "E-0442", "E-0871", "E-1108"]);
    let anomalous = &tracks.iter().find(|(id, _)| id == "E-0871").unwrap().1;
    assert_eq!(anomalous.track, "anomalous");
    assert_eq!(anomalous.conf, 0.54);
    assert_eq!(
        (anomalous.x, anomalous.y, anomalous.hdg),
        (74.0, 34.0, 197.0)
    );
    assert_eq!(anomalous.options.as_ref().map(Vec::len), Some(3));
}

#[test]
fn a_lower_clearance_sees_withheld_rows_not_the_data() {
    // k.rao (restricted, no compartment): the OPS-N tracks are withheld by level only.
    let mut s = session("k.rao");
    let rows = s.rows::<HeimdallTrack>("heimdall.tracks").unwrap();
    assert_eq!(rows.len(), 5);
    let withheld: Vec<MarkingLevel> = rows
        .iter()
        .filter_map(|v| match v {
            Visible::Withheld { level } => Some(*level),
            Visible::Shown { .. } => None,
        })
        .collect();
    assert_eq!(withheld, vec![MarkingLevel::Confidential; 4]);
    let shown = shown(rows);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].0, "E-0442");

    // At the source too: nothing of a withheld row but its level.
    let src = SampleSource::new().unwrap();
    let g = src.grant("k.rao").unwrap();
    for d in src.fetch(&g, "heimdall.workflows").unwrap() {
        if let Delivered::Withheld { level } = d {
            assert_eq!(level, MarkingLevel::Confidential);
        }
    }
}

#[test]
fn authorise_with_a_token_is_applied_and_audited_as_the_mock_does() {
    let mut s = session("k.osei");
    let wf = shown(s.rows::<HeimdallWorkflow>("heimdall.workflows").unwrap());
    assert!(wf.iter().any(|(id, w)| id == "W-0912" && w.human_gated));
    let id = s
        .request_action(
            "heimdall.authoriseWorkflow",
            "heimdall.workflows/W-0912",
            &[("humanGateToken".into(), "HG-test".into())],
        )
        .unwrap();
    assert!(matches!(s.action_state(&id), ActionState::Queued));
    s.sync().unwrap();
    let ActionState::Applied { audit, result } = s.action_state(&id) else {
        panic!("not applied: {:?}", s.action_state(&id));
    };
    assert_eq!(audit, "D-4473");
    assert_eq!(result, r#"{"applied":true}"#);

    let audit_rows = shown(s.rows::<AuditRecord>("heimdall.audit").unwrap());
    let (_, row) = &audit_rows[0];
    assert_eq!(row.id, "D-4473");
    assert_eq!(row.actor, "k.osei");
    // The core's wording, which the device key checks before signing.
    assert_eq!(row.action, "applied · heimdall.authoriseWorkflow");
    assert_eq!(row.resource, "heimdall.workflows/W-0912");
    assert_eq!(row.ts, "14:39:04");
    // Signed by this device's audit key, not claimed by the sample.
    assert_eq!(row.pq_signature, "device");
    // The seeded rows carry no build-time signature here.
    assert!(audit_rows[1..]
        .iter()
        .all(|(_, r)| r.pq_signature == "none"));

    // A second authorise: the next id and stamp, as the mock numbers them.
    let again = s
        .request_action(
            "heimdall.authoriseWorkflow",
            "heimdall.workflows/W-0913",
            &[("humanGateToken".into(), "HG-test-2".into())],
        )
        .unwrap();
    s.sync().unwrap();
    assert!(
        matches!(s.action_state(&again), ActionState::Applied { ref audit, .. } if audit == "D-4474")
    );
    let audit_rows = shown(s.rows::<AuditRecord>("heimdall.audit").unwrap());
    assert_eq!(audit_rows[0].1.ts, "14:39:06");
}

#[test]
fn a_human_gated_workflow_without_a_token_is_refused() {
    let mut s = session("k.osei");
    s.rows::<HeimdallWorkflow>("heimdall.workflows").unwrap();
    let id = s
        .request_action(
            "heimdall.authoriseWorkflow",
            "heimdall.workflows/W-0912",
            &[],
        )
        .unwrap();
    s.sync().unwrap();
    match s.action_state(&id) {
        ActionState::Refused { reason } => assert_eq!(
            reason,
            "Human gate required for action \"Re-task sensor feed NORTH-2 → E-0871\""
        ),
        other => panic!("not refused: {other:?}"),
    }
    // Refused actions are audited too (the core's rule), in the mock's numbering.
    let audit_rows = shown(s.rows::<AuditRecord>("heimdall.audit").unwrap());
    assert_eq!(audit_rows[0].1.id, "D-4473");
    assert_eq!(
        audit_rows[0].1.action,
        "refused · heimdall.authoriseWorkflow"
    );
}

#[test]
fn a_workflow_needing_no_gate_is_applied_without_a_token() {
    let mut s = session("j.benjamin");
    s.rows::<HeimdallWorkflow>("heimdall.workflows").unwrap();
    let id = s
        .request_action(
            "heimdall.authoriseWorkflow",
            "heimdall.workflows/W-0911",
            &[],
        )
        .unwrap();
    s.sync().unwrap();
    assert!(matches!(s.action_state(&id), ActionState::Applied { .. }));
}

#[test]
fn a_withheld_workflow_cannot_be_acted_on() {
    // k.rao cannot see the OPS-N workflows: the session refuses before queueing.
    let mut s = session("k.rao");
    s.rows::<HeimdallWorkflow>("heimdall.workflows").unwrap();
    assert!(s
        .request_action(
            "heimdall.authoriseWorkflow",
            "heimdall.workflows/W-0912",
            &[("humanGateToken".into(), "HG".into())],
        )
        .is_err());
}

#[test]
fn other_consoles_actions_are_not_there_yet() {
    let mut s = session("k.osei");
    s.rows::<HeimdallWorkflow>("heimdall.workflows").unwrap();
    let id = s
        .request_action("heimdall.rejectWorkflow", "heimdall.workflows/W-0912", &[])
        .unwrap();
    s.sync().unwrap();
    assert!(
        matches!(s.action_state(&id), ActionState::Refused { ref reason } if reason == "no such action: heimdall.rejectWorkflow")
    );
}

#[test]
fn offline_is_simulated_and_unknown_things_are_refused() {
    let mut src = SampleSource::new().unwrap();
    let g = src.grant("k.osei").unwrap();
    assert!(src.online());
    src.set_online(false);
    assert_eq!(
        src.fetch(&g, "heimdall.tracks").unwrap_err(),
        SourceError::Offline
    );
    src.set_online(true);
    assert_eq!(
        src.fetch(&g, "heimdall.motion").unwrap_err(),
        SourceError::UnknownCollection("heimdall.motion".into())
    );
    assert_eq!(
        src.grant("local").unwrap_err(),
        SourceError::UnknownPersona("local".into())
    );
}
