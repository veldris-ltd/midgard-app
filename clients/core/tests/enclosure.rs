//! The demo enclosure serves rows filtered by the grant, refuses everything while
//! offline, runs each queued action exactly once, loads only a verified, guarded,
//! strictly marked dataset, and never shows an audit row as signed without a real
//! check. [AC-C-13] [FR-C-4] [FR-C-5] [NFR-PQ-3]

use std::fs;
use std::path::PathBuf;

use attest::json::{parse, Json};
use attest::TrustAnchors;
use midgard_client_core::demo::audit::SignatureStatus;
use midgard_client_core::demo::canon;
use midgard_client_core::demo::dataset::DatasetError;
use midgard_client_core::demo::enclosure::{DemoEnclosure, Outcome};
use midgard_client_core::marking::MarkingLevel;
use midgard_client_core::source::{action, DataSource, Delivered, SourceError};
use midgard_client_core::sync::{Replica, SyncOp};
use midgard_client_core::CORE_VERSION;

const BODY: &str = r#"{"collections":{
  "midgard.audit":{"type":"AuditRecord","rows":[
    {"id":"AUD-1","marking":{"level":"internal","compartments":[]},"item":{"id":"AUD-1","actor":"k.rao","action":"Query","resource":"r","ts":"14:00:00","pqSignature":"ML-DSA ✓"}},
    {"id":"AUD-2","marking":{"level":"confidential","compartments":["OPS-N"]},"item":{"id":"AUD-2","actor":"x","action":"y","resource":"z","ts":"14:00:01","pqSignature":"ML-DSA ✓"}}]}},
 "personas":[
  {"id":"k.rao","display":"K. Rao","role":"analyst","clearance":"restricted","compartments":[],"roles":["analyst"]},
  {"id":"k.osei","display":"K. Osei","role":"approver","clearance":"confidential","compartments":["OPS-N","SIG"],"roles":["approver"]}],
 "format":"midgard-demo-dataset","version":1,"release":"test","inputs":{}}"#;

fn from(body: &str) -> Result<DemoEnclosure, SourceError> {
    DemoEnclosure::from_body("test", &parse(body).unwrap(), &TrustAnchors::new())
}

fn enclosure() -> DemoEnclosure {
    from(BODY).unwrap()
}

fn ping(client: &mut Replica, id: &str, by: &str) -> Vec<Vec<u8>> {
    for (f, v) in [
        (action::VERB, "demo.ping"),
        (action::TARGET, "midgard.audit/AUD-1"),
        (action::PARAMS, "{}"),
        (action::BY, by),
    ] {
        client.set(id, f, v.as_bytes().to_vec());
    }
    client.drain_pending().iter().map(SyncOp::encode).collect()
}

fn merge(client: &mut Replica, back: &[Vec<u8>]) {
    let ops: Vec<SyncOp> = back.iter().map(|b| SyncOp::decode(b).unwrap()).collect();
    client.merge_all(ops.iter());
}

#[test]
fn rows_above_the_grant_are_withheld_at_the_source() {
    let e = enclosure();
    let analyst = e.grant("k.rao").unwrap();
    let rows = e.fetch(&analyst, "midgard.audit").unwrap();
    assert!(matches!(&rows[0], Delivered::Row { id, .. } if id == "AUD-1"));
    assert_eq!(
        rows[1],
        Delivered::Withheld {
            level: MarkingLevel::Confidential
        }
    );
    let shown = format!("{rows:?}");
    assert!(
        !shown.contains("AUD-2") && !shown.contains("OPS-N"),
        "{shown}"
    );
    let approver = e.grant("k.osei").unwrap();
    assert!(matches!(
        &e.fetch(&approver, "midgard.audit").unwrap()[1],
        Delivered::Row { .. }
    ));
    assert_eq!(
        e.grant("nobody").unwrap_err(),
        SourceError::UnknownPersona("nobody".into())
    );
    assert_eq!(
        e.fetch(&analyst, "midgard.nothing").unwrap_err(),
        SourceError::UnknownCollection("midgard.nothing".into())
    );
}

