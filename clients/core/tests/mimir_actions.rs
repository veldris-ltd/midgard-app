//! Mimir's actions in the sample mode, ported from the release's `MimirMock`: every
//! eval run and guardrail check answers exactly as the mock answered the golden inputs
//! (`clients/demo/golden/<release>/mimir.json`, recorded from the mock), with the
//! mock's audit numbering; the Agent Studio's proposal is refused without a human-gate
//! token, authorised with one and decided once; the Logic Builder's human gate cannot
//! be deleted, whatever a screen sends; a session above a persona's clearance is not
//! there for them. Run through a real session, so the core's own rules hold around
//! them. [FR-C-4] [AC-C-13] [FR-MI-2] [FR-MI-3] [FR-MI-4] [FR-MI-7] [NFR-AI-1]
#![cfg(feature = "backend-fips")]

use attest::json::{parse, Json};
use midgard_client_core::consoles::common::AuditRecord;
use midgard_client_core::consoles::mimir::{MimirEval, MimirLogicFn, MimirSession};
use midgard_client_core::consoles::studio::MimirStudioExchange;
use midgard_client_core::demo::actions::mimir::{human_gate_required, GATE_UNDELETABLE};
use midgard_client_core::demo::canon;
use midgard_client_core::device_audit::DeviceAuditKey;
use midgard_client_core::keystore::SoftwareKeystore;
use midgard_client_core::marking::Visible;
use midgard_client_core::sample_source::SampleSource;
use midgard_client_core::session::{ActionState, Session};
use pqc_agility::backend::fips::provider;
use pqc_agility::OsRng;

type S = Session<SoftwareKeystore, SampleSource>;

const GOLDEN: &str = include_str!("../../demo/golden/0.1.0-rc.3/mimir.json");

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

fn refused(state: ActionState) -> String {
    match state {
        ActionState::Refused { reason } => reason,
        other => panic!("not refused: {other:?}"),
    }
}

fn str_of<'a>(v: &'a Json, key: &str) -> &'a str {
    v.get(key).and_then(Json::as_str).unwrap_or_default()
}

/// The mock's answer without its `auditId` (which the port gives with the action).
fn without_audit(v: &Json) -> (String, Json) {
    let Json::Obj(pairs) = v else {
        panic!("not an object")
    };
    let audit = str_of(v, "auditId").to_string();
    let rest = pairs
        .iter()
        .filter(|(k, _)| k != "auditId")
        .cloned()
        .collect();
    (audit, Json::Obj(rest))
}

#[test]
fn eval_runs_and_guardrail_checks_answer_as_the_mock_did() {
    let golden = parse(GOLDEN).unwrap();
    assert_eq!(str_of(&golden, "format"), "midgard-golden");
    let mut s = session("j.benjamin");
    s.rows::<MimirEval>("mimir.evals").unwrap();
    s.rows::<midgard_client_core::consoles::mimir::MimirGuardrail>("mimir.guardrails")
        .unwrap();

    let runs = golden.get("runEval").and_then(Json::as_array).unwrap();
    assert_eq!(runs.len(), 4);
    for run in runs {
        let id = str_of(run, "id");
        let (want_audit, want) = without_audit(run.get("result").unwrap());
        let (audit, got) = applied(act(
            &mut s,
            "mimir.runEval",
            &format!("mimir.evals/{id}"),
            &[],
        ));
        assert_eq!(audit, want_audit, "{id}");
        assert_eq!(canon::encode(&got), canon::encode(&want), "{id}");
    }

    let checks = golden
        .get("simulateGuardrail")
        .and_then(Json::as_array)
        .unwrap();
    assert!(checks.len() >= 30);
    let mut codes = std::collections::BTreeSet::new();
    for check in checks {
        let input = str_of(check, "input");
        let (want_audit, want) = without_audit(check.get("result").unwrap());
        let (audit, got) = applied(act(
            &mut s,
            "mimir.simulateGuardrail",
            "mimir.guardrails/*",
            &[("input", input)],
        ));
        assert_eq!(audit, want_audit, "{input:?}");
        assert_eq!(canon::encode(&got), canon::encode(&want), "{input:?}");
        codes.insert(str_of(&got, "code").to_string());
    }
    // Every verdict the mock has is among the examples.
    assert_eq!(
        codes.into_iter().collect::<Vec<_>>(),
        [
            "PASS",
            "REFUSED · EXFILTRATION",
            "REFUSED · PROMPT-INJECTION"
        ]
    );

    // The ledger: newest first, in the mock's numbering and stamps, the persona the actor.
    let rows = shown(s.rows::<AuditRecord>("mimir.audit").unwrap());
    assert_eq!(rows.len(), runs.len() + checks.len());
    let first = rows.last().unwrap();
    assert_eq!(first.id, "A-77121");
    assert_eq!(first.ts, "14:32:10");
    assert_eq!(first.actor, "j.benjamin");
    assert_eq!(first.action, "applied · mimir.runEval");
    assert_eq!(first.resource, "mimir.evals/dispatch-core");
    assert_eq!(rows[rows.len() - 2].ts, "14:32:12");
    assert_eq!(rows[0].resource, "mimir.guardrails/*");
}

