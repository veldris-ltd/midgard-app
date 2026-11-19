//! Clearance is applied at the source for every row type of all six consoles: a row
//! above the persona's grant is delivered withheld, with its level only — no id, no
//! body, no compartment. Checked against the committed, signed dataset, and against
//! the fixtures read independently of the core. [AC-C-13] [FR-C-5] [NFR-SEC-2]

use std::fs;
use std::path::PathBuf;

use attest::json::{parse, Json};
use attest::TrustAnchors;
use midgard_client_core::consoles::admin::{AdminBreakGlass, AdminLedgerEntry};
use midgard_client_core::consoles::bifrost::BifrostFleetNode;
use midgard_client_core::consoles::common::AuditRecord;
use midgard_client_core::consoles::forge::ForgeDataset;
use midgard_client_core::consoles::heimdall::HeimdallTrack;
use midgard_client_core::consoles::midgard::MidgardKeyRecord;
use midgard_client_core::consoles::mimir::MimirSession;
use midgard_client_core::consoles::motion::HeimdallMotion;
use midgard_client_core::demo::audit::{self, SignatureStatus};
use midgard_client_core::demo::dataset;
use midgard_client_core::demo::enclosure::{
    DemoEnclosure, PINNED_AUDIT_ANCHOR, PINNED_DATASET_ANCHOR,
};
use midgard_client_core::marking::{MarkingLevel, Visible};
use midgard_client_core::source::{decode_rows, DataSource, Delivered};
use midgard_client_core::CORE_VERSION;

const CONSOLES: [&str; 6] = ["midgard", "admin", "heimdall", "forge", "mimir", "bifrost"];
const LEVELS: [&str; 4] = ["open", "internal", "restricted", "confidential"];

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn committed() -> DemoEnclosure {
    let file =
        fs::read(repo().join(format!("clients/demo/datasets/{CORE_VERSION}.mgdemo"))).unwrap();
    DemoEnclosure::open_pinned(&file).unwrap()
}

/// The committed body with one more persona, `t.guest`, cleared `open` and holding no
/// compartment: below every row in every console (the lowest row marking is
/// `internal`), so every row of every type must come back withheld.
fn with_guest() -> DemoEnclosure {
    let file =
        fs::read(repo().join(format!("clients/demo/datasets/{CORE_VERSION}.mgdemo"))).unwrap();
    let anchors = TrustAnchors::parse(PINNED_DATASET_ANCHOR).unwrap();
    let ds = dataset::verify(&file, &anchors, CORE_VERSION).unwrap();
    let Json::Obj(mut top) = ds.body else {
        panic!("body is not an object")
    };
    let guest = parse(
        r#"{"id":"t.guest","display":"T. Guest","role":"analyst","clearance":"open","compartments":[],"roles":[]}"#,
    )
    .unwrap();
    for (k, v) in top.iter_mut() {
        if k == "personas" {
            let Json::Arr(list) = v else {
                panic!("personas")
            };
            list.push(guest.clone());
        }
    }
    DemoEnclosure::from_body(CORE_VERSION, &Json::Obj(top), &audit_anchors()).unwrap()
}

fn audit_anchors() -> TrustAnchors {
    TrustAnchors::parse(PINNED_AUDIT_ANCHOR).unwrap()
}

/// One fixture row as the fixtures file has it, read without the core's marking code.
struct FixtureRow {
    id: String,
    level: usize,
    compartments: Vec<String>,
}

/// `(collection, rows)` of a console, from its committed fixtures file.
fn fixture(console: &str) -> Vec<(String, Vec<FixtureRow>)> {
    let path = repo().join(format!(
        "clients/demo/fixtures/{CORE_VERSION}/{console}.json"
    ));
    let v = parse(&fs::read_to_string(path).unwrap()).unwrap();
    let Some(Json::Obj(colls)) = v.get("collections") else {
        panic!("{console}: no collections")
    };
    colls
        .iter()
        .map(|(name, c)| {
            let rows = c
                .get("rows")
                .and_then(Json::as_array)
                .unwrap()
                .iter()
                .map(|r| {
                    let m = r.get("marking").unwrap();
                    let word = m.get("level").and_then(Json::as_str).unwrap();
                    FixtureRow {
                        id: r.get("id").and_then(Json::as_str).unwrap().to_string(),
                        level: LEVELS.iter().position(|l| *l == word).unwrap(),
                        compartments: m
                            .get("compartments")
                            .and_then(Json::as_array)
                            .unwrap()
                            .iter()
                            .map(|c| c.as_str().unwrap().to_string())
                            .collect(),
                    }
                })
                .collect();
            (name.clone(), rows)
        })
        .collect()
}

