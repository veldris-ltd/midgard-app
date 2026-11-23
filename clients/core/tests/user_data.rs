//! Your own data: imports checked as `mg-admin import` checks them (the guide's
//! examples and the sample sets accepted, the ontology fixtures refused, all or
//! nothing), imported rows under the personas' clearance, Heimdall observations, and
//! the address rule. [DD-1] [RC-10] [AC-C-13] [NFR-SEC-2]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};
use std::process::Command;

use attest::json::{parse, Json};
use midgard_client_core::consoles::common::AuditRecord;
use midgard_client_core::consoles::midgard::{MidgardObjectInstance, MidgardObjectType};
use midgard_client_core::device_audit::DeviceAuditKey;
use midgard_client_core::keystore::SoftwareKeystore;
use midgard_client_core::marking::{MarkingLevel, Visible};
use midgard_client_core::sample_source::SampleSource;
use midgard_client_core::session::{ActionState, Session};
use midgard_client_core::source::{DataSource, Delivered};
use midgard_client_core::user_data::{
    self, check, check_feed_address, check_gateway_address, kind_of, parse_observations, FileKind,
    NamedText,
};
use pqc_agility::backend::fips::provider;
use pqc_agility::OsRng;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn file(rel: &str) -> NamedText {
    let path = root().join(rel);
    NamedText {
        name: path.file_name().unwrap().to_string_lossy().into_owned(),
        text: std::fs::read_to_string(&path).unwrap(),
    }
}

fn text(name: &str, text: &str) -> NamedText {
    NamedText {
        name: name.into(),
        text: text.into(),
    }
}

fn utility() -> (NamedText, NamedText) {
    (
        file("docs/guides/examples/utility/ontology.json"),
        file("docs/guides/examples/utility/network.jsonl"),
    )
}

fn reserved() -> Vec<String> {
    SampleSource::new().unwrap().reserved_types()
}

#[test]
fn the_guides_example_imports_as_mg_admin_imports_it() {
    let (ontology, network) = utility();
    let c = check(None, Some(&ontology), &[network], &reserved()).unwrap();
    // "imported 7 object(s) and 4 link(s)"
    assert_eq!((c.objects.len(), c.links.len()), (7, 4));
    assert_eq!(c.files[0].kind, FileKind::Records);
    assert_eq!(c.unreadable(), 0);
}

#[test]
fn the_sample_sets_import() {
    // convert.py (standard library only) writes the ontology and import file the
    // samples README loads: "imported 418 object(s) and 28 link(s)".
    let run = |cmd: &str| {
        Command::new("python3")
            .arg(root().join("samples/convert.py"))
            .arg(cmd)
            .current_dir(root())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8(o.stdout).unwrap())
    };
    let (Some(ontology), Some(records)) = (run("ontology"), run("import")) else {
        eprintln!("skipped: python3 is not available to run samples/convert.py");
        return;
    };
    let c = check(
        None,
        Some(&text("ontology.json", &ontology)),
        &[text("samples.jsonl", &records)],
        &reserved(),
    )
    .unwrap();
    assert_eq!((c.objects.len(), c.links.len()), (418, 28));

    // And every observation file of the set, as Heimdall observations.
    let dir = root().join("samples/4-heimdall-observations");
    let mut n = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(kind_of(&body), FileKind::Observations, "{path:?}");
        let layers = parse_observations("x.json", &body).unwrap();
        assert_eq!(layers.len(), 1);
        assert!(
            layers[0].observations.iter().all(|o| o.position.is_some()),
            "{path:?}"
        );
        n += 1;
    }
    assert_eq!(n, 10);
    // The tab-separated lines `convert.py observations` writes for heimdall-source.
    let tsv = Command::new("python3")
        .arg(root().join("samples/convert.py"))
        .args(["observations", "--feeds", "gb-grid,reservoirs"])
        .output()
        .unwrap();
    let tsv = String::from_utf8(tsv.stdout).unwrap();
    assert_eq!(kind_of(&tsv), FileKind::Observations);
    let layers = parse_observations("observations.tsv", &tsv).unwrap();
    let feeds: Vec<&str> = layers.iter().map(|l| l.feed.as_str()).collect();
    assert_eq!(feeds, ["gb-grid", "reservoirs"]);
    assert_eq!(layers[0].total, 11);
    let grid = &layers[0].observations[0];
    assert_eq!(
        (grid.entity.as_str(), grid.label()),
        ("GB-GRID", "GB electricity grid")
    );
    assert_eq!(grid.position, Some((54.3, -2.3)));
}

