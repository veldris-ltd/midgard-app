//! Forge's actions in the sample mode, ported from the release's `ForgeMock`
//! (`sdk/ts/src/mock/forge.ts`), which is what the rc.3 web console runs on in mock
//! mode. `[FR-C-4]` `[AC-C-13]` `[FR-F-1]` `[FR-F-3]` `[FR-F-7]` `[NFR-AI-1]`
//!
//! Each runs inside the demo enclosure's action pipeline, so the core's rules hold
//! around it: the target must be a row the persona may see (a pipeline above the
//! persona is refused exactly as a missing one is, "no such target"), every outcome is
//! decided once and audited, refusals included, and only the persona signed in acts.
//! Forge's audit rows go to `forge.audit` (seeded with the mock's five records),
//! numbered `FRG-2044`, `FRG-2045`, … and stamped from `14:02:13` in steps of 2 s, as the
//! mock numbers them ([`FORGE_CLOCK`]).
//!
//! | Verb | Target | Port of |
//! |---|---|---|
//! | `forge.runPipeline` | `forge.pipelines/<id>` | `runPipeline(id)` |
//! | `forge.previewTransform` (`sql`) | `forge.transform/transform` | `previewTransform(sql)` |
//! | `forge.applyTransform` (`sql`, `humanGateToken`) | `forge.transform/transform` | `applyTransform(sql, humanGateToken)` |
//!
//! The mock's other methods are reads (`listPipelines`, `listConnectors`,
//! `listDatasets`, `listHealthChecks`, `listSchedules`, `getTransform`,
//! `listProjections`, `getAudit`), served from the collections, and `auditStream`,
//! whose synthetic records are not invented here. The web screens' other buttons (Add
//! connector, Rotate certificate, New branch, Time travel, Open in builder, New
//! expectation, Preview quarantine, Branch, Preview transform, New schedule, Sync now,
//! Open type in Midgard, …) have no handler in the web console either, and no action
//! here: a connector test, a schedule change or a projection sync is not something the
//! mock does.
//!
//! Where the port knowingly differs from the mock:
//! - **The audit wording is the core's** (`applied · forge.runPipeline` on
//!   `forge.pipelines/<id>`, where the mock writes `Build · <name>` on
//!   `dataset/<name>@<version>`): that wording is what this device's audit key checks
//!   before it signs the row. A refusal is audited too (the mock throws before it
//!   writes a record) and takes the next number.
//! - **The persona signed in is the actor**, where the mock always names its default
//!   subject (`j.benjamin`).
//! - **An unknown pipeline is refused** ("no such target"), where the mock builds its
//!   first pipeline instead.
//! - **The build's audit id comes with the action**, not in the result: the result is
//!   the mock's `ForgeBuildResult` (run id, version, duration, the six staged steps);
//!   likewise `applyTransform`'s result carries `version` and `applied`, its `auditId`
//!   being the action's audit id.
//! - Nothing in the collections changes (nor does the mock's): a pipeline built again
//!   names the same next version, and the transform's next version stays `v43`.

use attest::json::Json;

use super::midgard::{num, obj, param, text};
use crate::demo::enclosure::{ActionRequest, DemoState, MockAuditClock, Outcome};
use crate::source::Grant;

/// Forge's pipelines (`ForgePipeline` rows).
pub const PIPELINES: &str = "forge.pipelines";
/// Forge's transform (one `ForgeTransform` row, id `transform`).
pub const TRANSFORM: &str = "forge.transform";
/// The transform row's id.
pub const TRANSFORM_ID: &str = "transform";
/// Forge's audit (`AuditRecord` rows): the mock's seed, then every action.
pub const AUDIT: &str = "forge.audit";

/// `ForgeMock`'s audit numbering: `nextId` is `FRG-${2043 + seq}` with `seq` from 1, so
/// ids after `FRG-2043`; stamps after `14:02:11` (`clock`), each `1 + floor(160 / 100)`
/// = 2 s later (the mock's default 160 ms latency).
pub const FORGE_CLOCK: MockAuditClock = MockAuditClock {
    prefix: "FRG-",
    seq: 2043,
    clock: 14 * 3600 + 2 * 60 + 11,
    step: 2,
};

/// The run numbering: `RUN-${1600 + runSeq}`, `runSeq` from 1.
pub const RUN_SEED: u64 = 1600;

/// `ForgeHumanGateRequiredError`'s message for `subject`.
pub fn human_gate_required(subject: &str) -> String {
    format!("Human gate required to commit \"{subject}\"")
}

fn target_row<'a>(req: &'a ActionRequest, collection: &str) -> Option<&'a str> {
    match req.target.split_once('/') {
        Some((c, id)) if c == collection => Some(id),
        _ => None,
    }
}

fn s(v: &str) -> Json {
    Json::Str(v.to_string())
}

fn step(stage: &str, detail: &str, status: &str) -> Json {
    obj(vec![
        ("stage", s(stage)),
        ("detail", s(detail)),
        ("status", s(status)),
    ])
}

