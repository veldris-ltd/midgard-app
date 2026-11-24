//! Bifrost's actions in the sample mode, ported from the release's `BifrostMock`
//! (`sdk/ts/src/mock/bifrost.ts`), which is what the rc.3 web console runs on in mock
//! mode. `[FR-C-4]` `[AC-C-13]` `[FR-B-2]` `[FR-B-5]`
//!
//! Each runs inside the demo enclosure's action pipeline, so the core's rules hold
//! around it: the target must be a row the persona may see (a release or plan above the
//! persona is refused exactly as a missing one is, "no such target"), every outcome is
//! decided once and audited, refusals included, at the target's marking, and only the
//! persona signed in acts. Bifrost's audit rows go to `bifrost.audit` (seeded with the
//! mock's five records, `AUD-B-1042` to `AUD-B-1038`), numbered `AUD-B-1044`,
//! `AUD-B-1045`, … and stamped from `14:32:10` in steps of 2 s, as the mock numbers them
//! ([`BIFROST_CLOCK`]: the mock's `nextId` skips `AUD-B-1043`).
//!
//! | Verb | Target | Port of |
//! |---|---|---|
//! | `bifrost.promoteRelease` (`ring`) | `bifrost.releases/<id>` | `promoteRelease(releaseId, ring)` |
//! | `bifrost.rollback` | `bifrost.rollbacks/<id>` | `rollback(id)` |
//!
//! Both need the role `release-manager` (the mock's policy context holds it; of the
//! sample personas only A. Holt does). The mock's other methods are reads
//! (`listReleases`, `listRings`, `listArtefacts`, `listChannels`, `listBundles`,
//! `listFleet`, `listRollbacks`, `getAudit`), served from the collections, and
//! `auditStream`, whose synthetic records are not invented here. The web screens' other
//! buttons (Register artefact, New channel, Cut new bundle, Reconcile all, Revert to
//! desired, Adopt, Prepare rollback plan, Open release console, the channel select) have
//! no handler in the web console either, and no action here; the Rollback centre's Run
//! drill is a canned log the web plays on its own, not a gateway call.
//!
//! The mock's gates, kept with its words:
//! - **Air-gap rings advance by signed courier bundle**, not online promotion: a
//!   promotion to a ring with `offlineBundle` is refused (fail closed, nothing rolls),
//!   with the mock's own record as the reason — `Promote · <release> → ring air-gap
//!   (bundle required)`.
//! - **A promotion step** advances the ring by `max(1, ceil(total × 15 %))`
//!   installations up to its total; the health gate is WATCH while rolling and PASS once
//!   the ring is fully rolled (`progress` the rounded share, as `Math.round` gives it).
//!   The ring's `done` is kept, so the rings read back advanced, as the mock's do.
//! - **A rollback runs only while its plan is armed or available** (`secure` or
//!   `healthy`), the web screen's health gate, refused otherwise in its words ("Rollback
//!   held by gate").
//!
//! Where the port knowingly differs from the mock:
//! - **The audit wording is the core's** (`applied · bifrost.promoteRelease` on
//!   `bifrost.releases/<id>`): that wording is what this device's audit key checks before
//!   it signs the row. The mock's own record wording comes with the answer, as `step`
//!   (`Promote · REL-2050 → ring broad (60%)`, `Rollback · midgard-suite 2.5.0 → 2.4.1`).
//! - **The air-gap promotion is a refusal** (audited as refused), where the mock answers
//!   with `healthGate: 'fail'` and the ring's unchanged progress.
//! - **The role is checked**: a persona without `release-manager` is refused ("Role
//!   release-manager required to promote REL-2050"); the mock has one policy context and
//!   checks nothing. **The persona signed in is the actor**, where the mock always names
//!   `j.benjamin`.
//! - **An unknown ring or plan is refused**, where the mock records a promotion to
//!   `ring/<stage>` at 0 % or a rollback of the bare id.
//! - **The audit id comes with the action**, not in the result.

use attest::json::Json;

use super::midgard::{num, obj, param, set, text};
use crate::demo::enclosure::{ActionRequest, DemoState, MockAuditClock, Outcome};
use crate::source::Grant;

/// Bifrost's releases (`BifrostRelease` rows).
pub const RELEASES: &str = "bifrost.releases";
/// Bifrost's deployment rings (`BifrostRing` rows).
pub const RINGS: &str = "bifrost.rings";
/// Bifrost's standing rollback plans (`BifrostRollback` rows).
pub const ROLLBACKS: &str = "bifrost.rollbacks";
/// Bifrost's audit (`AuditRecord` rows): the mock's seed, then every action.
pub const AUDIT: &str = "bifrost.audit";

/// The role a promotion or a rollback needs (the mock's policy context's).
pub const RELEASE_MANAGER: &str = "release-manager";

/// `BifrostMock`'s audit numbering: `nextId` is `AUD-B-${1043 + seq}` with `seq` from 1,
/// so ids after `AUD-B-1043`; stamps after `14:32:08` (`clock`), each
/// `1 + floor(180 / 100)` = 2 s later (the mock's default 180 ms latency).
pub const BIFROST_CLOCK: MockAuditClock = MockAuditClock {
    prefix: "AUD-B-",
    seq: 1043,
    clock: 14 * 3600 + 32 * 60 + 8,
    step: 2,
};

