//! The sample mode: the signed release's own built-in sample data, as its web consoles
//! show it in mock mode. `[FR-C-5]` `[AC-C-13]` `[FR-C-4]`
//!
//! Owner decision (22 Nov 2026): the apps show exactly the sample data the
//! `v0.1.0-rc.3` web consoles ship with (`sdk/ts/src/mock/`), under the same
//! "Demonstration data" banner, with no feeds and no network. The data is
//! `clients/demo/sample/<release>/sample.json`, extracted from those mocks by
//! `clients/demo/tools/extract-sample.sh` (whose `--check` is the drift check against
//! the tag) and compiled into the core, so the app reads no file for it.
//!
//! It is served by the demo enclosure's machinery ([`DemoEnclosure`]), so everything
//! the core enforces on the demo dataset holds here too: strict loading, the
//! local-suite data guard, every row's marking, clearance filtering (rows above a
//! persona are delivered withheld), action objects decided once per persona and
//! audited. Two things differ from the signed dataset: no audit row carries a
//! build-time signature (there is no file to sign; every audit row is delivered
//! unsigned, and only the device's own rows show as signed), and the actions are the
//! web mocks', numbered and stamped as the mocks number and stamp their audit rows.
//!
//! Actions, as in the web mocks:
//! - `heimdall.authoriseWorkflow` on `heimdall.workflows/<id>` (param
//!   `humanGateToken`): a human-gated workflow without a token is refused ("Human gate
//!   required for action \"<title>\""); otherwise it is applied. Its audit row is
//!   numbered `D-4473`, `D-4474`, … and stamped from `14:39:04` in steps of 2 s, as
//!   `HeimdallMock` does. The row keeps the core's wording (`applied ·
//!   heimdall.authoriseWorkflow` on `heimdall.workflows/<id>`, where the mock writes
//!   `Authorise · <title>` on `workflow/<id>`): that wording is what this device's
//!   audit key checks before it signs the row, so the row shows as signed. The
//!   workflow row itself does not change (nor does the mock's). A refusal is audited
//!   too and takes the next number in the same sequence (so a refused attempt moves
//!   the next applied row to `D-4474`), as every action the enclosure decides is.
//!
//! - Midgard's, ported from `MockGateway` (`crate::demo::actions::midgard`, which lists
//!   them and where they knowingly differ): the dual-control object action
//!   (`midgard.requestApproval`, `midgard.decideApproval` by a second persona,
//!   `midgard.applyAction` with that approval as the human-gate token), and the
//!   computed reads `midgard.query`, `midgard.searchAround` and
//!   `midgard.simulatePolicy`, each audited as the mock audits them. Their rows are
//!   numbered `AUD-1043`, `AUD-1044`, … from `14:32:10`, as `MockGateway` numbers
//!   them, and approvals live in the runtime collection `midgard.approvals`.
//!
//! - Admin's, ported from `AdminMock` (`crate::demo::actions::admin`, which lists them
//!   and where they knowingly differ): the dual-control decision `admin.approve` (the
//!   approver the persona signed in; self-approval refused and recorded), the config
//!   lifecycle `admin.simulateConfig`, `admin.commitConfig` and `admin.rollbackConfig`,
//!   the ledger search `admin.searchLedger` and `admin.requestBreakGlass`. Their rows go
//!   to the runtime collection `admin.audit`, seeded with the mock stream's three
//!   records and numbered `A-9111`, `A-9112`, … from `14:02:13`, as `AdminMock` does.
//!
//! The other consoles' actions come with their phases (Phases E to H, Task 2); until
//! then an action on them is refused as unknown ("no such action").

use attest::json::{parse, Json};

use crate::demo::actions::{admin, midgard};
use crate::demo::enclosure::{
    ActionRequest, Collection, DemoEnclosure, DemoState, MockAuditClock, Outcome,
};
use crate::source::{DataSource, Delivered, Grant, Persona, Seal, SourceError, VerifiedDigest};
use crate::CORE_VERSION;

/// The release whose mocks the sample data is.
pub const SAMPLE_RELEASE: &str = "0.1.0-rc.3";

/// The sample body, as extracted (canonical JSON).
pub const SAMPLE_JSON: &str = include_str!("../../demo/sample/0.1.0-rc.3/sample.json");

/// Heimdall's audit numbering in `HeimdallMock`: ids after `D-4472`, stamps after
/// `14:39:02`, each `1 + floor(180 / 100)` = 2 s later (the console's default 180 ms
/// latency).
pub const HEIMDALL_CLOCK: MockAuditClock = MockAuditClock {
    prefix: "D-",
    seq: 4472,
    clock: 14 * 3600 + 39 * 60 + 2,
    step: 2,
};

/// The source over the compiled-in sample data.
pub struct SampleSource {
    inner: DemoEnclosure,
}

