//! The demo dataset may hold only the release's mock fixtures: local-suite data is
//! refused before anything is signed. [NFR-PRV-1] [AC-C-13]

use attest::json::parse;
use midgard_client_core::demo::guard::{check, GuardError};

#[test]
fn the_release_fixture_shapes_pass() {
    let v = parse(r#"{"collections":{"heimdall.motion":{"rows":[{"item":{"samples":[{"lat":53.4,"lon":-6.2,"t":0}]}}]}},
                      "note":"NORTH-2 radar"}"#).unwrap();
    assert_eq!(check(&v), Ok(()));
}

#[test]
fn a_leak_archive_or_live_feed_marker_is_refused_wherever_it_is() {
    for text in [
        "ICIJ Offshore Leaks node 12000",
        "source: opensky-network.org",
        "Port Louis harbour",
    ] {
        let v = parse(&format!(
            r#"{{"rows":[{{"item":{{"title":{:?}}}}}]}}"#,
            text
        ))
        .unwrap();
        let errs = check(&v).unwrap_err();
        assert!(
            matches!(&errs[0], GuardError::Marker { path, .. } if path == "$.rows[0].item.title"),
            "{errs:?}"
        );
    }
}

#[test]
fn a_position_in_the_mauritius_area_is_refused() {
    let v = parse(r#"{"samples":[{"lat":-20.16,"lon":57.5}]}"#).unwrap();
    assert!(
        matches!(&check(&v).unwrap_err()[0], GuardError::Area { area, .. } if *area == "Mauritius EEZ")
    );
}

#[test]
fn a_marker_in_a_key_and_the_long_coordinate_names_are_refused_too() {
    let v = parse(
        r#"{"collections":{"aisstream.ships":{"rows":[{"latitude":-20.3,"longitude":63.4}]}}}"#,
    )
    .unwrap();
    let errs = check(&v).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, GuardError::Marker { path, marker }
        if path == "$.collections.aisstream.ships" && *marker == "aisstream")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| matches!(e, GuardError::Area { path, .. }
        if path == "$.collections.aisstream.ships.rows[0]")),
        "{errs:?}"
    );
}

#[test]
fn every_committed_fixture_passes_the_guard() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../demo/fixtures/0.1.0-rc.3");
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let v = parse(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(check(&v), Ok(()), "{}", p.display());
        seen += 1;
    }
    assert!(
        seen >= 8,
        "only {seen} fixture files under {}",
        dir.display()
    );
}
