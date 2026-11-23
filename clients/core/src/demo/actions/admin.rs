//! Admin's actions in the sample mode, ported method by method from the release's
//! `AdminMock` (`sdk/ts/src/mock/admin.ts`), which is what the rc.3 web console runs on
//! in mock mode. `[FR-C-4]` `[AC-C-13]` `[AC-AD-4]` `[AC-AD-6]` `[AC-AD-7]` `[FR-AD-8]`
//!
//! Each runs inside the demo enclosure's action pipeline, so the core's rules hold
//! around it: the target must be a row (or, for a computed read, a collection) the
//! persona may see, every outcome is decided once and audited, denials included, and
//! only the persona signed in acts. Admin's audit rows go to `admin.audit` (the
//! console's live ledger, the web's "LEDGER LINEAGE"), numbered `A-9111`, `A-9112`, …
//! and stamped from `14:02:13` in steps of 2 s, as the mock numbers them.
//!
//! | Verb | Target | Port of |
//! |---|---|---|
//! | `admin.approve` | `admin.approvals/<id>` | `approve(id, approver)`, the approver the persona |
//! | `admin.simulateConfig` | `admin.configCommits/<id>` | `simulateConfig` |
//! | `admin.commitConfig` | `admin.configCommits/<id>` | `commitConfig` |
//! | `admin.rollbackConfig` | `admin.configCommits/<id>` | `rollbackConfig` |
//! | `admin.searchLedger` (`query`) | `admin.ledger/*` | `searchLedger` |
//! | `admin.requestBreakGlass` (`reason`) | `admin.breakGlass/breakglass` | `requestBreakGlass` |
//!
//! Where the port knowingly differs from the mock (each for governance, or because the
//! apps have the personas the mock has not):
//! - **The approver is the persona signed in**, not the name the web screen passes
//!   (the item's routed approver), so separation of duties is tested against who
//!   actually decides; the approval then names that persona as its approver.
//! - **A decided approval is not decided again**: approving one that is no longer
//!   awaiting is a conflict, where the mock would approve it again.
//! - **Search covers what the persona may read**: the ledger's rows above the persona
//!   are left out of the answer.
//! - **The audit wording is the core's** (`applied · admin.approve`,
//!   `refused · admin.approve`), with the target as the resource, and a refused
//!   approval is audited as refused, where the mock writes `Denied · self-approval · …`.
//! - **The commit and the rollback name the persona** as the actor, where the mock
//!   always names its default subject.

use attest::json::Json;

use super::midgard::{in_collection, obj, param, set, text};
use crate::demo::audit::{SignatureStatus, AUDIT_TYPE};
use crate::demo::enclosure::{
    ActionRequest, Collection, DemoState, MockAuditClock, Outcome, StoredRow,
};
use crate::marking::{may_see, MarkingLevel, RowMarking};
use crate::source::Grant;

/// Admin's live ledger (`AuditRecord` rows): the mock's seed, then every action.
pub const AUDIT: &str = "admin.audit";
/// `AdminMock`'s audit numbering: ids after `A-9110` (`ledgerSeq`), stamps after
/// `14:02:11` (`clock`), each `1 + floor(180 / 100)` = 2 s later.
pub const ADMIN_CLOCK: MockAuditClock = MockAuditClock {
    prefix: "A-",
    seq: 9110,
    clock: 14 * 3600 + 2 * 60 + 11,
    step: 2,
};
/// The config version before any commit or rollback (`cfgVersion`).
pub const CONFIG_VERSION: u64 = 481;

const APPROVALS: &str = "admin.approvals";
const COMMITS: &str = "admin.configCommits";
const LEDGER: &str = "admin.ledger";
const BREAK_GLASS: &str = "admin.breakGlass";

/// `LEDGER_SEED`: the three records `auditStream` primes every subscriber with (the
/// web's "LEDGER LINEAGE · LIVE" before anything is done), in the mock's order, marked
/// as the ledger is. The sample extraction reads list calls only, and a stream is not
/// one, so they are seeded here, from the release's mock.
const LEDGER_SEED: [(&str, &str, &str, &str, &str); 3] = [
    (
        "A-9108",
        "k.osei",
        "Action · kms.rotate approved",
        "kms/enclosure-root-03",
        "14:02:11",
    ),
    (
        "A-9107",
        "m.silva",
        "Denied · fleet-release self-approval",
        "bifrost/stable",
        "11:40:02",
    ),
    (
        "B-0331",
        "k.osei",
        "Action · break-glass opened",
        "kms/enclosure-root-03",
        "05:12:47",
    ),
];

