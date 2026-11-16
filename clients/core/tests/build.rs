//! The dataset body is built only from fixtures whose digests match their manifest,
//! whose numbers have one spelling and which the local-suite guard passes; inputs
//! come only from the release's mocks and the committed demo sources. [AC-C-13]
//! [NFR-PRV-1] [NFR-PQ-3]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use attest::hex::hex;
use attest::json::parse;
use midgard_client_core::demo::build::{self, BuildError, InputOrigin};
use midgard_client_core::demo::guard::GuardError;
use pqc_agility::sha256;

/// A fresh fixture directory holding `files` (name, text) and a manifest naming them.
fn fixtures(files: &[(&str, &str)]) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "midgard-demo-build-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut listed = Vec::new();
    for (name, text) in files {
        fs::write(dir.join(name), text).unwrap();
        listed.push(format!("{name:?}:{:?}", hex(&sha256(text.as_bytes()))));
    }
    let manifest = format!(
        r#"{{"files":{{{}}},"format":"midgard-demo-fixtures","inputs":{{"clients/demo/personas.json":"0000"}},"release":"0.1.0-rc.3","version":1}}"#,
        listed.join(",")
    );
    fs::write(dir.join("manifest.json"), manifest).unwrap();
    dir
}

const PERSONAS: &str = r#"[{"clearance":"internal","id":"a.holt"}]"#;

fn console(rows: &str) -> String {
    format!(
        r#"{{"collections":{{"midgard.audit":{{"rows":{rows},"type":"AuditEntry"}}}},"console":"midgard"}}"#
    )
}

fn body_of(dir: &Path) -> Result<String, BuildError> {
    build::canonical_body(dir).map(|b| String::from_utf8(b).unwrap())
}

#[test]
fn the_committed_fixtures_build_a_canonical_body() {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../clients/demo/fixtures/0.1.0-rc.3");
    let body = build::canonical_body(&dir).unwrap();
    let v = parse(std::str::from_utf8(&body).unwrap()).unwrap();
    assert_eq!(
        midgard_client_core::demo::canon::encode(&v).as_bytes(),
        &body[..]
    );
    assert_eq!(
        v.get("format").and_then(|x| x.as_str()),
        Some("midgard-demo-dataset")
    );
    assert_eq!(
        v.get("release").and_then(|x| x.as_str()),
        Some("0.1.0-rc.3")
    );
    assert!(v
        .get("collections")
        .and_then(|c| c.get("heimdall.motion"))
        .is_some());
    assert!(v
        .get("personas")
        .and_then(|p| p.as_array())
        .is_some_and(|p| p.len() == 4));
}

#[test]
fn a_small_fixture_set_builds_the_expected_body() {
    let rows = r#"[{"id":"1","item":{"n":12.5},"marking":{"level":"internal"}}]"#;
    let dir = fixtures(&[
        ("personas.json", PERSONAS),
        ("midgard.json", &console(rows)),
    ]);
    assert_eq!(
        body_of(&dir).unwrap(),
        format!(
            r#"{{"collections":{{"midgard.audit":{{"rows":{rows},"type":"AuditEntry"}}}},"format":"midgard-demo-dataset","inputs":{{"clients/demo/personas.json":"0000"}},"personas":{PERSONAS},"release":"0.1.0-rc.3","version":1}}"#
        )
    );
}