#[test]
fn a_suite_below_the_gate_blocks_and_one_above_clears() {
    let mut s = session("a.holt");
    let suites = shown(s.rows::<MimirEval>("mimir.evals").unwrap());
    assert_eq!(suites[3].id, "risk-calibration");
    let (_, run) = applied(act(
        &mut s,
        "mimir.runEval",
        "mimir.evals/risk-calibration",
        &[],
    ));
    assert_eq!(run.get("score"), Some(&Json::Num("87.5".into())));
    assert_eq!(run.get("gateClear"), Some(&Json::Bool(false)));
    assert_eq!(str_of(&run, "suite"), "risk-calibration");
    let (_, run) = applied(act(
        &mut s,
        "mimir.runEval",
        "mimir.evals/dispatch-core",
        &[],
    ));
    assert_eq!(run.get("gateClear"), Some(&Json::Bool(true)));
    // An unknown suite is not a target.
    assert!(s
        .request_action("mimir.runEval", "mimir.evals/no-such-suite", &[])
        .is_err());
}

#[test]
fn the_proposal_needs_a_token_and_is_decided_once() {
    let mut s = session("j.benjamin");
    let ex = shown(
        s.rows::<MimirStudioExchange>("mimir.studioExchange")
            .unwrap(),
    );
    assert_eq!(ex.len(), 1);
    assert_eq!(ex[0].proposal, "Schedule maintenance · AC-2214 → FAC-DUB");
    assert_eq!(
        ex[0].answer.cites,
        [
            "Aircraft/AC-2214",
            "MaintenanceEvent/ME-88121",
            "fn check_slack()"
        ]
    );
    assert_eq!(ex[0].follow_up.cites.len(), 2);
    let target = "mimir.studioExchange/dispatch";
    for token in [None, Some(""), Some("  ")] {
        let params: Vec<(&str, &str)> = token
            .map(|t| vec![("humanGateToken", t)])
            .unwrap_or_default();
        let reason = refused(act(&mut s, "mimir.applyProposal", target, &params));
        assert_eq!(reason, human_gate_required(&ex[0].proposal));
        assert_eq!(
            reason,
            "Human gate required to authorise \"Schedule maintenance · AC-2214 → FAC-DUB\""
        );
    }
    let (audit, result) = applied(act(
        &mut s,
        "mimir.applyProposal",
        target,
        &[("humanGateToken", "HG-studio-1")],
    ));
    assert_eq!(audit, "A-77124");
    assert_eq!(result.get("authorised"), Some(&Json::Bool(true)));
    match act(
        &mut s,
        "mimir.applyProposal",
        target,
        &[("humanGateToken", "HG-studio-2")],
    ) {
        ActionState::Conflict { reason } => assert!(reason.contains("already authorised")),
        other => panic!("not a conflict: {other:?}"),
    }
    let rows = shown(s.rows::<AuditRecord>("mimir.audit").unwrap());
    let head: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r.id.as_str(), r.action.as_str()))
        .collect();
    assert_eq!(
        head,
        [
            ("A-77125", "conflict · mimir.applyProposal"),
            ("A-77124", "applied · mimir.applyProposal"),
            ("A-77123", "refused · mimir.applyProposal"),
            ("A-77122", "refused · mimir.applyProposal"),
            ("A-77121", "refused · mimir.applyProposal"),
        ]
    );
}