#[test]
fn the_ontology_fixtures_are_refused_and_the_valid_ones_read() {
    let dir = root().join("contracts/fixtures/ontology");
    let (_, network) = utility();
    let mut refused = 0;
    for sub in ["invalid", "beyond-schema"] {
        for entry in std::fs::read_dir(dir.join(sub)).unwrap() {
            let path = entry.unwrap().path();
            let ontology = NamedText {
                name: "ontology.json".into(),
                text: std::fs::read_to_string(&path).unwrap(),
            };
            let err = check(None, Some(&ontology), &[network.clone()], &[]).unwrap_err();
            assert!(
                err.starts_with("ontology.json: ontology file refused: "),
                "{path:?}: {err}"
            );
            refused += 1;
        }
    }
    assert_eq!(refused, 31);
    for entry in std::fs::read_dir(dir.join("valid")).unwrap() {
        let path = entry.unwrap().path();
        let ontology = NamedText {
            name: "ontology.json".into(),
            text: std::fs::read_to_string(&path).unwrap(),
        };
        // A valid ontology reads; the example's data is not written against it.
        let err = check(None, Some(&ontology), &[network.clone()], &[]).unwrap_err();
        assert!(
            err.starts_with("network.jsonl: import refused: line 1"),
            "{path:?}: {err}"
        );
    }
}

#[test]
fn an_import_is_all_or_nothing_with_mg_admins_refusals() {
    let (ontology, network) = utility();
    let first = check(None, Some(&ontology), &[network.clone()], &[]).unwrap();

    // The guide's three refusals, word for word after the file's name.
    let bad = text(
        "bad.jsonl",
        "{\"object\":\"transformer\",\"key\":\"TX-9\",\"marking\":\"internal\",\"purposes\":[\"operations\"],\"properties\":{\"serial\":\"TX-9\",\"rating_kva\":\"500\"}}\n",
    );
    assert_eq!(
        check(Some(&first), None, &[bad], &[]).unwrap_err(),
        "bad.jsonl: import refused: line 1 (transformer/TX-9): \"rating_kva\" must be an integer, not \"500\""
    );
    let missing = text(
        "missing.jsonl",
        "{\"object\":\"transformer\",\"key\":\"TX-9\",\"marking\":\"internal\",\"purposes\":[\"operations\"],\"properties\":{\"serial\":\"TX-9\"}}\n",
    );
    assert_eq!(
        check(Some(&first), None, &[missing], &[]).unwrap_err(),
        "missing.jsonl: import refused: line 1 (transformer/TX-9): required property \"rating_kva\" is missing"
    );
    let again = text("again.jsonl", network.text.lines().next().unwrap());
    assert_eq!(
        check(Some(&first), None, &[again], &[]).unwrap_err(),
        "again.jsonl: import refused: substation/SS-ELM is already in this app; an import never replaces an object"
    );

    // The first bad line refuses the whole import, earlier good files included.
    let good = text(
        "good.jsonl",
        "{\"object\":\"feeder\",\"key\":\"FDR-NEW\",\"marking\":\"internal\",\"purposes\":[\"operations\"],\"properties\":{\"name\":\"New\"}}\n{\"link\":\"feeder_from\",\"from\":\"FDR-NEW\",\"to\":\"SS-ELM\"}\n",
    );
    let third = text(
        "third.jsonl",
        "{\"object\":\"feeder\",\"key\":\"FDR-3\",\"marking\":\"internal\",\"purposes\":[\"operations\"],\"properties\":{\"name\":\"3\"}}\n\n{\"link\":\"feeder_from\",\"from\":\"FDR-3\",\"to\":\"SS-NOWHERE\"}\n",
    );
    let err = check(Some(&first), None, &[good.clone(), third], &[]).unwrap_err();
    assert!(err.starts_with("third.jsonl: import refused: link feeder_from from feeder/FDR-3 to substation/SS-NOWHERE names substation/SS-NOWHERE"), "{err}");
    // Nothing of it was kept: `first` is unchanged, and the good file alone imports,
    // its link resolving to an object imported before.
    assert_eq!(first.objects.len(), 7);
    let more = check(Some(&first), None, &[good], &[]).unwrap();
    assert_eq!((more.objects.len(), more.links.len()), (8, 5));

    // One ontology; and never a type the sample data declares.
    let other = text("ontology.json", "{\"version\":1,\"objectTypes\":[]}");
    assert!(check(Some(&first), Some(&other), &[], &[])
        .unwrap_err()
        .contains("already has an ontology"));
    let clash = check(None, Some(&ontology), &[network], &["substation".into()]).unwrap_err();
    assert!(
        clash.contains("object type \"substation\" is already declared by the sample data"),
        "{clash}"
    );
}