#[test]
fn a_file_that_differs_from_its_manifest_digest_is_refused() {
    let dir = fixtures(&[
        ("personas.json", PERSONAS),
        ("midgard.json", &console("[]")),
    ]);
    fs::write(dir.join("midgard.json"), console(r#"[{"id":"x"}]"#)).unwrap();
    assert!(matches!(body_of(&dir), Err(BuildError::Digest(n)) if n == "midgard.json"));
}

#[test]
fn local_suite_data_anywhere_in_the_body_stops_the_build() {
    let rows = r#"[{"id":"1","item":{"where":"Port Louis harbour"}}]"#;
    let dir = fixtures(&[
        ("personas.json", PERSONAS),
        ("midgard.json", &console(rows)),
    ]);
    match body_of(&dir) {
        Err(BuildError::Guard(found)) => assert!(
            found.iter().any(|g| matches!(g, GuardError::Marker { .. })),
            "{found:?}"
        ),
        other => panic!("not refused by the guard: {other:?}"),
    }
    // In a persona too: the guard reads the whole body, not only the collections.
    let dir = fixtures(&[
        (
            "personas.json",
            r#"[{"display":"opensky-network.org","id":"x"}]"#,
        ),
        ("midgard.json", &console("[]")),
    ]);
    assert!(matches!(body_of(&dir), Err(BuildError::Guard(_))));
}

#[test]
fn a_number_not_in_its_one_spelling_is_refused() {
    for bad in [
        "1E2",
        "1e2",
        "100.0",
        "1.50",
        "-0",
        "0.0",
        "1e+2",
        "0.0000001",
    ] {
        let rows = format!(r#"[{{"id":"1","item":{{"n":{bad}}}}}]"#);
        let dir = fixtures(&[
            ("personas.json", PERSONAS),
            ("midgard.json", &console(&rows)),
        ]);
        assert!(
            matches!(body_of(&dir), Err(BuildError::Number { ref text, .. }) if text == bad),
            "{bad} was accepted"
        );
    }
    for good in [
        "0",
        "100",
        "-2.5",
        "0.000001",
        "1e-7",
        "1.5e-7",
        "1e+21",
        "123456789012345680000",
    ] {
        let rows = format!(r#"[{{"id":"1","item":{{"n":{good}}}}}]"#);
        let dir = fixtures(&[
            ("personas.json", PERSONAS),
            ("midgard.json", &console(&rows)),
        ]);
        body_of(&dir).unwrap_or_else(|e| panic!("{good} refused: {e}"));
    }
}

#[test]
fn the_number_spelling_is_the_one_json_stringify_gives() {
    for (v, want) in [
        (0.0, "0"),
        (-0.0, "0"),
        (100.0, "100"),
        (0.1, "0.1"),
        (-2.5, "-2.5"),
        (1e21, "1e+21"),
        (1e20, "100000000000000000000"),
        (0.000001, "0.000001"),
        (0.0000001, "1e-7"),
        (1.5e-7, "1.5e-7"),
        (123.456, "123.456"),
        (1.7976931348623157e308, "1.7976931348623157e+308"),
        (5e-324, "5e-324"),
        (53.12345, "53.12345"),
    ] {
        assert_eq!(build::number_text(v).as_deref(), Some(want), "{v:e}");
    }
    assert_eq!(build::number_text(f64::INFINITY), None);
    assert_eq!(build::number_text(f64::NAN), None);
}

#[test]
fn missing_or_misshapen_pieces_are_refused() {
    // No personas file.
    let dir = fixtures(&[("midgard.json", &console("[]"))]);
    assert!(matches!(body_of(&dir), Err(BuildError::Shape(_))));
    // A console file without collections.
    let dir = fixtures(&[
        ("personas.json", PERSONAS),
        ("midgard.json", r#"{"console":"midgard"}"#),
    ]);
    assert!(matches!(body_of(&dir), Err(BuildError::Shape(_))));
    // The same collection in two files.
    let dir = fixtures(&[
        ("personas.json", PERSONAS),
        ("midgard.json", &console("[]")),
        ("other.json", &console("[]")),
    ]);
    assert!(matches!(body_of(&dir), Err(BuildError::Shape(_))));
    // A listed file that is not there.
    let dir = fixtures(&[
        ("personas.json", PERSONAS),
        ("midgard.json", &console("[]")),
    ]);
    fs::remove_file(dir.join("midgard.json")).unwrap();
    assert!(matches!(body_of(&dir), Err(BuildError::Io(_))));
    // A manifest naming a file outside the directory.
    let dir = fixtures(&[("personas.json", PERSONAS)]);
    let m = fs::read_to_string(dir.join("manifest.json")).unwrap();
    fs::write(
        dir.join("manifest.json"),
        m.replace(r#""personas.json""#, r#""../personas.json""#),
    )
    .unwrap();
    assert!(matches!(body_of(&dir), Err(BuildError::Shape(_))));
    // Not a fixtures manifest.
    let dir = fixtures(&[
        ("personas.json", PERSONAS),
        ("midgard.json", &console("[]")),
    ]);
    let m = fs::read_to_string(dir.join("manifest.json")).unwrap();
    fs::write(
        dir.join("manifest.json"),
        m.replace("midgard-demo-fixtures", "something-else"),
    )
    .unwrap();
    assert!(matches!(body_of(&dir), Err(BuildError::Shape(_))));
}

#[test]
fn inputs_come_only_from_the_release_mocks_and_the_committed_demo_sources() {
    assert_eq!(
        build::input_origin("sdk/ts/src/mock/heimdall.ts"),
        Ok(InputOrigin::Tagged)
    );
    assert_eq!(
        build::input_origin("clients/demo/overlay.json"),
        Ok(InputOrigin::Committed)
    );
    assert_eq!(
        build::input_origin("clients/demo/tools/extract-fixtures.mjs"),
        Ok(InputOrigin::Committed)
    );
    for bad in [
        "sdk/ts/src/live/feeds.ts",
        "services/gateway/data.json",
        "clients/demo/../../services/x.json",
        "sdk/ts/src/mock/../../../../etc/passwd",
        "/clients/demo/overlay.json",
        "./clients/demo/overlay.json",
        "clients/demo//overlay.json",
        "clients/demo/",
        "sdk/ts/src/mock",
        "clients\\demo\\overlay.json",
        "",
    ] {
        assert!(build::input_origin(bad).is_err(), "{bad:?} accepted");
    }
}
