//! Heimdall's actions in the sample mode, ported from the release's `HeimdallMock`
//! (`sdk/ts/src/mock/heimdall.ts`), which is what the rc.3 web console runs on in mock
//! mode. `[FR-C-4]` `[AC-C-13]` `[FR-H-4]` `[FR-H-5]` `[NFR-AI-1]`
//!
//! Each runs inside the demo enclosure's action pipeline, so the core's rules hold
//! around it: the target must be a row the persona may see (a workflow above the
//! persona is refused exactly as a missing one is, "no such target"), every outcome is
//! decided once and audited, refusals included, and only the persona signed in acts.
//! Heimdall's audit rows go to `heimdall.audit` (the console's decision lineage, seeded
//! with the mock's five records), numbered `D-4473`, `D-4474`, … and stamped from
//! `14:39:04` in steps of 2 s, as the mock numbers them ([`HEIMDALL_CLOCK`]).
//!
//! | Verb | Target | Port of |
//! |---|---|---|
//! | `heimdall.authoriseWorkflow` (`humanGateToken`) | `heimdall.workflows/<id>` | `authoriseWorkflow(id, humanGateToken)` |
//!
//! The mock's other methods are reads (`getPicture`, `listFeeds`, `listEntities`,
//! `listSyncSites`, `listWorkflows`, `listDecisions`, `getAudit`), served from the
//! collections, and `auditStream`, whose synthetic records are not invented here (see
//! below). The web screens' other buttons (Register feed, Re-key bridge, Cut picture
//! snapshot, Export for review, Replay picture) have no handler in the web console
//! either, and no action here.
//!
//! Where the port knowingly differs from the mock:
//! - **The audit wording is the core's** (`applied · heimdall.authoriseWorkflow` on
//!   `heimdall.workflows/<id>`, where the mock writes `Authorise · <title>` on
//!   `workflow/<id>`): that wording is what this device's audit key checks before it
//!   signs the row, so the row shows as signed. A refusal is audited too (the mock
//!   throws before it writes a record) and takes the next number.
//! - **The persona signed in is the actor**, where the mock always names its default
//!   subject (`k.osei`).
//! - **No live tail is synthesised.** The mock's `auditStream` adds a record from a
//!   rotating pool every 4.2 s; the apps' lineage tail shows `heimdall.audit` as it is
//!   (the seed and what this device's actions add), re-read while the screen is open.
//! - The workflow row itself does not change (nor does the mock's), so a workflow can
//!   be authorised again, each time audited.

use attest::json::Json;

use super::midgard::{obj, param, text};
use crate::demo::enclosure::{ActionRequest, DemoState, MockAuditClock, Outcome};
use crate::source::Grant;

/// Heimdall's workflows (`HeimdallWorkflow` rows).
pub const WORKFLOWS: &str = "heimdall.workflows";
/// Heimdall's decision lineage (`AuditRecord` rows): the mock's seed, then every action.
pub const AUDIT: &str = "heimdall.audit";

/// `HeimdallMock`'s audit numbering: ids after `D-4472` (`seq`), stamps after
/// `14:39:02` (`clock`), each `1 + floor(180 / 100)` = 2 s later (the console's default
/// 180 ms latency).
pub const HEIMDALL_CLOCK: MockAuditClock = MockAuditClock {
    prefix: "D-",
    seq: 4472,
    clock: 14 * 3600 + 39 * 60 + 2,
    step: 2,
};

/// `HumanGateRequiredError`'s message for an action titled `title`.
pub fn human_gate_required(title: &str) -> String {
    format!("Human gate required for action \"{title}\"")
}

/// `authoriseWorkflow(id, humanGateToken)`: a human-gated workflow without a token is
/// refused with `HumanGateRequiredError`'s words; otherwise it is applied (`applied:
/// true`, the audit id coming with the action). In the sample mode the human gate is
/// the operator's Authorise press itself: the token only shows it happened, and no
/// handler may ever send one automatically.
pub fn authorise_workflow(state: &mut DemoState, _grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some((WORKFLOWS, id)) = req.target.split_once('/') else {
        return Outcome::Refused("a workflow is authorised on heimdall.workflows/<id>".into());
    };
    let Some(row) = state.row(WORKFLOWS, id) else {
        return Outcome::Refused("no such target".into());
    };
    let title = text(&row.item, "title").unwrap_or_else(|| id.to_string());
    let gated = matches!(row.item.get("humanGated"), Some(Json::Bool(true)));
    let token = param(req, "humanGateToken").filter(|v| !v.trim().is_empty());
    if gated && token.is_none() {
        return Outcome::Refused(human_gate_required(&title));
    }
    Outcome::Applied(obj(vec![("applied", Json::Bool(true))]))
}