fn session(persona: &str, source: SampleSource) -> Session<SoftwareKeystore, SampleSource> {
    let ks = SoftwareKeystore::generate(&provider(), "device-root", &mut OsRng).unwrap();
    let (device, _) = DeviceAuditKey::create(&ks, &mut OsRng).unwrap();
    Session::open(ks, device, source, persona, "dev-1").unwrap()
}

fn shown<T>(rows: &[Visible<T>]) -> Vec<&T> {
    rows.iter()
        .filter_map(|v| match v {
            Visible::Shown { item, .. } => Some(item),
            Visible::Withheld { .. } => None,
        })
        .collect()
}

/// The import as the app makes it: checked, staged, then installed by the audited
/// action `midgard.importData`.
fn import(
    s: &mut Session<SoftwareKeystore, SampleSource>,
    files: Vec<NamedText>,
    ontology: NamedText,
) -> String {
    let checked = check(
        s.source().user_data(),
        Some(&ontology),
        &files,
        &s.source().reserved_types(),
    )
    .unwrap();
    let digest = s.source_mut().stage_user_data(checked);
    s.rows::<MidgardObjectType>("midgard.objectTypes").unwrap();
    let id = s
        .request_action(
            "midgard.importData",
            "midgard.objectTypes/*",
            &[("digest".into(), digest)],
        )
        .unwrap();
    s.sync().unwrap();
    match s.action_state(&id) {
        ActionState::Applied { audit, .. } => audit,
        other => panic!("not applied: {other:?}"),
    }
}

