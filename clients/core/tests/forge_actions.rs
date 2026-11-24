//! Forge's actions in the sample mode, ported from the release's `ForgeMock`: a
//! pipeline build replays the mock's staged steps under the next version and run id;
//! a transform preview gives the mock's sandboxed diff; a commit is refused without a
//! human-gate token (`ForgeHumanGateRequiredError`'s words) and applied with one;
//! every outcome is audited in `forge.audit` in the mock's numbering, at the target's
//! marking; a pipeline above the persona is not there for them. Run through a real
//! session, so the core's own rules hold around them. [FR-C-4] [AC-C-13] [FR-F-1]
//! [FR-F-3] [FR-F-7] [NFR-AI-1]
#![cfg(feature = "backend-fips")]

use attest::json::{parse, Json};
use midgard_client_core::consoles::common::AuditRecord;
use midgard_client_core::consoles::forge::{ForgePipeline, ForgeTransform};
use midgard_client_core::demo::actions::forge::human_gate_required;
use midgard_client_core::device_audit::DeviceAuditKey;
use midgard_client_core::keystore::SoftwareKeystore;
use midgard_client_core::marking::{MarkingLevel, Visible};
use midgard_client_core::sample_source::SampleSource;
use midgard_client_core::session::{ActionState, Session};
use midgard_client_core::source::{DataSource, Delivered};
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

fn act(s: &mut S, verb: &str, target: &str, params: &[(&str, &str)]) -> ActionState {
    let params: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let action = s.request_action(verb, target, &params).unwrap();
    s.sync().unwrap();
    s.action_state(&action)
}

fn applied(state: ActionState) -> (String, Json) {
    match state {
        ActionState::Applied { audit, result } => (audit, parse(&result).unwrap()),
        other => panic!("not applied: {other:?}"),
    }
}

fn str_of<'a>(v: &'a Json, key: &str) -> &'a str {
    v.get(key).and_then(Json::as_str).unwrap_or_default()
}

#[test]
fn a_build_replays_the_mocks_steps_under_the_next_version() {
    let mut s = session("j.benjamin");
    let pipelines = shown(s.rows::<ForgePipeline>("forge.pipelines").unwrap());
    assert_eq!(
        pipelines.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        [
            "flight_tracks_eu",
            "sensor_fusion_live",
            "maintenance_rollup"
        ]
    );
    let (audit, result) = applied(act(
        &mut s,
        "forge.runPipeline",
        "forge.pipelines/flight_tracks_eu",
        &[],
    ));
    assert_eq!(audit, "FRG-2044");
    assert_eq!(str_of(&result, "runId"), "RUN-1601");
    assert_eq!(str_of(&result, "version"), "v42");
    assert_eq!(str_of(&result, "durationLabel"), "1m38s");
    let Some(Json::Arr(steps)) = result.get("steps") else {
        panic!("no steps")
    };
    let stages: Vec<&str> = steps.iter().map(|s| str_of(s, "stage")).collect();
    assert_eq!(
        stages,
        [
            "resolve",
            "transform",
            "transform",
            "seal",
            "project",
            "done"
        ]
    );
    assert_eq!(str_of(&steps[2], "status"), "warn");
    assert_eq!(
        str_of(&steps[5], "detail"),
        "build v42 OK · 1m38s · lineage signed (ML-DSA)"
    );

    // A second build of another pipeline: the next run and audit numbers.
    let (audit, result) = applied(act(
        &mut s,
        "forge.runPipeline",
        "forge.pipelines/maintenance_rollup",
        &[],
    ));
    assert_eq!(audit, "FRG-2045");
    assert_eq!(str_of(&result, "runId"), "RUN-1602");
    assert_eq!(str_of(&result, "version"), "v9");

    let rows = shown(s.rows::<AuditRecord>("forge.audit").unwrap());
    assert_eq!(rows[0].id, "FRG-2045");
    assert_eq!(rows[0].ts, "14:02:15");
    assert_eq!(rows[1].id, "FRG-2044");
    assert_eq!(rows[1].ts, "14:02:13");
    assert_eq!(rows[1].actor, "j.benjamin");
    assert_eq!(rows[1].action, "applied · forge.runPipeline");
    assert_eq!(rows[1].resource, "forge.pipelines/flight_tracks_eu");
    assert_eq!(rows[1].pq_signature, "device");
    // The mock's five seed records stay under the new ones.
    assert_eq!(rows.len(), 7);
    assert_eq!(rows[2].id, "FRG-2042");
}