#[test]
fn a_grant_from_another_source_is_not_honoured() {
    // A grant names a persona; the source re-reads that persona's clearance from its
    // own table, so a grant made by another source (with other personas) is refused.
    let other = from(&BODY.replace(r#""id":"k.rao""#, r#""id":"k.other""#)).unwrap();
    let foreign = other.grant("k.other").unwrap();
    let e = enclosure();
    assert_eq!(
        e.fetch(&foreign, "midgard.audit").unwrap_err(),
        SourceError::UnknownPersona("k.other".into())
    );
}

#[test]
fn offline_refuses_fetch_and_exchange() {
    let mut e = enclosure();
    let g = e.grant("k.rao").unwrap();
    e.set_online(false);
    assert!(!e.online());
    assert_eq!(
        e.fetch(&g, "midgard.audit").unwrap_err(),
        SourceError::Offline
    );
    assert_eq!(e.exchange(&g, &[]).unwrap_err(), SourceError::Offline);
    e.set_online(true);
    assert!(e.fetch(&g, "midgard.audit").is_ok());
}

#[test]
fn an_action_runs_once_and_is_audited() {
    let mut e = enclosure();
    let g = e.grant("k.rao").unwrap();
    let mut client = Replica::new("dev-1");
    let ops = ping(&mut client, "action:dev-1-1", "k.rao");
    let back = e.exchange(&g, &ops).unwrap();
    merge(&mut client, &back);
    assert_eq!(
        client.get("action:dev-1-1", action::STATUS),
        Some(&b"applied"[..])
    );
    let audit_id = client
        .get("action:dev-1-1", action::AUDIT)
        .unwrap()
        .to_vec();
    // The same ops again: decided already. The same decision comes back (same
    // versions, so merging it again changes nothing) and nothing runs twice.
    assert_eq!(e.exchange(&g, &ops).unwrap(), back);
    let osei = e.grant("k.osei").unwrap();
    let audited = e.fetch(&osei, "midgard.audit").unwrap();
    assert_eq!(audited.len(), 3);
    // The new row is first, and not shown as signed: no device key exists yet.
    match &audited[0] {
        Delivered::Row {
            id,
            body,
            signature,
            ..
        } => {
            assert_eq!(id.as_bytes(), &audit_id[..]);
            assert_eq!(*signature, Some(SignatureStatus::Unsigned));
            let item = parse(std::str::from_utf8(body).unwrap()).unwrap();
            assert_eq!(item.get("pqSignature").and_then(Json::as_str), Some("none"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_action_for_another_persona_or_an_unknown_verb_is_refused() {
    let mut e = enclosure();
    let g = e.grant("k.rao").unwrap();
    let mut client = Replica::new("dev-1");
    let back = e
        .exchange(&g, &ping(&mut client, "action:dev-1-1", "k.osei"))
        .unwrap();
    merge(&mut client, &back);
    assert_eq!(
        client.get("action:dev-1-1", action::STATUS),
        Some(&b"refused"[..])
    );
    let mut ops = ping(&mut client, "action:dev-1-2", "k.rao");
    client.set("action:dev-1-2", action::VERB, b"demo.nothing".to_vec());
    ops.extend(client.drain_pending().iter().map(SyncOp::encode));
    let back = e.exchange(&g, &ops).unwrap();
    merge(&mut client, &back);
    assert_eq!(
        client.get("action:dev-1-2", action::STATUS),
        Some(&b"refused"[..])
    );
    assert!(client
        .get("action:dev-1-2", action::REASON)
        .is_some_and(|r| r.starts_with(b"no such action")));
}

#[test]
fn a_client_cannot_decide_its_own_action() {
    // The client writes the source's fields itself: they are not taken, and the
    // action still runs once, with the source's own status.
    let mut e = enclosure();
    let g = e.grant("k.rao").unwrap();
    let mut client = Replica::new("dev-1");
    client.set("action:dev-1-1", action::STATUS, b"applied".to_vec());
    client.set("action:dev-1-1", action::AUDIT, b"AUD-FAKE".to_vec());
    let mut ops: Vec<Vec<u8>> = client.drain_pending().iter().map(SyncOp::encode).collect();
    ops.extend(ping(&mut client, "action:dev-1-1", "k.osei"));
    let back = e.exchange(&g, &ops).unwrap();
    merge(&mut client, &back);
    assert_eq!(
        client.get("action:dev-1-1", action::STATUS),
        Some(&b"refused"[..])
    );
    assert_ne!(
        client.get("action:dev-1-1", action::AUDIT),
        Some(&b"AUD-FAKE"[..])
    );
}

#[test]
fn a_malformed_op_is_refused_and_nothing_is_merged() {
    let mut e = enclosure();
    let g = e.grant("k.rao").unwrap();
    let mut client = Replica::new("dev-1");
    let mut ops = ping(&mut client, "action:dev-1-1", "k.rao");
    ops.push(b"not an op".to_vec());
    assert!(matches!(
        e.exchange(&g, &ops),
        Err(SourceError::Malformed(_))
    ));
    let osei = e.grant("k.osei").unwrap();
    assert_eq!(e.fetch(&osei, "midgard.audit").unwrap().len(), 2);
}

#[test]
fn the_audit_row_for_an_action_on_a_marked_row_carries_that_marking() {
    // An action on AUD-2 (confidential // OPS-N) is audited at that marking, so the
    // audit row does not show a restricted reader the id of a row it may not see.
    let mut e = enclosure();
    let g = e.grant("k.osei").unwrap();
    let mut client = Replica::new("dev-1");
    let mut ops = ping(&mut client, "action:dev-1-1", "k.osei");
    client.set(
        "action:dev-1-1",
        action::TARGET,
        b"midgard.audit/AUD-2".to_vec(),
    );
    ops.extend(client.drain_pending().iter().map(SyncOp::encode));
    e.exchange(&g, &ops).unwrap();
    let rao = e.grant("k.rao").unwrap();
    let rows = e.fetch(&rao, "midgard.audit").unwrap();
    assert_eq!(
        rows[0],
        Delivered::Withheld {
            level: MarkingLevel::Confidential
        }
    );
}

#[test]
fn every_dataset_audit_row_is_unsigned_until_a_row_signature_is_checked() {
    let e = enclosure();
    let g = e.grant("k.osei").unwrap();
    for row in e.fetch(&g, "midgard.audit").unwrap() {
        let Delivered::Row {
            body, signature, ..
        } = row
        else {
            panic!("withheld from k.osei")
        };
        assert_eq!(signature, Some(SignatureStatus::Unsigned));
        // The mock's check mark is not passed on: it was never checked.
        assert!(!String::from_utf8(body).unwrap().contains('✓'));
    }
}

#[test]
fn row_markings_are_read_strictly_and_never_default_to_open() {
    let ok = r#"{"level":"internal","compartments":[]}"#;
    let bad_markings = [
        // No marking at all: refused, never read as `open`.
        None,
        Some(r#"{"level":"secret","compartments":[]}"#),
        Some(r#"{"level":"INTERNAL","compartments":[]}"#),
        Some(r#"{"level":"","compartments":[]}"#),
        Some(r#"{"compartments":[]}"#),
        Some(r#"{"level":"internal"}"#),
        Some(r#"{"level":"internal","compartments":[1]}"#),
        Some(r#"{"level":"internal","compartments":"OPS-N"}"#),
        Some(r#"{"level":"internal","compartments":[],"extra":true}"#),
        Some(r#""internal""#),
    ];
    let row = |m: Option<&str>| {
        let marking = m.map(|m| format!(r#""marking":{m},"#)).unwrap_or_default();
        BODY.replacen(&format!(r#""marking":{ok},"#), &marking, 1)
    };
    from(&row(Some(ok))).unwrap();
    for m in bad_markings {
        assert!(
            matches!(from(&row(m)), Err(SourceError::Malformed(_))),
            "accepted marking {m:?}"
        );
    }
}

#[test]
fn a_misshapen_dataset_body_is_refused() {
    for (what, body) in [
        (
            "persona clearance",
            BODY.replace(r#""clearance":"restricted""#, r#""clearance":"secret""#),
        ),
        (
            "persona compartments",
            BODY.replace(r#""compartments":["OPS-N","SIG"]"#, r#""compartments":[7]"#),
        ),
        (
            "duplicate persona",
            BODY.replace(r#""id":"k.osei""#, r#""id":"k.rao""#),
        ),
        (
            "duplicate row id",
            BODY.replace(r#"{"id":"AUD-2","marking""#, r#"{"id":"AUD-1","marking""#),
        ),
        (
            "unknown row type",
            BODY.replace(r#""type":"AuditRecord""#, r#""type":"Nothing""#),
        ),
        (
            "row that is not its type",
            BODY.replace(r#""resource":"r","#, ""),
        ),
        (
            "format",
            BODY.replace("midgard-demo-dataset", "something-else"),
        ),
        ("version", BODY.replace(r#""version":1"#, r#""version":2"#)),
        (
            "release",
            BODY.replace(r#""release":"test""#, r#""release":"other""#),
        ),
        (
            "local-suite data",
            BODY.replace(r#""resource":"r""#, r#""resource":"Port Louis harbour""#),
        ),
    ] {
        assert!(
            matches!(from(&body), Err(SourceError::Malformed(_))),
            "accepted a body with a bad {what}"
        );
    }
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn committed() -> Vec<u8> {
    fs::read(repo().join(format!("clients/demo/datasets/{CORE_VERSION}.mgdemo"))).unwrap()
}

#[test]
fn the_committed_dataset_opens() {
    let anchors = TrustAnchors::parse(
        &fs::read_to_string(repo().join("clients/demo/trust/demo-dataset.pub")).unwrap(),
    )
    .unwrap();
    let e =
        DemoEnclosure::open(&committed(), &anchors, &TrustAnchors::new(), CORE_VERSION).unwrap();
    assert_eq!(e.personas().len(), 4);
    assert_eq!(e.release(), CORE_VERSION);
    // The pinned path opens the same file under the anchor compiled into the core.
    let pinned = DemoEnclosure::open_pinned(&committed()).unwrap();
    assert_eq!(pinned.collections(), e.collections());
    assert_eq!(pinned.collections().len(), 53);
}

#[test]
fn the_pinned_anchor_is_the_committed_one() {
    assert_eq!(
        midgard_client_core::demo::enclosure::PINNED_DATASET_ANCHOR,
        fs::read_to_string(repo().join("clients/demo/trust/demo-dataset.pub")).unwrap()
    );
}

#[test]
fn a_dataset_that_does_not_verify_is_refused_with_no_fallback() {
    let file = committed();
    // Any byte changed: refused, and there is no enclosure to fall back on.
    for at in [0, 9, 20, file.len() / 2, file.len() - 1] {
        let mut bad = file.clone();
        bad[at] ^= 1;
        assert!(DemoEnclosure::open_pinned(&bad).is_err(), "byte {at}");
    }
    assert!(DemoEnclosure::open_pinned(&[]).is_err());
    // Another key's anchors refuse it too.
    assert!(matches!(
        DemoEnclosure::open(
            &file,
            &TrustAnchors::new(),
            &TrustAnchors::new(),
            CORE_VERSION
        ),
        Err(DatasetError::UnknownKey(_))
    ));
    assert!(matches!(
        DemoEnclosure::open_pinned_for(&file, "0.0.0"),
        Err(DatasetError::WrongRelease { .. })
    ));
}

/// Run one ping by `who` on `target` in a fresh enclosure; return the client's
/// status and reason, and the audit row as `who` fetches it (resource field blanked:
/// it is the target the actor typed, so it differs by construction).
fn ping_on(who: &str, target: &[u8]) -> (Vec<u8>, Vec<u8>, Delivered) {
    let mut e = enclosure();
    let g = e.grant(who).unwrap();
    let mut client = Replica::new("dev-1");
    let mut ops = ping(&mut client, "action:dev-1-1", who);
    client.set("action:dev-1-1", action::TARGET, target.to_vec());
    ops.extend(client.drain_pending().iter().map(SyncOp::encode));
    merge(&mut client, &e.exchange(&g, &ops).unwrap());
    let row = match e.fetch(&g, "midgard.audit").unwrap().remove(0) {
        Delivered::Row {
            id,
            marking,
            body,
            signature,
        } => {
            let Json::Obj(pairs) = parse(std::str::from_utf8(&body).unwrap()).unwrap() else {
                panic!("not an object")
            };
            let pairs = pairs
                .into_iter()
                .map(|(k, v)| {
                    if k == "resource" {
                        (k, Json::Str(String::new()))
                    } else {
                        (k, v)
                    }
                })
                .collect();
            Delivered::Row {
                id,
                marking,
                body: canon::encode(&Json::Obj(pairs)).into_bytes(),
                signature,
            }
        }
        withheld => withheld,
    };
    (
        client
            .get("action:dev-1-1", action::STATUS)
            .unwrap()
            .to_vec(),
        client
            .get("action:dev-1-1", action::REASON)
            .unwrap()
            .to_vec(),
        row,
    )
}

#[test]
fn an_action_on_a_hidden_row_is_refused_and_audited_as_if_the_row_were_missing() {
    for who in ["k.rao", "k.osei"] {
        let hidden = ping_on(who, b"midgard.audit/AUD-2");
        let missing = ping_on(who, b"midgard.audit/AUD-9");
        if who == "k.rao" {
            assert_eq!(hidden.0, b"refused");
            assert!(matches!(hidden.2, Delivered::Row { .. }), "{:?}", hidden.2);
            assert_eq!(hidden, missing, "the hidden row shows through");
            assert_eq!(hidden, ping_on(who, b"nothing"));
        } else {
            // k.osei may see AUD-2: the action applies, audited at its marking.
            assert_eq!(hidden.0, b"applied");
            assert_eq!(missing.0, b"refused");
        }
    }
}

#[test]
fn the_audit_row_names_the_signed_in_persona_not_the_claimed_one() {
    let mut e = enclosure();
    let g = e.grant("k.rao").unwrap();
    let mut client = Replica::new("dev-1");
    e.exchange(&g, &ping(&mut client, "action:dev-1-1", "k.osei"))
        .unwrap();
    let osei = e.grant("k.osei").unwrap();
    let Delivered::Row { body, .. } = &e.fetch(&osei, "midgard.audit").unwrap()[0] else {
        panic!("withheld")
    };
    let item = parse(std::str::from_utf8(body).unwrap()).unwrap();
    assert_eq!(item.get("actor").and_then(Json::as_str), Some("k.rao"));
    assert_eq!(
        item.get("action").and_then(Json::as_str),
        Some("refused · demo.ping (claimed by k.osei)")
    );
}

fn with_heimdall_audit() -> DemoEnclosure {
    let body = BODY.replace(
        r#""midgard.audit":{"#,
        r#""heimdall.audit":{"type":"AuditRecord","rows":[]},"midgard.audit":{"#,
    );
    from(&body).unwrap()
}

fn audit_counts(e: &DemoEnclosure) -> (usize, usize) {
    let g = e.grant("k.osei").unwrap();
    (
        e.fetch(&g, "heimdall.audit").unwrap().len(),
        e.fetch(&g, "midgard.audit").unwrap().len(),
    )
}

#[test]
fn the_audit_collection_is_the_registered_handlers_console() {
    let mut e = with_heimdall_audit();
    e.register("heimdall.noop", |_, _, _| {
        Outcome::Applied(Json::Obj(vec![]))
    });
    let g = e.grant("k.rao").unwrap();
    let mut client = Replica::new("dev-1");
    // An unregistered verb that names Heimdall is audited in midgard.audit.
    let mut ops = ping(&mut client, "action:dev-1-1", "k.rao");
    client.set("action:dev-1-1", action::VERB, b"heimdall.bogus".to_vec());
    ops.extend(client.drain_pending().iter().map(SyncOp::encode));
    e.exchange(&g, &ops).unwrap();
    assert_eq!(audit_counts(&e), (0, 3));
    let mut ops = ping(&mut client, "action:dev-1-2", "k.rao");
    client.set("action:dev-1-2", action::VERB, b"heimdall.noop".to_vec());
    ops.extend(client.drain_pending().iter().map(SyncOp::encode));
    let back = e.exchange(&g, &ops).unwrap();
    merge(&mut client, &back);
    assert_eq!(
        client.get("action:dev-1-2", action::STATUS),
        Some(&b"applied"[..])
    );
    assert_eq!(audit_counts(&e), (1, 3));
}

#[test]
fn a_lost_reply_is_answered_again_on_re_push() {
    let mut e = with_heimdall_audit();
    let g = e.grant("k.rao").unwrap();
    let mut client = Replica::new("dev-1");
    let ops = ping(&mut client, "action:dev-1-1", "k.rao");
    let _lost = e.exchange(&g, &ops).unwrap();
    let before = audit_counts(&e);
    let back = e.exchange(&g, &ops).unwrap();
    merge(&mut client, &back);
    assert_eq!(
        client.get("action:dev-1-1", action::STATUS),
        Some(&b"applied"[..])
    );
    assert_eq!(audit_counts(&e), before);
    // Merging the same answer again changes nothing.
    let snapshot = client.snapshot();
    merge(&mut client, &e.exchange(&g, &ops).unwrap());
    assert_eq!(client.snapshot(), snapshot);
}

#[test]
fn another_persona_reusing_an_action_id_gets_a_conflict_and_nothing_of_the_first() {
    let mut e = enclosure();
    let rao = e.grant("k.rao").unwrap();
    let osei = e.grant("k.osei").unwrap();
    let mut first = Replica::new("dev-1");
    let back = e
        .exchange(&rao, &ping(&mut first, "action:dev-1-1", "k.rao"))
        .unwrap();
    merge(&mut first, &back);
    let mut second = Replica::new("dev-2");
    let back = e
        .exchange(&osei, &ping(&mut second, "action:dev-1-1", "k.osei"))
        .unwrap();
    merge(&mut second, &back);
    assert_eq!(
        second.get("action:dev-1-1", action::STATUS),
        Some(&b"conflict"[..])
    );
    for f in [action::AUDIT, action::RESULT, action::REASON] {
        assert_ne!(
            second.get("action:dev-1-1", f),
            first.get("action:dev-1-1", f),
            "{f} leaked across personas"
        );
    }
    assert_eq!(
        second.get("action:dev-1-1", action::RESULT),
        Some(&b"{}"[..])
    );
    // Each persona's re-push still gets its own answer.
    let again = e
        .exchange(
            &rao,
            &ping(&mut Replica::new("dev-1"), "action:dev-1-1", "k.rao"),
        )
        .unwrap();
    let mut check = Replica::new("dev-1");
    merge(&mut check, &again);
    assert_eq!(
        check.get("action:dev-1-1", action::STATUS),
        Some(&b"applied"[..])
    );
}

#[test]
fn a_malformed_action_is_refused_as_malformed() {
    let long = "x".repeat(257);
    for (field, value) in [
        (action::PARAMS, r#"{"a":1}"#),
        (action::PARAMS, r#"{"b":"1","a":"2"}"#),
        (action::PARAMS, r#"{ "a":"1"}"#),
        (action::PARAMS, r#"["a"]"#),
        (action::PARAMS, "not json"),
        (action::VERB, long.as_str()),
        (action::TARGET, long.as_str()),
        (action::BY, long.as_str()),
    ] {
        let mut e = enclosure();
        let g = e.grant("k.rao").unwrap();
        let mut client = Replica::new("dev-1");
        let mut ops = ping(&mut client, "action:dev-1-1", "k.rao");
        client.set("action:dev-1-1", field, value.as_bytes().to_vec());
        ops.extend(client.drain_pending().iter().map(SyncOp::encode));
        merge(&mut client, &e.exchange(&g, &ops).unwrap());
        assert_eq!(
            client.get("action:dev-1-1", action::STATUS),
            Some(&b"refused"[..]),
            "{field}={value}"
        );
        assert!(client
            .get("action:dev-1-1", action::REASON)
            .is_some_and(|r| r.starts_with(b"malformed action")));
    }
}

#[test]
fn a_check_mark_in_a_field_delivery_does_not_replace_refuses_the_dataset() {
    let body = BODY.replace(r#""action":"Query""#, r#""action":"Query ✓""#);
    assert!(matches!(from(&body), Err(SourceError::Malformed(_))));
}