#[test]
fn imported_rows_keep_their_markings_and_the_import_is_audited() {
    let (ontology, network) = utility();
    let mut s = session("k.rao", SampleSource::new().unwrap());
    let audit = import(&mut s, vec![network], ontology);

    // Audited in the app's ledger, as every action is.
    let ledger = s.rows::<AuditRecord>("midgard.audit").unwrap();
    let row = shown(&ledger).into_iter().find(|r| r.id == audit).unwrap();
    assert_eq!(row.action, "applied · midgard.importData");
    assert_eq!(row.actor, "k.rao");

    // k.rao (restricted, no compartment) reads the internal objects; Quayside
    // (restricted // GRID-OPS) is withheld, by level only.
    let objects = s.rows::<MidgardObjectInstance>("midgard.objects").unwrap();
    let imported: Vec<&MidgardObjectInstance> = shown(&objects)
        .into_iter()
        .filter(|o| ["substation", "feeder", "transformer"].contains(&o.type_id.as_str()))
        .collect();
    assert_eq!(imported.len(), 6);
    assert!(imported.iter().all(|o| o.id != "substation/SS-QUAY"));
    assert!(objects
        .iter()
        .any(|v| matches!(v, Visible::Withheld { level } if *level == MarkingLevel::Restricted)));
    let tx = imported
        .iter()
        .find(|o| o.id == "transformer/TX-40117")
        .unwrap();
    let rating = tx
        .property_values
        .0
        .iter()
        .find(|(k, _)| k == "rating_kva")
        .map(|(_, v)| v.as_str());
    assert_eq!(rating, Some("500"));

    // The type rows count only what the reader may read.
    let types = s.rows::<MidgardObjectType>("midgard.objectTypes").unwrap();
    let sub = shown(&types)
        .into_iter()
        .find(|t| t.api_name == "substation")
        .unwrap();
    assert_eq!(sub.instance_count, "1");
    assert_eq!(sub.marking, "internal");
    assert!(sub.updated.contains("network.jsonl"));

    // A query and a search-around over the imported data, as the gateway answers.
    let id = s
        .request_action(
            "midgard.query",
            "midgard.objectTypes/transformer",
            &[(
                "where".into(),
                r#"[{"property":"load_fraction","op":"gt","value":"0.8"}]"#.into(),
            )],
        )
        .unwrap();
    s.sync().unwrap();
    let ActionState::Applied { result, .. } = s.action_state(&id) else {
        panic!()
    };
    let answer = parse(&result).unwrap();
    assert_eq!(answer.get("total"), Some(&Json::Num("1".into())));
    let id = s
        .request_action(
            "midgard.searchAround",
            "midgard.objectTypes/substation",
            &[
                ("seedId".into(), "SS-ELM".into()),
                ("hops".into(), "2".into()),
            ],
        )
        .unwrap();
    s.sync().unwrap();
    let ActionState::Applied { result, .. } = s.action_state(&id) else {
        panic!()
    };
    let graph = parse(&result).unwrap();
    let nodes = graph.get("nodes").and_then(Json::as_array).unwrap();
    // The substation, its two feeders and the two transformers on them.
    assert_eq!(nodes.len(), 5);
    assert_eq!(
        nodes[0].get("label").and_then(Json::as_str),
        Some("Elm Road primary")
    );

    // A persona at internal sees the same; a quick look at its type count.
    let source = s.into_source();
    let holt = session("a.holt", source);
    let grant = holt.grant().clone();
    let delivered = holt.source().fetch(&grant, "midgard.objects").unwrap();
    let withheld = delivered
        .iter()
        .filter(|d| matches!(d, Delivered::Withheld { .. }))
        .count();
    assert!(withheld >= 1);
}

#[test]
fn objects_without_the_operations_purpose_are_as_if_absent() {
    let ontology = file("docs/guides/examples/utility/ontology.json");
    let records = text(
        "assets.jsonl",
        "{\"object\":\"feeder\",\"key\":\"F-1\",\"marking\":\"open\",\"purposes\":[\"asset-management\"],\"properties\":{\"name\":\"Hidden\"}}\n{\"object\":\"feeder\",\"key\":\"F-2\",\"marking\":\"open\",\"purposes\":[\"operations\"],\"properties\":{\"name\":\"Shown\"}}\n",
    );
    let mut s = session("k.osei", SampleSource::new().unwrap());
    import(&mut s, vec![records], ontology);
    assert_eq!(s.source().user_data().unwrap().unreadable(), 1);
    let objects = s.rows::<MidgardObjectInstance>("midgard.objects").unwrap();
    let ids: Vec<&str> = shown(&objects).iter().map(|o| o.id.as_str()).collect();
    assert!(ids.contains(&"feeder/F-2"));
    assert!(!ids.contains(&"feeder/F-1"));
}

#[test]
fn a_saved_import_reads_back_and_a_changed_one_is_refused() {
    let (ontology, network) = utility();
    let c = check(None, Some(&ontology), &[network], &[]).unwrap();
    let back = user_data::restore(&c.persisted(), &[]).unwrap();
    assert_eq!(back, c);
    let tampered = String::from_utf8(c.persisted())
        .unwrap()
        .replace("\\\"voltage_kv\\\":33", "\\\"voltage_kv\\\":\\\"33\\\"");
    assert!(user_data::restore(tampered.as_bytes(), &[])
        .unwrap_err()
        .contains("must be an integer"));
}