impl SampleSource {
    /// A fresh source: the sample data as extracted, online, with no action taken.
    /// Fails only if the compiled-in body does not load (a build that changed it
    /// without passing the checks), never silently.
    pub fn new() -> Result<Self, SourceError> {
        if CORE_VERSION != SAMPLE_RELEASE {
            return Err(SourceError::Malformed(format!(
                "the sample data is for release {SAMPLE_RELEASE}, this core is {CORE_VERSION}"
            )));
        }
        let body =
            parse(SAMPLE_JSON).map_err(|e| SourceError::Malformed(format!("sample: {e}")))?;
        let mut inner = DemoEnclosure::load_sample(SAMPLE_RELEASE, &body)?;
        inner.state_mut().set_mock_clock("heimdall", HEIMDALL_CLOCK);
        inner
            .state_mut()
            .set_mock_clock("midgard", midgard::MIDGARD_CLOCK);
        // Approval requests are made at run time; the collection starts empty.
        inner.state_mut().collections.insert(
            midgard::APPROVALS.to_string(),
            Collection {
                type_name: midgard::APPROVAL_TYPE.into(),
                rows: Vec::new(),
            },
        );
        inner
            .state_mut()
            .set_mock_clock("admin", admin::ADMIN_CLOCK);
        // Admin's live ledger starts as the mock's stream does: its three seed records.
        inner
            .state_mut()
            .collections
            .insert(admin::AUDIT.to_string(), admin::seeded_audit());
        inner.register("heimdall.authoriseWorkflow", authorise_workflow);
        inner.register("midgard.requestApproval", midgard::request_approval);
        inner.register("midgard.decideApproval", midgard::decide_approval);
        inner.register("midgard.applyAction", midgard::apply_action);
        inner.register("midgard.query", midgard::query);
        inner.register("midgard.searchAround", midgard::search_around);
        inner.register("midgard.simulatePolicy", midgard::simulate_policy);
        inner.register("admin.approve", admin::approve);
        inner.register("admin.simulateConfig", admin::simulate_config);
        inner.register("admin.commitConfig", admin::commit_config);
        inner.register("admin.rollbackConfig", admin::rollback_config);
        inner.register("admin.searchLedger", admin::search_ledger);
        inner.register("admin.requestBreakGlass", admin::request_break_glass);
        Ok(Self { inner })
    }

    /// [`SampleSource::new`] reading the time from `now` (seconds since the epoch)
    /// rather than the system clock: for tests of an approval's expiry.
    pub fn with_clock(now: fn() -> u64) -> Result<Self, SourceError> {
        let mut s = Self::new()?;
        s.inner.state_mut().set_now(now);
        Ok(s)
    }
}

fn str_field<'a>(item: &'a Json, key: &str) -> Option<&'a str> {
    item.get(key).and_then(Json::as_str)
}

/// `HeimdallMock.authoriseWorkflow(id, humanGateToken)`.
fn authorise_workflow(state: &mut DemoState, _grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(("heimdall.workflows", id)) = req.target.split_once('/') else {
        return Outcome::Refused("a workflow is authorised on heimdall.workflows/<id>".into());
    };
    let Some(row) = state.row("heimdall.workflows", id) else {
        return Outcome::Refused("no such target".into());
    };
    let title = str_field(&row.item, "title").unwrap_or(id).to_string();
    let gated = matches!(row.item.get("humanGated"), Some(Json::Bool(true)));
    let token = req
        .params
        .iter()
        .find(|(k, _)| k == "humanGateToken")
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty());
    // In sample mode the human gate is the operator's Authorise press itself: the token
    // only shows it happened, and no handler may ever send one automatically.
    if gated && token.is_none() {
        return Outcome::Refused(format!("Human gate required for action \"{title}\""));
    }
    Outcome::Applied(Json::Obj(vec![("applied".into(), Json::Bool(true))]))
}

impl DataSource for SampleSource {
    fn personas(&self) -> Vec<Persona> {
        self.inner.personas()
    }

    fn collections(&self) -> Vec<(String, String)> {
        self.inner.collections()
    }

    fn grant(&self, persona_id: &str) -> Result<Grant, SourceError> {
        self.inner.grant(persona_id)
    }

    fn fetch(&self, grant: &Grant, collection: &str) -> Result<Vec<Delivered>, SourceError> {
        self.inner.fetch(grant, collection)
    }

    fn exchange(&mut self, grant: &Grant, ops: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, SourceError> {
        self.inner.exchange(grant, ops)
    }

    fn online(&self) -> bool {
        self.inner.online()
    }

    fn set_online(&mut self, online: bool) {
        self.inner.set_online(online);
    }

    /// No sample audit row was checked in the core, so this is always `None`; passed
    /// through all the same, so the seal reaches the only code that could answer.
    fn in_core_verified(
        &self,
        seal: &Seal,
        grant: &Grant,
        collection: &str,
        id: &str,
    ) -> Option<VerifiedDigest> {
        self.inner.in_core_verified(seal, grant, collection, id)
    }
}
