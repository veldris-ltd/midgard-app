//! Admin's actions in the sample mode, ported from the release's `AdminMock`: each
//! succeeds as the mock answers, refuses as it refuses (and where the port adds a
//! refusal for governance), and is audited in `admin.audit` in the mock's numbering,
//! denials included. Run through a real session, so the core's own rules (targets the
//! persona may see, one decision per action, device-signed audit rows) hold around
//! them. [FR-C-4] [AC-C-13] [AC-AD-4] [AC-AD-6] [AC-AD-7] [FR-AD-8]
#![cfg(feature = "backend-fips")]

use attest::json::{parse, Json};
use midgard_client_core::consoles::admin::{
    AdminApproval, AdminBreakGlass, AdminConfigCommit, AdminLedgerEntry,
};
use midgard_client_core::consoles::common::AuditRecord;
use midgard_client_core::device_audit::DeviceAuditKey;
use midgard_client_core::keystore::SoftwareKeystore;
use midgard_client_core::marking::Visible;
use midgard_client_core::sample_source::SampleSource;
use midgard_client_core::session::{ActionState, Session, SessionError};
use pqc_agility::backend::fips::provider;
use pqc_agility::OsRng;

type S = Session<SoftwareKeystore, SampleSource>;

fn session(persona: &str) -> S {
    let ks = SoftwareKeystore::generate(&provider(), "device-root", &mut OsRng).unwrap();
    let (device, _) = DeviceAuditKey::create(&ks, &mut OsRng).unwrap();
    Session::open(ks, device, SampleSource::new().unwrap(), persona, "dev-1").unwrap()
}

fn switch(s: S, persona: &str) -> S {
    s.switch_persona(persona).map_err(|(_, e)| e).unwrap()
}

fn shown<T>(rows: Vec<Visible<T>>) -> Vec<T> {
    rows.into_iter()
        .filter_map(|v| match v {
            Visible::Shown { item, .. } => Some(item),
            Visible::Withheld { .. } => None,
        })
        .collect()
}