#[test]
fn a_preview_gives_the_sandboxed_diff_and_commits_nothing() {
    let mut s = session("j.benjamin");
    let t = shown(s.rows::<ForgeTransform>("forge.transform").unwrap());
    let code = t[0].code.join("\n");
    let (audit, result) = applied(act(
        &mut s,
        "forge.previewTransform",
        "forge.transform/transform",
        &[("sql", &code)],
    ));
    assert_eq!(audit, "FRG-2044");
    let diff = result.get("diff").unwrap();
    assert_eq!(
        str_of(diff, "summary"),
        format!(
            "Preview of: {}…",
            code.trim().chars().take(72).collect::<String>()
        )
    );
    assert_eq!(diff.get("rowsIn"), Some(&Json::Num("48112".into())));
    assert_eq!(diff.get("rowsOut"), Some(&Json::Num("47924".into())));
    assert_eq!(str_of(diff, "expectations"), "12/12");
    let Some(Json::Arr(hunks)) = diff.get("hunks") else {
        panic!("no hunks")
    };
    assert_eq!(
        hunks.iter().map(|h| str_of(h, "op")).collect::<Vec<_>>(),
        ["~", "-", "+"]
    );
    // The transform is as it was.
    let after = shown(s.rows::<ForgeTransform>("forge.transform").unwrap());
    assert_eq!(after[0].next_version, "v43");
}

#[test]
fn a_commit_is_refused_without_a_token_and_applied_with_one() {
    let mut s = session("j.benjamin");
    s.rows::<ForgeTransform>("forge.transform").unwrap();
    for token in [None, Some(""), Some("  ")] {
        let params: Vec<(&str, &str)> = token
            .map(|t| vec![("humanGateToken", t)])
            .unwrap_or_default();
        match act(
            &mut s,
            "forge.applyTransform",
            "forge.transform/transform",
            &params,
        ) {
            ActionState::Refused { reason } => {
                assert_eq!(reason, human_gate_required("fuse_positions"));
                assert_eq!(reason, "Human gate required to commit \"fuse_positions\"");
            }
            other => panic!("{token:?}: not refused: {other:?}"),
        }
    }
    let (audit, result) = applied(act(
        &mut s,
        "forge.applyTransform",
        "forge.transform/transform",
        &[("sql", "x"), ("humanGateToken", "HG-forge-ide")],
    ));
    assert_eq!(audit, "FRG-2047");
    assert_eq!(str_of(&result, "version"), "v43");
    assert_eq!(result.get("applied"), Some(&Json::Bool(true)));
    let rows = shown(s.rows::<AuditRecord>("forge.audit").unwrap());
    let head: Vec<(&str, &str)> = rows
        .iter()
        .take(4)
        .map(|r| (r.id.as_str(), r.action.as_str()))
        .collect();
    assert_eq!(
        head,
        [
            ("FRG-2047", "applied · forge.applyTransform"),
            ("FRG-2046", "refused · forge.applyTransform"),
            ("FRG-2045", "refused · forge.applyTransform"),
            ("FRG-2044", "refused · forge.applyTransform"),
        ]
    );
}

#[test]
fn a_pipeline_above_the_persona_is_not_there_for_them_and_the_build_keeps_its_marking() {
    // K. Rao (restricted) does not see the OPS-N pipeline: withheld by count, and the
    // session will not queue an action on it.
    let mut s = session("k.rao");
    let rows = s.rows::<ForgePipeline>("forge.pipelines").unwrap();
    assert_eq!(
        rows.iter()
            .filter(|v| matches!(v, Visible::Withheld { .. }))
            .count(),
        1
    );
    assert!(s
        .request_action("forge.runPipeline", "forge.pipelines/flight_tracks_eu", &[])
        .is_err());
    // The restricted one is built for them.
    let (audit, _) = applied(act(
        &mut s,
        "forge.runPipeline",
        "forge.pipelines/sensor_fusion_live",
        &[],
    ));
    assert_eq!(audit, "FRG-2044");

    // J. Benjamin's build of the OPS-N pipeline is CONFIDENTIAL // OPS-N, withheld from
    // K. Rao at the source.
    let mut s = session("j.benjamin");
    s.rows::<ForgePipeline>("forge.pipelines").unwrap();
    applied(act(
        &mut s,
        "forge.runPipeline",
        "forge.pipelines/flight_tracks_eu",
        &[],
    ));
    let src = s.source();
    match &src
        .fetch(&src.grant("j.benjamin").unwrap(), "forge.audit")
        .unwrap()[0]
    {
        Delivered::Row { id, marking, .. } => {
            assert_eq!(id, "FRG-2044");
            assert_eq!(marking.level(), MarkingLevel::Confidential);
            assert_eq!(marking.compartments(), ["OPS-N".to_string()]);
        }
        other => panic!("not a row: {other:?}"),
    }
    assert!(matches!(
        src.fetch(&src.grant("k.rao").unwrap(), "forge.audit")
            .unwrap()[0],
        Delivered::Withheld {
            level: MarkingLevel::Confidential
        }
    ));
}