/// `(level, compartments)` of a persona, from the committed personas file.
fn persona(id: &str) -> (usize, Vec<String>) {
    if id == "t.guest" {
        return (0, Vec::new());
    }
    let path = repo().join(format!(
        "clients/demo/fixtures/{CORE_VERSION}/personas.json"
    ));
    let v = parse(&fs::read_to_string(path).unwrap()).unwrap();
    let p = v
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p.get("id").and_then(Json::as_str) == Some(id))
        .unwrap();
    let word = p.get("clearance").and_then(Json::as_str).unwrap();
    let comps = p
        .get("compartments")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    (LEVELS.iter().position(|l| *l == word).unwrap(), comps)
}

/// Fetch every collection of `console` for `who` and check each delivered row against
/// the fixtures: shown exactly when the persona's level and compartments cover the
/// row, and when withheld, nothing of the row but its level. Returns (shown, withheld).
fn check_console(e: &DemoEnclosure, who: &str, console: &str) -> (usize, usize) {
    let grant = e.grant(who).unwrap();
    let (level, held) = persona(who);
    let (mut shown, mut withheld) = (0, 0);
    let all_compartments = ["OPS-N", "SIG"];
    for (name, rows) in fixture(console) {
        let delivered = e.fetch(&grant, &name).unwrap();
        assert_eq!(delivered.len(), rows.len(), "{name}");
        for (row, d) in rows.iter().zip(&delivered) {
            let may = row.level <= level && row.compartments.iter().all(|c| held.contains(c));
            match d {
                Delivered::Row { id, marking, .. } => {
                    assert!(may, "{who} was given {name}/{} above the grant", row.id);
                    assert_eq!(id, &row.id);
                    assert_eq!(marking.level().as_str(), LEVELS[row.level]);
                    assert_eq!(marking.compartments(), &row.compartments[..]);
                    shown += 1;
                }
                Delivered::Withheld { level } => {
                    assert!(!may, "{who} was refused {name}/{} within the grant", row.id);
                    assert_eq!(level.as_str(), LEVELS[row.level]);
                    let text = format!("{d:?}");
                    assert!(!text.contains(&row.id), "{name}: withheld row leaks its id");
                    for c in all_compartments {
                        assert!(!text.contains(c), "{name}: withheld row leaks {c}");
                    }
                    withheld += 1;
                }
            }
        }
    }
    (shown, withheld)
}

/// The withheld rows of `collection` for `who`, decoded as `T` where shown.
fn typed<T: midgard_client_core::consoles::decode::FromJson>(
    e: &DemoEnclosure,
    who: &str,
    collection: &str,
) -> Vec<Visible<T>> {
    let g = e.grant(who).unwrap();
    decode_rows(e.fetch(&g, collection).unwrap()).unwrap()
}

fn withheld<T>(rows: &[Visible<T>]) -> Vec<MarkingLevel> {
    rows.iter()
        .filter_map(|r| match r {
            Visible::Withheld { level } => Some(*level),
            Visible::Shown { .. } => None,
        })
        .collect()
}

#[test]
fn a_guest_below_every_row_gets_every_row_of_every_console_withheld() {
    let e = with_guest();
    for console in CONSOLES {
        let (shown, withheld) = check_console(&e, "t.guest", console);
        assert_eq!(shown, 0, "{console}");
        assert!(withheld > 0, "{console}");
    }
}

#[test]
fn every_persona_sees_exactly_what_the_fixtures_allow_in_every_console() {
    let e = committed();
    for who in ["k.rao", "j.benjamin", "k.osei", "a.holt"] {
        for console in CONSOLES {
            check_console(&e, who, console);
        }
    }
}