/// Request, sync and return the action's state.
fn act(s: &mut S, verb: &str, target: &str, params: &[(&str, &str)]) -> ActionState {
    let params: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let id = s.request_action(verb, target, &params).unwrap();
    s.sync().unwrap();
    s.action_state(&id)
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

fn ledger(s: &mut S) -> Vec<AuditRecord> {
    shown(s.rows::<AuditRecord>("admin.audit").unwrap())
}

fn approval(s: &mut S, id: &str) -> AdminApproval {
    shown(s.rows::<AdminApproval>("admin.approvals").unwrap())
        .into_iter()
        .find(|a| a.id == id)
        .unwrap()
}

#[test]
fn the_live_ledger_starts_with_the_mocks_seed_records() {
    let mut s = session("k.osei");
    let ids: Vec<String> = ledger(&mut s).into_iter().map(|r| r.id).collect();
    assert_eq!(ids, ["B-0331", "A-9107", "A-9108"]);
    // A.Holt (internal) does not see the restricted ledger.
    let mut s = switch(s, "a.holt");
    assert!(ledger(&mut s).is_empty());
}

#[test]
fn self_approval_is_denied_and_audited_and_a_second_person_approves() {
    // j.benjamin requested A-9110: approving it himself is denied, recorded on the
    // item, and audited as the mock numbers and stamps its rows.
    let mut s = session("j.benjamin");
    s.rows::<AdminApproval>("admin.approvals").unwrap();
    let why = refused(act(&mut s, "admin.approve", "admin.approvals/A-9110", &[]));
    assert_eq!(
        why,
        "separation-of-duties: requester cannot approve own request"
    );
    let a = approval(&mut s, "A-9110");
    assert_eq!(a.state, "denied");
    assert_eq!(
        a.denied_reason.as_deref(),
        Some("DENIED · SELF-APPROVAL BLOCKED (AC-AD-4)")
    );
    let row = ledger(&mut s).remove(0);
    assert_eq!((row.id.as_str(), row.ts.as_str()), ("A-9111", "14:02:13"));
    assert_eq!(row.action, "refused · admin.approve");
    assert_eq!(row.resource, "admin.approvals/A-9110");
    assert_eq!(row.actor, "j.benjamin");
    assert_eq!(row.pq_signature, "device");

    // k.osei, a second person, approves A-9109 (requested by j.benjamin).
    let mut s = switch(s, "k.osei");
    s.rows::<AdminApproval>("admin.approvals").unwrap();
    let (audit, decision) = applied(act(&mut s, "admin.approve", "admin.approvals/A-9109", &[]));
    assert_eq!(audit, "A-9112");
    assert_eq!(str_of(&decision, "decision"), "granted");
    assert_eq!(
        str_of(&decision, "reason"),
        "granted by k.osei · token 10m · single-use"
    );
    let a = approval(&mut s, "A-9109");
    assert_eq!(
        (a.state.as_str(), a.approver.as_str()),
        ("approved", "k.osei")
    );
    let row = ledger(&mut s).remove(0);
    assert_eq!((row.id.as_str(), row.ts.as_str()), ("A-9112", "14:02:15"));
    assert_eq!(row.action, "applied · admin.approve");

    // A decided approval is not decided again.
    match act(&mut s, "admin.approve", "admin.approvals/A-9110", &[]) {
        ActionState::Conflict { reason } => assert_eq!(reason, "approval A-9110 is already denied"),
        other => panic!("not a conflict: {other:?}"),
    }
    assert_eq!(ledger(&mut s).remove(0).action, "conflict · admin.approve");
}

#[test]
fn a_config_change_is_simulated_committed_and_rolled_back() {
    let mut s = session("k.osei");
    s.rows::<AdminConfigCommit>("admin.configCommits").unwrap();
    let (audit, sim) = applied(act(
        &mut s,
        "admin.simulateConfig",
        "admin.configCommits/CFG-2214",
        &[],
    ));
    assert_eq!(audit, "A-9111");
    let lines = |key: &str| -> Vec<String> {
        sim.get(key)
            .and_then(Json::as_array)
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        lines("grants"),
        [
            "datasets: [flight_tracks_eu, maintenance_events]",
            "expiry: 2026-10-06 (90d)"
        ]
    );
    assert_eq!(lines("revokes"), ["datasets: [flight_tracks_eu]"]);
    assert!(str_of(&sim, "summary").starts_with("Grants 14 users read on INTERNAL-marked"));
    assert_eq!(
        ledger(&mut s).remove(0).action,
        "applied · admin.simulateConfig"
    );

    let (_, commit) = applied(act(
        &mut s,
        "admin.commitConfig",
        "admin.configCommits/CFG-2214",
        &[],
    ));
    assert_eq!(str_of(&commit, "version"), "v482");
    assert_eq!(commit.get("committed"), Some(&Json::Bool(true)));
    let (audit, back) = applied(act(
        &mut s,
        "admin.rollbackConfig",
        "admin.configCommits/CFG-2214",
        &[],
    ));
    assert_eq!(audit, "A-9113");
    assert_eq!(str_of(&back, "version"), "v483");
    assert_eq!(back.get("rolledBack"), Some(&Json::Bool(true)));
    let c = shown(s.rows::<AdminConfigCommit>("admin.configCommits").unwrap())
        .into_iter()
        .find(|c| c.id == "CFG-2214")
        .unwrap();
    assert_eq!(c.state, "rolled-back");
    let row = ledger(&mut s).remove(0);
    assert_eq!(
        (row.action.as_str(), row.resource.as_str()),
        (
            "applied · admin.rollbackConfig",
            "admin.configCommits/CFG-2214"
        )
    );

    // A change that is not there is refused before anything is queued, as a hidden
    // one would be.
    assert!(matches!(
        s.request_action("admin.commitConfig", "admin.configCommits/CFG-9999", &[]),
        Err(SessionError::AboveClearance)
    ));
}

#[test]
fn the_ledger_search_reads_the_mocks_tokens_over_what_the_persona_may_read() {
    let mut s = session("k.osei");
    s.rows::<AdminLedgerEntry>("admin.ledger").unwrap();
    let ids = |v: &Json| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|e| str_of(e, "id").to_string())
            .collect()
    };
    let (audit, all) = applied(act(
        &mut s,
        "admin.searchLedger",
        "admin.ledger/*",
        &[("query", "")],
    ));
    assert_eq!(audit, "A-9111");
    assert_eq!(
        ids(&all),
        ["A-9108", "A-9107", "B-0331", "A-9104", "A-9101"]
    );
    let (_, osei) = applied(act(
        &mut s,
        "admin.searchLedger",
        "admin.ledger/*",
        &[("query", "actor:k.osei subsystem:kms since:24h")],
    ));
    assert_eq!(ids(&osei), ["A-9108", "B-0331"]);
    let (_, free) = applied(act(
        &mut s,
        "admin.searchLedger",
        "admin.ledger/*",
        &[("query", "CVE")],
    ));
    assert_eq!(ids(&free), ["A-9104"]);
    let row = ledger(&mut s).remove(0);
    assert_eq!(
        (row.action.as_str(), row.resource.as_str()),
        ("applied · admin.searchLedger", "admin.ledger/*")
    );

    // One entry is not a search.
    let why = refused(act(
        &mut s,
        "admin.searchLedger",
        "admin.ledger/A-9108",
        &[],
    ));
    assert_eq!(why, "a ledger search is on admin.ledger/*");

    // A.Holt (internal) may read none of the restricted ledger: nothing is queued.
    let mut s = switch(s, "a.holt");
    s.rows::<AdminLedgerEntry>("admin.ledger").unwrap();
    assert!(matches!(
        s.request_action("admin.searchLedger", "admin.ledger/*", &[]),
        Err(SessionError::AboveClearance)
    ));
}