/// `admin.audit` as the mock's stream starts it: its seed records, unsigned (the seed
/// carries no signature the core could check), marked `restricted` as the ledger is.
/// The web's live table puts each record it receives on top, so the seed reads
/// last-primed first (`B-0331`, `A-9107`, `A-9108`); new rows go on top of it.
pub fn seeded_audit() -> Collection {
    let rows = LEDGER_SEED
        .iter()
        .rev()
        .map(|(id, actor, action, resource, ts)| {
            let status = SignatureStatus::Unsigned;
            StoredRow {
                id: (*id).to_string(),
                marking: RowMarking::new(MarkingLevel::Restricted, Vec::<String>::new()),
                item: obj(vec![
                    ("id", Json::Str((*id).into())),
                    ("actor", Json::Str((*actor).into())),
                    ("action", Json::Str((*action).into())),
                    ("resource", Json::Str((*resource).into())),
                    ("ts", Json::Str((*ts).into())),
                    ("pqSignature", Json::Str(status.as_str().into())),
                ]),
                signature: Some(status),
            }
        })
        .collect();
    Collection {
        type_name: AUDIT_TYPE.into(),
        rows,
    }
}

/// `approve(id, approver)`, the approver being the persona signed in. Separation of
/// duties: the requester can never approve their own request; that denial is
/// recorded on the item and audited, as every outcome is.
pub fn approve(state: &mut DemoState, grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(id) = in_collection(&req.target, APPROVALS) else {
        return Outcome::Refused("an approval is decided on admin.approvals/<id>".into());
    };
    let me = grant.persona().id.clone();
    let Some(row) = state.row_mut(APPROVALS, id) else {
        return Outcome::Refused(format!("unknown approval: {id}"));
    };
    let current = text(&row.item, "state").unwrap_or_default();
    if current != "awaiting" {
        return Outcome::Conflict(format!("approval {id} is already {current}"));
    }
    if text(&row.item, "requester").as_deref() == Some(me.as_str()) {
        set(&mut row.item, "state", Json::Str("denied".into()));
        set(
            &mut row.item,
            "deniedReason",
            Json::Str("DENIED · SELF-APPROVAL BLOCKED (AC-AD-4)".into()),
        );
        return Outcome::Refused(
            "separation-of-duties: requester cannot approve own request".into(),
        );
    }
    // Granted: step-up re-auth assumed at the moment of approval; a time-boxed
    // single-use token is issued.
    set(&mut row.item, "state", Json::Str("approved".into()));
    set(&mut row.item, "approver", Json::Str(me.clone()));
    Outcome::Applied(obj(vec![
        ("decision", Json::Str("granted".into())),
        (
            "reason",
            Json::Str(format!("granted by {me} · token 10m · single-use")),
        ),
    ]))
}

/// The plain-language summary every simulation gives (the mock's fixed text).
const SIMULATION_SUMMARY: &str = "Grants 14 users read on INTERNAL-marked maintenance data. \
    No marking exposure above clearance. Reversible. Blast radius: one dataset family.";

/// `simulateConfig(commitId)`: the diff's added lines as grants, its removed lines as
/// revokes, and the summary.
pub fn simulate_config(state: &mut DemoState, _grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(id) = in_collection(&req.target, COMMITS) else {
        return Outcome::Refused("a change is simulated on admin.configCommits/<id>".into());
    };
    let Some(row) = state.row(COMMITS, id) else {
        return Outcome::Refused(format!("unknown change: {id}"));
    };
    let diff: Vec<String> = row
        .item
        .get("diff")
        .and_then(Json::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|l| l.as_str().map(str::to_string))
        .collect();
    let lines = |prefix: &str| {
        Json::Arr(
            diff.iter()
                .filter_map(|l| l.strip_prefix(prefix))
                .map(|l| Json::Str(l.to_string()))
                .collect(),
        )
    };
    Outcome::Applied(obj(vec![
        ("grants", lines("+ ")),
        ("revokes", lines("- ")),
        ("summary", Json::Str(SIMULATION_SUMMARY.into())),
    ]))
}

/// Set the change's state and take the next config version (`v482`, `v483`, …).
fn version_change(state: &mut DemoState, req: &ActionRequest, to: &str) -> Result<String, Outcome> {
    let Some(id) = in_collection(&req.target, COMMITS) else {
        return Err(Outcome::Refused(
            "a change is committed or rolled back on admin.configCommits/<id>".into(),
        ));
    };
    let Some(row) = state.row_mut(COMMITS, id) else {
        return Err(Outcome::Refused(format!("unknown change: {id}")));
    };
    set(&mut row.item, "state", Json::Str(to.into()));
    Ok(format!(
        "v{}",
        state.next_count("admin.configVersion", CONFIG_VERSION)
    ))
}