#[test]
fn observations_are_parsed_strictly() {
    let doc = r#"{"feed":"grid","sample":[{"entity":"A","kind":"k","observedAt":1,"attrs":{"latitude":51.5,"longitude":-0.1,"name":"Alpha","nested":{"x":1}}},{"entity":"B","kind":"k","observedAt":2}]}"#;
    let layers = parse_observations("grid.json", doc).unwrap();
    assert_eq!(layers[0].feed, "grid");
    assert_eq!(layers[0].observations[0].position, Some((51.5, -0.1)));
    assert_eq!(layers[0].observations[0].label(), "Alpha");
    // Nested values are left out, as convert.py leaves them out.
    assert!(layers[0].observations[0]
        .attrs
        .iter()
        .all(|(k, _)| k != "nested"));
    assert_eq!(layers[0].observations[1].position, None);

    // JSON Lines of single observations (a feed URL's answer).
    let lines = "{\"entity\":\"A\",\"kind\":\"k\",\"observedAt\":1,\"attrs\":{\"lat\":1,\"lon\":2}}\n{\"entity\":\"B\",\"kind\":\"k\",\"observedAt\":2,\"attrs\":{\"lat\":100,\"lon\":2}}\n";
    let layers = parse_observations("my feed", lines).unwrap();
    assert_eq!((layers[0].feed.as_str(), layers[0].total), ("my feed", 2));
    // Out of range: kept, but not placed.
    assert_eq!(layers[0].observations[1].position, None);

    for (bad, why) in [
        (
            r#"{"feed":"x","sample":[{"entity":"A","observedAt":1}]}"#,
            "\"kind\"",
        ),
        (
            r#"{"feed":"x","sample":[{"entity":"A","kind":"k","observedAt":-5}]}"#,
            "Unix seconds",
        ),
        (
            r#"{"feed":"x","sample":[{"entity":"A","kind":"k","when":1}]}"#,
            "unknown key \"when\"",
        ),
        (r#"{"feed":"x","rows":[]}"#, "unknown key \"rows\""),
        (
            "{\"entity\":\"A\",\"kind\":\"k\"}\nnot json\n",
            "line 2: not valid JSON",
        ),
        ("gb\tk\tE\tsoon\tlat=1", "Unix seconds"),
        ("", "empty"),
    ] {
        let err = parse_observations("f", bad).unwrap_err();
        assert!(
            err.starts_with("f: observations refused: ") && err.contains(why),
            "{bad}: {err}"
        );
    }
}

#[test]
fn addresses_follow_the_sdks_rule() {
    for ok in [
        "https://gw.example.org",
        "https://gw.example.org:8443/api/v1/",
        "http://127.0.0.1:7790",
        "http://localhost:7790/api/v1",
        "http://[::1]:7790",
        "HTTPS://Gw.Example.Org",
    ] {
        assert!(check_gateway_address(ok).is_ok(), "{ok}");
    }
    assert_eq!(
        check_gateway_address("https://gw.example.org")
            .unwrap()
            .url(),
        "https://gw.example.org/api/v1"
    );
    assert_eq!(
        check_gateway_address("http://127.0.0.1:7790/api/v1/")
            .unwrap()
            .url(),
        "http://127.0.0.1:7790/api/v1"
    );
    for plaintext in [
        "http://gw.example.org",
        "http://10.0.0.5:7710",
        "http://127.0.0.2",
        "http://localhost.evil.org",
    ] {
        assert_eq!(
            check_gateway_address(plaintext).unwrap_err(),
            "Refusing a plaintext gateway URL; use https:// unless the edge is on loopback",
            "{plaintext}"
        );
    }
    for broken in [
        "gw.example.org",
        "ftp://gw.example.org",
        "https://",
        "https://user:pw@gw.example.org",
        "https://gw example.org",
        "https://gw.example.org?x=1",
    ] {
        assert!(
            check_gateway_address(broken)
                .unwrap_err()
                .starts_with("Refusing a gateway URL that does not parse"),
            "{broken}"
        );
    }
    assert!(check_feed_address("https://feeds.example.org/obs.json?since=1").is_ok());
    assert!(check_feed_address("http://127.0.0.1:9000/feed").is_ok());
    assert_eq!(
        check_feed_address("http://feeds.example.org/obs.json").unwrap_err(),
        "Refusing a plaintext feed URL; use https:// unless the feed is on loopback"
    );
}