#[test]
fn the_persona_counts_of_the_fixture_check_hold_at_the_source() {
    // tools/ci/tests/demo-fixtures.test.sh: k.rao sees 13 Heimdall rows, a.holt all
    // 30 Bifrost rows; k.osei holds every compartment at the top level.
    let e = committed();
    assert_eq!(check_console(&e, "k.rao", "heimdall"), (13, 25));
    assert_eq!(check_console(&e, "a.holt", "bifrost"), (30, 0));
    for console in CONSOLES {
        let (_, withheld) = check_console(&e, "k.osei", console);
        assert_eq!(withheld, 0, "k.osei is refused a {console} row");
    }
}

#[test]
fn midgard_keys_above_an_analyst_are_withheld() {
    let rows = typed::<MidgardKeyRecord>(&committed(), "k.rao", "midgard.keys");
    assert_eq!(withheld(&rows), [MarkingLevel::Confidential; 3]);
    assert_eq!(rows.iter().filter(|r| r.shown().is_some()).count(), 1);
}

#[test]
fn admin_break_glass_and_ledger_are_withheld_below_their_marking() {
    let e = committed();
    let rows = typed::<AdminBreakGlass>(&e, "k.rao", "admin.breakGlass");
    assert_eq!(withheld(&rows), [MarkingLevel::Confidential]);
    let rows = typed::<AdminLedgerEntry>(&e, "a.holt", "admin.ledger");
    assert_eq!(withheld(&rows), [MarkingLevel::Restricted; 5]);
    assert_eq!(
        typed::<AdminLedgerEntry>(&e, "k.rao", "admin.ledger")
            .iter()
            .filter(|r| r.shown().is_some())
            .count(),
        5
    );
}

#[test]
fn heimdall_ops_north_rows_and_motion_are_withheld_from_an_analyst() {
    let e = committed();
    let audit = typed::<AuditRecord>(&e, "k.rao", "heimdall.audit");
    assert_eq!(withheld(&audit), [MarkingLevel::Confidential; 5]);
    let tracks = typed::<HeimdallTrack>(&e, "k.rao", "heimdall.tracks");
    assert_eq!(withheld(&tracks), [MarkingLevel::Confidential; 4]);
    let motion = typed::<HeimdallMotion>(&e, "k.rao", "heimdall.motion");
    assert_eq!(withheld(&motion), [MarkingLevel::Confidential; 4]);
    let shown: Vec<&HeimdallMotion> = motion.iter().filter_map(Visible::shown).collect();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].samples.len(), 360);
    // j.benjamin holds OPS-N: every track's motion.
    let motion = typed::<HeimdallMotion>(&e, "j.benjamin", "heimdall.motion");
    assert!(withheld(&motion).is_empty());
}

#[test]
fn forge_datasets_above_a_release_manager_are_withheld() {
    let rows = typed::<ForgeDataset>(&committed(), "a.holt", "forge.datasets");
    let mut levels = withheld(&rows);
    levels.sort();
    assert_eq!(
        levels,
        [MarkingLevel::Restricted, MarkingLevel::Confidential]
    );
}

#[test]
fn a_mimir_session_in_a_compartment_not_held_is_withheld() {
    // j.benjamin is confidential with OPS-N only: the SIG session is withheld even
    // though the level is within the grant.
    let rows = typed::<MimirSession>(&committed(), "j.benjamin", "mimir.sessions");
    assert_eq!(withheld(&rows), [MarkingLevel::Confidential]);
    assert_eq!(rows.iter().filter(|r| r.shown().is_some()).count(), 1);
}

#[test]
fn bifrost_rows_are_withheld_below_internal() {
    let rows = typed::<BifrostFleetNode>(&with_guest(), "t.guest", "bifrost.fleet");
    assert_eq!(withheld(&rows), [MarkingLevel::Internal; 5]);
    assert!(withheld(&typed::<BifrostFleetNode>(
        &committed(),
        "a.holt",
        "bifrost.fleet"
    ))
    .is_empty());
}