fn steps_json(logic: &MimirLogicFn, keep: impl Fn(usize, &str) -> bool) -> String {
    let steps: Vec<Json> = logic
        .steps
        .iter()
        .enumerate()
        .filter(|(i, st)| keep(*i, &st.kind))
        .map(|(_, st)| {
            Json::Obj(vec![
                ("kind".into(), Json::Str(st.kind.clone())),
                ("title".into(), Json::Str(st.title.clone())),
                ("body".into(), Json::Str(st.body.clone())),
                ("icon".into(), Json::Str(st.icon.clone())),
            ])
        })
        .collect();
    canon::encode(&Json::Arr(steps))
}

#[test]
fn the_human_gate_cannot_be_deleted_whatever_is_sent() {
    let mut s = session("j.benjamin");
    let logic = shown(s.rows::<MimirLogicFn>("mimir.logic").unwrap());
    let dispatch = logic.iter().find(|l| l.id == "dispatch").unwrap().clone();
    assert_eq!(dispatch.steps.len(), 7);
    assert_eq!(dispatch.steps[5].kind, "gate");
    let target = "mimir.logic/dispatch";

    // The gate deleted: refused by the core, not only by a screen.
    let no_gate = steps_json(&dispatch, |_, k| k != "gate");
    let reason = refused(act(
        &mut s,
        "mimir.saveLogic",
        target,
        &[("steps", &no_gate)],
    ));
    assert_eq!(reason, GATE_UNDELETABLE);
    // The gate moved before the action it guards: refused too.
    let mut kinds: Vec<Json> = parse(&steps_json(&dispatch, |_, _| true))
        .unwrap()
        .as_array()
        .unwrap()
        .to_vec();
    kinds.swap(4, 5);
    let swapped = canon::encode(&Json::Arr(kinds));
    let reason = refused(act(
        &mut s,
        "mimir.saveLogic",
        target,
        &[("steps", &swapped)],
    ));
    assert_eq!(reason, GATE_UNDELETABLE);
    // Malformed steps are refused with their reason.
    let reason = refused(act(&mut s, "mimir.saveLogic", target, &[("steps", "{}")]));
    assert_eq!(reason, "steps must be a JSON array of steps");
    let reason = refused(act(
        &mut s,
        "mimir.saveLogic",
        target,
        &[(
            "steps",
            r#"[{"kind":"wish","title":"x","body":"","icon":"zap"}]"#,
        )],
    ));
    assert_eq!(reason, "step 1: unknown kind \"wish\"");

    // A step deleted that is not the gate (the confidence check): saved, renumbered.
    let without_check = steps_json(&dispatch, |_, k| k != "branch");
    let (_, result) = applied(act(
        &mut s,
        "mimir.saveLogic",
        target,
        &[("steps", &without_check)],
    ));
    assert_eq!(result.get("steps"), Some(&Json::Num("6".into())));
    assert_eq!(result.get("gates"), Some(&Json::Num("1".into())));
    assert_eq!(str_of(&result, "version"), "v7");
    let after = shown(s.rows::<MimirLogicFn>("mimir.logic").unwrap());
    let d = after.iter().find(|l| l.id == "dispatch").unwrap();
    assert_eq!(
        d.steps.iter().map(|st| st.n as u32).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6]
    );
    assert_eq!(d.steps[4].kind, "gate");
    // The other agents' logic is untouched.
    assert_eq!(
        after.iter().find(|l| l.id == "triage").unwrap().steps.len(),
        7
    );
}

#[test]
fn sessions_above_the_persona_are_not_there_for_them() {
    // K. Rao (restricted) sees neither trace (OPS-N, SIG); J. Benjamin (OPS-N) sees one;
    // K. Osei (OPS-N, SIG) both.
    let count = |persona: &str| {
        let mut s = session(persona);
        let rows = s.rows::<MimirSession>("mimir.sessions").unwrap();
        let withheld = rows
            .iter()
            .filter(|v| matches!(v, Visible::Withheld { .. }))
            .count();
        (shown(rows).len(), withheld)
    };
    assert_eq!(count("k.rao"), (0, 2));
    assert_eq!(count("j.benjamin"), (1, 1));
    assert_eq!(count("k.osei"), (2, 0));
    // No action can be queued on a trace the persona does not see.
    let mut s = session("k.rao");
    s.rows::<MimirSession>("mimir.sessions").unwrap();
    assert!(s
        .request_action("mimir.runEval", "mimir.sessions/S-8812", &[])
        .is_err());
}