/// The refusal for a persona without the release-manager role.
pub fn role_required(what: &str) -> String {
    format!("Role {RELEASE_MANAGER} required to {what}")
}

/// The mock's record for a promotion to an air-gap ring: the refusal's words.
pub fn bundle_required(release: &str, ring: &str) -> String {
    format!("Promote · {release} → ring {ring} (bundle required)")
}

/// The web's health gate on a plan that is not armed or available.
pub const ROLLBACK_HELD: &str = "Rollback held by gate";

fn target_row<'a>(req: &'a ActionRequest, collection: &str) -> Option<&'a str> {
    match req.target.split_once('/') {
        Some((c, id)) if c == collection => Some(id),
        _ => None,
    }
}

fn s(v: &str) -> Json {
    Json::Str(v.to_string())
}

fn count(item: &Json, key: &str) -> u64 {
    match item.get(key) {
        Some(Json::Num(n)) => n.parse::<f64>().map_or(0, |v| v.max(0.0) as u64),
        _ => 0,
    }
}

fn manager(grant: &Grant) -> bool {
    grant.persona().roles.iter().any(|r| r == RELEASE_MANAGER)
}

/// `Math.round((done / total) * 100)`, exact in integers (half rounds up).
pub fn progress(done: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    (done * 200 + total) / (2 * total)
}

/// One promotion step: `max(1, ceil(total × 0.15))`.
pub fn step_size(total: u64) -> u64 {
    // ceil(total * 15 / 100) in integers.
    ((total * 15).div_ceil(100)).max(1)
}

/// `promoteRelease(releaseId, ring)`: advance the ring of stage `ring` one health-gated
/// step for the release. Refused for a persona without the role, an unknown ring, and
/// (fail closed) an air-gap ring, which advances by courier bundle only.
pub fn promote_release(state: &mut DemoState, grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(release) = target_row(req, RELEASES) else {
        return Outcome::Refused("a release is promoted on bifrost.releases/<id>".into());
    };
    if state.row(RELEASES, release).is_none() {
        return Outcome::Refused("no such target".into());
    }
    if !manager(grant) {
        return Outcome::Refused(role_required(&format!("promote {release}")));
    }
    let stage = param(req, "ring").unwrap_or("").to_string();
    let ring = state.collections.get(RINGS).and_then(|c| {
        c.rows
            .iter()
            .find(|r| text(&r.item, "stage").as_deref() == Some(stage.as_str()))
            .map(|r| r.id.clone())
    });
    let Some(ring_id) = ring else {
        return Outcome::Refused(format!("no such ring: {stage}"));
    };
    let row = state.row_mut(RINGS, &ring_id).expect("found above");
    if row.item.get("offlineBundle") == Some(&Json::Bool(true)) {
        return Outcome::Refused(bundle_required(release, &stage));
    }
    let total = count(&row.item, "total");
    let mut done = count(&row.item, "done");
    if done < total {
        done = total.min(done + step_size(total));
        set(&mut row.item, "done", num(done));
    }
    let pct = progress(done, total);
    let gate = if pct >= 100 { "pass" } else { "watch" };
    Outcome::Applied(obj(vec![
        ("ring", Json::Str(stage.clone())),
        ("progress", num(pct)),
        ("healthGate", s(gate)),
        (
            "step",
            Json::Str(format!("Promote · {release} → ring {stage} ({pct}%)")),
        ),
        ("resource", Json::Str(ring_id)),
    ]))
}

/// `rollback(id)`: execute a standing plan, while it is armed or available. Refused for
/// a persona without the role and for a plan whose gate is held.
pub fn rollback(state: &mut DemoState, grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(id) = target_row(req, ROLLBACKS) else {
        return Outcome::Refused("a rollback is run on bifrost.rollbacks/<id>".into());
    };
    let Some(row) = state.row(ROLLBACKS, id) else {
        return Outcome::Refused("no such target".into());
    };
    if !manager(grant) {
        return Outcome::Refused(role_required(&format!("roll back {id}")));
    }
    let plan_state = text(&row.item, "state").unwrap_or_default();
    if plan_state != "secure" && plan_state != "healthy" {
        return Outcome::Refused(ROLLBACK_HELD.into());
    }
    let scope = text(&row.item, "scope").unwrap_or_else(|| id.to_string());
    Outcome::Applied(obj(vec![
        ("plan", s(id)),
        ("scope", Json::Str(scope.clone())),
        ("step", Json::Str(format!("Rollback · {scope}"))),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_and_progress_round_as_the_mock_does() {
        assert_eq!(step_size(48), 8); // ceil(7.2)
        assert_eq!(step_size(4), 1); // ceil(0.6)
        assert_eq!(step_size(20), 3);
        assert_eq!(step_size(0), 1);
        assert_eq!(progress(29, 48), 60); // 60.41…
        assert_eq!(progress(37, 48), 77); // 77.08…
        assert_eq!(progress(45, 48), 94); // 93.75
        assert_eq!(progress(1, 8), 13); // 12.5 rounds up
        assert_eq!(progress(0, 9), 0);
        assert_eq!(progress(0, 0), 0);
    }
}