#[test]
fn no_delivered_row_of_any_console_claims_an_unchecked_signature() {
    let e = committed();
    let g = e.grant("k.osei").unwrap();
    let mut replaced = 0;
    for (name, _) in e.collections() {
        for d in e.fetch(&g, &name).unwrap() {
            let Delivered::Row { body, .. } = d else {
                panic!("k.osei is refused a {name} row")
            };
            let text = String::from_utf8(body).unwrap();
            assert!(!text.contains('\u{2713}'), "{name}: {text}");
            assert!(
                !text.contains(r#""stateLabel":"Verified""#),
                "{name}: {text}"
            );
            replaced += text
                .matches(midgard_client_core::demo::audit::NOT_CHECKED)
                .count();
        }
    }
    // 4 artefact signatures, 2 artefact "Verified" labels, 4 fleet agents, 3 decisions.
    assert_eq!(replaced, 13);
}

#[test]
fn every_built_in_audit_row_of_the_committed_dataset_is_verified() {
    let e = committed();
    let g = e.grant("k.osei").unwrap();
    let mut verified = 0;
    for (name, ty) in e.collections() {
        for d in e.fetch(&g, &name).unwrap() {
            let Delivered::Row {
                body, signature, ..
            } = d
            else {
                panic!("k.osei is refused a {name} row")
            };
            if ty == audit::AUDIT_TYPE {
                assert_eq!(signature, Some(SignatureStatus::Verified), "{name}");
                let item = parse(std::str::from_utf8(&body).unwrap()).unwrap();
                assert_eq!(
                    item.get("pqSignature").and_then(Json::as_str),
                    Some("verified")
                );
                verified += 1;
            } else {
                assert_eq!(signature, None, "{name}");
            }
        }
    }
    assert_eq!(verified, 20);
}

#[test]
fn a_tampered_built_in_audit_row_refuses_the_dataset() {
    let file =
        fs::read(repo().join(format!("clients/demo/datasets/{CORE_VERSION}.mgdemo"))).unwrap();
    let anchors = TrustAnchors::parse(PINNED_DATASET_ANCHOR).unwrap();
    let ds = dataset::verify(&file, &anchors, CORE_VERSION).unwrap();
    let text = midgard_client_core::demo::canon::encode(&ds.body);
    let load =
        |t: &str| DemoEnclosure::from_body(CORE_VERSION, &parse(t).unwrap(), &audit_anchors());
    load(&text).unwrap();
    // One audit row's actor changed under its signature (heimdall.audit, row 0).
    let mut body = ds.body.clone();
    let mut changed = false;
    if let Json::Obj(top) = &mut body {
        for (k, colls) in top.iter_mut() {
            let (true, Json::Obj(colls)) = (k == "collections", colls) else {
                continue;
            };
            for (name, c) in colls.iter_mut() {
                let (true, Json::Obj(c)) = (name == "heimdall.audit", c) else {
                    continue;
                };
                for (ck, rows) in c.iter_mut() {
                    let (true, Json::Arr(rows)) = (ck == "rows", rows) else {
                        continue;
                    };
                    let Json::Obj(row) = &mut rows[0] else {
                        panic!("row")
                    };
                    for (rk, item) in row.iter_mut() {
                        let (true, Json::Obj(item)) = (rk == "item", item) else {
                            continue;
                        };
                        for (ik, v) in item.iter_mut() {
                            if ik == "actor" {
                                *v = Json::Str("k.rao".into());
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(changed);
    let tampered = midgard_client_core::demo::canon::encode(&body);
    assert!(load(&tampered).is_err());
    // Two rows' signatures swapped.
    let sigs: Vec<&str> = text
        .split(r#""auditSig":""#)
        .skip(1)
        .map(|t| &t[..t.find('"').unwrap()])
        .collect();
    assert_eq!(sigs.len(), 20);
    let swapped = text
        .replacen(sigs[0], "SWAP", 1)
        .replacen(sigs[1], sigs[0], 1)
        .replacen("SWAP", sigs[1], 1);
    assert!(load(&swapped).is_err());
}