/// `runPipeline(id)`: the next version after the pipeline's current one, a run id
/// `RUN-1601`, `RUN-1602`, … and the mock's six staged steps, which the screens replay
/// one by one as the web console does.
pub fn run_pipeline(state: &mut DemoState, _grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(id) = target_row(req, PIPELINES) else {
        return Outcome::Refused("a pipeline is run on forge.pipelines/<id>".into());
    };
    let Some(row) = state.row(PIPELINES, id) else {
        return Outcome::Refused("no such target".into());
    };
    let current = text(&row.item, "currentVersion").unwrap_or_else(|| "v0".into());
    let n: u64 = current.trim_start_matches('v').parse().unwrap_or(0);
    let next = format!("v{}", n + 1);
    let run = state.next_count("forge.runSeq", RUN_SEED);
    let steps = Json::Arr(vec![
        step("resolve", "sources 2/2 · PQ-mTLS OK", "ok"),
        step("transform", "clean_flights · 6/6 steps", "ok"),
        step(
            "transform",
            "fuse_positions · expectations 11/12 watch",
            "warn",
        ),
        step("seal", "AES-256-GCM · ML-KEM-1024 wrap", "ok"),
        step("project", "12,408 objects → fabric", "ok"),
        step(
            "done",
            &format!("build {next} OK · 1m38s · lineage signed (ML-DSA)"),
            "ok",
        ),
    ]);
    Outcome::Applied(obj(vec![
        ("runId", Json::Str(format!("RUN-{run}"))),
        ("version", Json::Str(next)),
        ("durationLabel", s("1m38s")),
        ("steps", steps),
    ]))
}

/// The preview's summary line: the code's first 72 characters (trimmed), with "…" when
/// longer, or the mock's words for an empty one.
fn summary(sql: &str) -> String {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return "Preview of the proposed bounds filter".into();
    }
    let head: String = trimmed.chars().take(72).collect();
    let more = trimmed.chars().count() > 72;
    format!("Preview of: {head}{}", if more { "…" } else { "" })
}

/// `previewTransform(sql)`: the sandboxed diff (rows in and out, expectations, the three
/// hunks). Nothing is committed.
pub fn preview_transform(_state: &mut DemoState, _grant: &Grant, req: &ActionRequest) -> Outcome {
    if target_row(req, TRANSFORM) != Some(TRANSFORM_ID) {
        return Outcome::Refused("a transform is previewed on forge.transform/transform".into());
    }
    let sql = param(req, "sql").unwrap_or("");
    let hunk = |op: &str, t: &str| obj(vec![("op", s(op)), ("text", s(t))]);
    let diff = obj(vec![
        ("summary", Json::Str(summary(sql))),
        ("rowsIn", num(48_112)),
        ("rowsOut", num(47_924)),
        ("expectations", s("12/12")),
        ("durationLabel", s("4.1s")),
        (
            "hunks",
            Json::Arr(vec![
                hunk(
                    "~",
                    "fuse_positions: add df[df.alt_ft.between(0, 60000)] before join",
                ),
                hunk("-", "188 out-of-bounds rows dropped (alt_ft ≥ 60000)"),
                hunk("+", "expectation rows_within(0.15).of_7d_mean() now 12/12"),
            ]),
        ),
    ]);
    Outcome::Applied(obj(vec![("diff", diff)]))
}

/// `applyTransform(sql, humanGateToken)`: without a token refused with
/// `ForgeHumanGateRequiredError`'s words; with one, committed as `v43`. In the sample
/// mode the human gate is the operator's Authorise press itself: the token only shows
/// it happened, and no handler may ever send one automatically.
pub fn apply_transform(state: &mut DemoState, _grant: &Grant, req: &ActionRequest) -> Outcome {
    if target_row(req, TRANSFORM) != Some(TRANSFORM_ID) {
        return Outcome::Refused("a transform is committed on forge.transform/transform".into());
    }
    let subject = state
        .row(TRANSFORM, TRANSFORM_ID)
        .and_then(|r| text(&r.item, "id"))
        .unwrap_or_else(|| "fuse_positions".into());
    let token = param(req, "humanGateToken").filter(|v| !v.trim().is_empty());
    if token.is_none() {
        return Outcome::Refused(human_gate_required(&subject));
    }
    Outcome::Applied(obj(vec![
        ("version", s("v43")),
        ("applied", Json::Bool(true)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_cuts_at_72_characters_as_the_mock_does() {
        assert_eq!(summary("  "), "Preview of the proposed bounds filter");
        assert_eq!(summary(" abc "), "Preview of: abc");
        let long = "é".repeat(80);
        assert_eq!(summary(&long), format!("Preview of: {}…", "é".repeat(72)));
        assert_eq!(
            summary(&"x".repeat(72)),
            format!("Preview of: {}", "x".repeat(72))
        );
    }
}
