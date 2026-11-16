//! The demo dataset may hold only the release's mock fixtures: local-suite data is
//! refused before anything is signed. [NFR-PRV-1] [AC-C-13]

use attest::json::parse;
use midgard_client_core::demo::guard::{check, normalise, GuardError};

#[test]
fn the_release_fixture_shapes_pass() {
    let v = parse(r#"{"collections":{"heimdall.motion":{"rows":[{"item":{"samples":[{"lat":53.4,"lon":-2.2,"t":0}]}}]}},
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
        if path == r#"$.collections["aisstream.ships"]"# && *marker == "aisstream")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| matches!(e, GuardError::Area { path, .. }
        if path == r#"$.collections["aisstream.ships"].rows[0]"#)),
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

fn findings(text: &str) -> Vec<GuardError> {
    check(&attest::json::Json::Str(text.to_string()))
        .err()
        .unwrap_or_default()
}

fn has_marker(errs: &[GuardError], want: &str) -> bool {
    errs.iter()
        .any(|e| matches!(e, GuardError::Marker { marker, .. } if *marker == want))
}

fn has_area(errs: &[GuardError], want: &str) -> bool {
    errs.iter()
        .any(|e| matches!(e, GuardError::Area { area, .. } if *area == want))
}

fn has_foreign(errs: &[GuardError]) -> bool {
    errs.iter()
        .any(|e| matches!(e, GuardError::ForeignChar { .. }))
}

#[test]
fn a_disguised_marker_is_folded_and_found() {
    // Decomposed (e + U+0301), full-width, zero-width space, case, doubled space.
    for (text, marker) in [
        ("Agale\u{301}ga", "agalega"),
        ("AGAL\u{c9}GA", "agalega"),
        (
            "\u{ff2d}\u{ff21}\u{ff35}\u{ff32}\u{ff29}\u{ff34}\u{ff29}\u{ff35}\u{ff33}",
            "mauritius",
        ),
        ("Maur\u{200b}itius", "mauritius"),
        ("PORT LOUIS", "port louis"),
        ("port  louis", "port louis"),
        ("Port\u{a0}Louis", "port louis"),
        ("port_louis", "port louis"),
        ("adsb_lol", "adsb.lol"),
        ("live feeds", "live-feeds"),
        ("MIDGARD-REPLAY=none", "midgard_replay"),
    ] {
        let errs = findings(text);
        assert!(has_marker(&errs, marker), "{text:?}: {errs:?}");
    }
}

#[test]
fn a_combining_mark_or_an_invisible_format_char_is_refused_outright() {
    for text in [
        "cafe\u{301}",
        "a\u{200b}b",
        "a\u{feff}b",
        "soft\u{ad}hyphen",
        "x\u{2060}y",
    ] {
        assert!(has_foreign(&findings(text)), "{text:?}");
    }
    assert!(!has_foreign(&findings(
        "plain \u{2014} \u{b7} \u{2192} text"
    )));
}

#[test]
fn every_latin_1_and_extended_a_letter_folds_to_ascii() {
    for cp in (0xc0u32..=0x17f).filter(|c| *c != 0xd7 && *c != 0xf7) {
        let c = char::from_u32(cp).unwrap();
        let folded = normalise(&c.to_string());
        assert!(
            !folded.is_empty() && folded.bytes().all(|b| b.is_ascii_lowercase()),
            "U+{cp:04X} {c} -> {folded:?}"
        );
    }
}

#[test]
fn the_local_suite_fetch_hosts_and_job_names_are_refused() {
    for (text, marker) in [
        (
            "https://api.tfl.gov.uk/Line/Mode/tube/Status",
            "api.tfl.gov.uk",
        ),
        ("data.elexon.co.uk", "data.elexon.co.uk"),
        ("api.carbonintensity.org.uk", "api.carbonintensity.org.uk"),
        ("environment.data.gov.uk", "environment.data.gov.uk"),
        ("data.sensor.community", "data.sensor.community"),
        ("api.open-meteo.com/v1", "open-meteo.com"),
        ("aviationweather.gov", "aviationweather.gov"),
        ("www.gdacs.org", "gdacs.org"),
        ("eonet.gsfc.nasa.gov", "eonet.gsfc.nasa.gov"),
        ("coralreefwatch.noaa.gov", "coralreefwatch.noaa.gov"),
        ("services.swpc.noaa.gov", "services.swpc.noaa.gov"),
        ("www.ncei.noaa.gov", "ncei.noaa.gov"),
        ("erddap.ifremer.fr", "erddap.ifremer.fr"),
        ("euro-argo.eu", "euro-argo.eu"),
        (
            "earth-search.aws.element84.com",
            "earth-search.aws.element84.com",
        ),
        (
            "rapidmapping.emergency.copernicus.eu",
            "rapidmapping.emergency.copernicus.eu",
        ),
        ("worldweather.wmo.int", "worldweather.wmo.int"),
        (
            "cap-sources.s3.amazonaws.com",
            "cap-sources.s3.amazonaws.com",
        ),
        ("ceb.mu outage list", "ceb.mu"),
        ("govmu.org", "govmu.org"),
        ("seismicportal.eu", "seismicportal.eu"),
        ("fetch_live()", "fetch_live"),
        ("live_loop", "live_loop"),
        ("job mu_flights", "mu_"),
        ("MU_SHIPS", "mu_"),
    ] {
        let errs = findings(text);
        assert!(has_marker(&errs, marker), "{text:?}: {errs:?}");
    }
    // `mu_` only at the start of a token.
    for text in ["menu_item", "emu_count", "mu", "mux_a"] {
        assert_eq!(findings(text), vec![], "{text:?}");
    }
}

#[test]
fn coordinates_under_other_names_and_as_numeric_strings_are_refused() {
    for body in [
        r#"{"lat":"-20.16","lon":"57.5"}"#,
        r#"{"Latitude":-20.16,"LNG":57.5}"#,
        r#"{"lat_deg":-20.16,"lon_deg":57.5}"#,
        r#"{"LatDeg":-20.16,"LonDeg":57.5}"#,
        r#"{"lat":-20.16,"long":57.5}"#,
    ] {
        let errs = check(&parse(body).unwrap()).unwrap_err();
        assert!(has_area(&errs, "Mauritius EEZ"), "{body}: {errs:?}");
    }
}

#[test]
fn a_coordinate_array_is_refused_in_either_order() {
    for body in [
        "[57.5,-20.16]",
        "[-20.16,57.5]",
        r#"["57.5","-20.16",0]"#,
        r#"{"geometry":{"coordinates":[57.5,-20.16]}}"#,
    ] {
        let errs = check(&parse(body).unwrap()).unwrap_err();
        assert!(has_area(&errs, "Mauritius EEZ"), "{body}: {errs:?}");
    }
    // An odd run longer than three is not a position; Paris is not in a denied area.
    assert_eq!(check(&parse("[57.5,-20.16,1,2,3]").unwrap()), Ok(()));
    assert_eq!(check(&parse("[48.85,2.35]").unwrap()), Ok(()));
}

#[test]
fn a_coordinate_pair_written_in_a_string_is_refused() {
    for text in [
        "57.5 -20.16",
        "-20.16,57.5",
        "-20.16, 57.5",
        "-20.16; 57.5",
        "POINT(57.5 -20.16)",
        "at \u{2212}20.16 57.5",
    ] {
        assert!(has_area(&findings(text), "Mauritius EEZ"), "{text:?}");
    }
    for text in [
        "2026-11-13",
        "0.1.0-rc.3",
        "1,204",
        "W-0912 -20",
        "lag 0.8s",
    ] {
        assert_eq!(findings(text), vec![], "{text:?}");
    }
}

#[test]
fn the_wider_mauritius_box_and_the_chagos_box_are_refused() {
    for (lat, lon, area) in [
        (-24.5, 52.5, "Mauritius EEZ"),
        (-6.5, 68.0, "Mauritius EEZ"),
        (-7.3, 72.4, "Chagos Archipelago"),
        (-4.0, 74.0, "Chagos Archipelago"),
    ] {
        let v = parse(&format!(r#"{{"lat":{lat},"lon":{lon}}}"#)).unwrap();
        assert!(has_area(&check(&v).unwrap_err(), area), "{lat},{lon}");
    }
    for (lat, lon) in [
        (-26.0, 57.0),
        (-5.0, 57.0),
        (-20.0, 51.0),
        (-2.0, 72.0),
        (-7.0, 75.0),
    ] {
        let v = parse(&format!(r#"{{"lat":{lat},"lon":{lon}}}"#)).unwrap();
        assert_eq!(check(&v), Ok(()), "{lat},{lon}");
    }
}

#[test]
fn a_key_holding_a_dot_or_bracket_is_quoted_in_the_finding_path() {
    let v = parse(r#"{"x.y":{"a[0]":"opensky"}}"#).unwrap();
    let errs = check(&v).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, GuardError::Marker { path, .. }
            if path == r#"$["x.y"]["a[0]"]"#)),
        "{errs:?}"
    );
}

#[test]
fn any_character_outside_the_allowed_set_is_refused() {
    for text in [
        "Maur\u{fe0f}itius",
        "Maur\u{1dc0}itius",
        "Mauri\u{180e}tius",
        "Mau\u{e0041}ritius",
        "M\u{430}uritius",
        "\u{f7}",
    ] {
        assert!(has_foreign(&findings(text)), "{text:?}");
    }
    // A key is checked like a value.
    let v = parse("{\"M\u{430}uritius\":1}").unwrap();
    assert!(has_foreign(&check(&v).unwrap_err()));
    // The symbols the release fixtures use, and folded Latin letters, are allowed.
    for text in ["\u{b1}2 \u{b7} \u{d7} \u{2014} \u{2019} \u{2026} \u{2192} \u{2212}1 \u{2265} \u{26a0} \u{2713}", "Caf\u{e9} \u{141}\u{f3}d\u{17a}"] {
        assert_eq!(findings(text), vec![], "{text:?}");
    }
}

#[test]
fn a_dot_leader_or_squashed_spacing_does_not_hide_a_marker() {
    for (text, marker) in [
        ("seismicportal\u{2024}eu", "seismicportal.eu"),
        ("PortLouis", "port louis"),
        ("p o r t louis", "port louis"),
        ("open-sky network", "opensky"),
    ] {
        let errs = findings(text);
        assert!(has_marker(&errs, marker), "{text:?}: {errs:?}");
    }
}

#[test]
fn every_matching_key_and_the_x_y_names_are_tried_for_a_keyed_position() {
    for body in [
        r#"{"Lat":1,"lat":-20.16,"lon":57.5}"#,
        r#"{"y":-20.16,"x":57.5}"#,
        r#"{"X":"57.5","Y":"-20.16"}"#,
    ] {
        let errs = check(&parse(body).unwrap()).unwrap_err();
        assert!(has_area(&errs, "Mauritius EEZ"), "{body}: {errs:?}");
    }
    // Picture coordinates (0 to 100) are never a denied position.
    assert_eq!(check(&parse(r#"{"x":57.5,"y":20.16}"#).unwrap()), Ok(()));
}

#[test]
fn an_even_run_of_numbers_is_read_as_pairs() {
    for body in [
        "[57.5,-20.16,57.6,-20.2]",
        "[1,2,57.6,-20.2]",
        "[48.85,2.35,-20.2,57.6]",
    ] {
        let errs = check(&parse(body).unwrap()).unwrap_err();
        assert!(has_area(&errs, "Mauritius EEZ"), "{body}: {errs:?}");
    }
    assert_eq!(check(&parse("[48.85,2.35,51.5,-0.12]").unwrap()), Ok(()));
}

#[test]
fn a_pair_with_a_short_label_or_an_exponent_between_is_refused() {
    for text in [
        "lat=-20.16 lon=57.5",
        "-20.16/57.5",
        "-2.016e1, 57.5",
        "57.5E0 -20.16",
    ] {
        assert!(has_area(&findings(text), "Mauritius EEZ"), "{text:?}");
    }
    // More than eight characters apart is not a pair.
    assert_eq!(findings("-20.16 is far from the 57.5"), vec![]);
}

// Keyed positions are read both ways round too; the synthetic Heimdall hour lies
// east of 6 W so that, swapped, it never reads as a Mauritius position.
#[test]
fn a_keyed_position_with_lat_and_lon_swapped_is_refused() {
    let errs = check(&parse(r#"{"lat":57.5,"lon":-20.16}"#).unwrap()).unwrap_err();
    assert!(has_area(&errs, "Mauritius EEZ"), "{errs:?}");
}

#[test]
fn a_foreign_character_inside_a_marker_is_reported_and_the_marker_still_found() {
    for text in [
        "Maur\u{fe0f}itius",
        "Mau\u{e0041}ritius",
        "Mauri\u{180e}tius",
        "Maur\u{1dc0}itius",
        "seismic\u{2063}portal.eu",
    ] {
        let errs = findings(text);
        assert!(has_foreign(&errs), "{text:?}: {errs:?}");
        assert!(
            has_marker(&errs, "mauritius") || has_marker(&errs, "seismicportal.eu"),
            "{text:?}: {errs:?}"
        );
    }
}