#[test]
fn break_glass_opens_time_boxed_for_the_requester_and_is_audited() {
    let mut s = session("j.benjamin");
    s.rows::<AdminBreakGlass>("admin.breakGlass").unwrap();
    let (audit, bg) = applied(act(
        &mut s,
        "admin.requestBreakGlass",
        "admin.breakGlass/breakglass",
        &[("reason", "  HSM failover mid-rotation  ")],
    ));
    assert_eq!(audit, "A-9111");
    assert_eq!(str_of(&bg, "state"), "active");
    assert_eq!(str_of(&bg, "holder"), "j.benjamin");
    assert_eq!(str_of(&bg, "remaining"), "00:59:59");
    assert_eq!(
        str_of(&bg, "scope"),
        "KMS · unwrap only · enclosure-root-03"
    );
    assert_eq!(str_of(&bg, "justification"), "HSM failover mid-rotation");
    let now = shown(s.rows::<AdminBreakGlass>("admin.breakGlass").unwrap()).remove(0);
    assert_eq!(
        (now.state.as_str(), now.holder.as_str()),
        ("active", "j.benjamin")
    );
    let row = ledger(&mut s).remove(0);
    assert_eq!(row.action, "applied · admin.requestBreakGlass");

    // Without a reason the mock's reminder stands in.
    let (_, bg) = applied(act(
        &mut s,
        "admin.requestBreakGlass",
        "admin.breakGlass/breakglass",
        &[],
    ));
    assert_eq!(
        str_of(&bg, "justification"),
        "Emergency elevation requested — justification mandatory."
    );

    // K. Rao (restricted) does not see the confidential break-glass state, so cannot
    // request it, nor read the audit row the request left.
    let mut s = switch(s, "k.rao");
    let rows = s.rows::<AdminBreakGlass>("admin.breakGlass").unwrap();
    assert!(matches!(rows.as_slice(), [Visible::Withheld { .. }]));
    assert!(matches!(
        s.request_action(
            "admin.requestBreakGlass",
            "admin.breakGlass/breakglass",
            &[]
        ),
        Err(SessionError::AboveClearance)
    ));
    assert!(ledger(&mut s)
        .iter()
        .all(|r| r.action != "applied · admin.requestBreakGlass"));
}