/// `commitConfig(id)`: the change committed as the next version, signed and audited.
pub fn commit_config(state: &mut DemoState, _grant: &Grant, req: &ActionRequest) -> Outcome {
    match version_change(state, req, "committed") {
        Ok(version) => Outcome::Applied(obj(vec![
            ("version", Json::Str(version)),
            ("committed", Json::Bool(true)),
        ])),
        Err(refusal) => refusal,
    }
}

/// `rollbackConfig(id)`: the change rolled back as the next version.
pub fn rollback_config(state: &mut DemoState, _grant: &Grant, req: &ActionRequest) -> Outcome {
    match version_change(state, req, "rolled-back") {
        Ok(version) => Outcome::Applied(obj(vec![
            ("version", Json::Str(version)),
            ("rolledBack", Json::Bool(true)),
        ])),
        Err(refusal) => refusal,
    }
}

/// Whether ledger entry `e` meets one search token (`actor:`, `subsystem:`, `since:`,
/// or free text over every field), as the mock reads it.
fn meets(e: &Json, token: &str) -> bool {
    let (field, value) = match token.split_once(':') {
        // JavaScript's `split(':', 2)`: the text after a second colon is dropped.
        Some((f, rest)) => (f, rest.split(':').next().unwrap_or("")),
        None => ("", token),
    };
    let field_has = |key: &str| {
        text(e, key)
            .unwrap_or_default()
            .to_lowercase()
            .contains(value)
    };
    match field {
        "actor" => field_has("actor"),
        "subsystem" => field_has("subsystem"),
        // A relative-time token: accepted, as in the mock.
        "since" => true,
        _ => [
            "id",
            "actor",
            "action",
            "subsystem",
            "justification",
            "approver",
        ]
        .iter()
        .map(|k| text(e, k).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .contains(value),
    }
}

/// `searchLedger(query)`: the ledger's entries the persona may read that meet every
/// token of the query (all of them for an empty query), in the ledger's order.
pub fn search_ledger(state: &mut DemoState, grant: &Grant, req: &ActionRequest) -> Outcome {
    if req.target != format!("{LEDGER}/*") {
        return Outcome::Refused("a ledger search is on admin.ledger/*".into());
    }
    let q = param(req, "query").unwrap_or("").trim().to_lowercase();
    let tokens: Vec<&str> = q.split_whitespace().collect();
    let rows: Vec<Json> = state
        .collections
        .get(LEDGER)
        .map(|c| {
            c.rows
                .iter()
                .filter(|r| may_see(grant.clearance(), &r.marking))
                .filter(|r| tokens.iter().all(|t| meets(&r.item, t)))
                .map(|r| r.item.clone())
                .collect()
        })
        .unwrap_or_default();
    Outcome::Applied(Json::Arr(rows))
}

/// `requestBreakGlass(reason)`: idle or review → active, time-boxed, held by the
/// persona, with the written justification (or the mock's reminder that one is
/// mandatory).
pub fn request_break_glass(state: &mut DemoState, grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(id) = in_collection(&req.target, BREAK_GLASS) else {
        return Outcome::Refused("break-glass is requested on admin.breakGlass/breakglass".into());
    };
    let reason = param(req, "reason").unwrap_or("").trim().to_string();
    let me = grant.persona().id.clone();
    let Some(row) = state.row_mut(BREAK_GLASS, id) else {
        return Outcome::Refused("no such target".into());
    };
    let justification = if reason.is_empty() {
        "Emergency elevation requested — justification mandatory.".to_string()
    } else {
        reason
    };
    row.item = obj(vec![
        ("state", Json::Str("active".into())),
        ("holder", Json::Str(me)),
        (
            "scope",
            Json::Str("KMS · unwrap only · enclosure-root-03".into()),
        ),
        ("remaining", Json::Str("00:59:59".into())),
        ("justification", Json::Str(justification)),
    ]);
    Outcome::Applied(row.item.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, actor: &str, subsystem: &str) -> Json {
        obj(vec![
            ("id", Json::Str(id.into())),
            ("actor", Json::Str(actor.into())),
            ("action", Json::Str("kms.rotate approved".into())),
            ("subsystem", Json::Str(subsystem.into())),
            ("justification", Json::Str("scheduled rotation".into())),
            ("approver", Json::Str("a.novak".into())),
        ])
    }

    #[test]
    fn tokens_read_as_the_mock_reads_them() {
        let e = entry("A-9108", "k.osei", "KMS");
        assert!(meets(&e, "actor:k.osei"));
        assert!(!meets(&e, "actor:m.silva"));
        assert!(meets(&e, "subsystem:kms"));
        assert!(meets(&e, "since:24h"));
        assert!(meets(&e, "rotation"));
        assert!(meets(&e, "unknown:rotation"));
        assert!(!meets(&e, "break-glass"));
    }
}
