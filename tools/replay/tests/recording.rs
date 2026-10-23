use midgard_replay::recording::{Event, Recording};
use std::fs;
use std::path::{Path, PathBuf};

/// A fresh temp recording dir, removed again when dropped.
struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn temp_dir(name: &str) -> TempDir {
    let d = std::env::temp_dir().join(format!("replay-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("forge")).unwrap();
    TempDir(d)
}

const OBS: &str = r#"{"entity":"A1","kind":"air-track","observedAt":1792000090,"attrs":{}}"#;

fn write_meta(d: &Path) {
    fs::write(
        d.join("meta.json"),
        r#"{"version":1,"recordedFrom":1792000000,"duration":600}"#,
    )
    .unwrap();
}

/// A recording with valid meta and one valid batch, ready for one broken piece.
fn valid(name: &str) -> TempDir {
    let d = temp_dir(name);
    write_meta(&d);
    fs::write(
        d.join("heimdall.jsonl"),
        format!(r#"{{"offset":0,"feed":"f","observations":[{OBS}]}}"#),
    )
    .unwrap();
    d
}

fn load_err(d: &Path) -> String {
    match Recording::load(d) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e,
    }
}

#[test]
fn loads_batches_and_forge_files_in_offset_order() {
    let d = temp_dir("load");
    fs::write(d.join("meta.json"), r#"{"version":1,"recordedFrom":1792000000,"duration":600,"feeds":["adsb-fi","usgs-quakes"],"forgeFiles":["aircraft.csv"]}"#).unwrap();
    fs::write(d.join("heimdall.jsonl"), concat!(
        r#"{"offset":0,"feed":"usgs-quakes","observations":[{"entity":"Q1","kind":"event-earthquake","observedAt":1792000000,"attrs":{}}]}"#, "\n",
        r#"{"offset":90,"feed":"adsb-fi","observations":[{"entity":"A1","kind":"air-track","observedAt":1792000090,"attrs":{}}]}"#, "\n")).unwrap();
    // Compressed, as recordings ship. Windows has no /usr/bin/gzip for the replay to read
    // it with (an open question for the Windows app), so there it stays plain.
    #[cfg(unix)]
    assert!(std::process::Command::new("/usr/bin/gzip")
        .arg(d.join("heimdall.jsonl"))
        .status()
        .unwrap()
        .success());
    fs::write(
        d.join("forge/000000-aircraft.csv"),
        "key,observed_at\nA1,1792000000\n",
    )
    .unwrap();
    fs::write(
        d.join("forge/000045-aircraft.csv"),
        "key,observed_at\nA1,1792000045\n",
    )
    .unwrap();
    fs::write(d.join("forge/manifest.json"), r#"[{"offset":0,"file":"aircraft.csv","path":"forge/000000-aircraft.csv"},{"offset":45,"file":"aircraft.csv","path":"forge/000045-aircraft.csv"}]"#).unwrap();

    let rec = Recording::load(&d).unwrap();
    assert_eq!((rec.recorded_from, rec.duration), (1792000000, 600));
    let offsets: Vec<(u64, String)> = rec
        .events
        .iter()
        .map(|e| match e {
            Event::Batch { offset, feed, .. } => (*offset, format!("batch {feed}")),
            Event::Forge { offset, file, .. } => (*offset, format!("forge {file}")),
        })
        .collect();
    assert_eq!(
        offsets,
        vec![
            (0, "batch usgs-quakes".into()),
            (0, "forge aircraft.csv".into()),
            (45, "forge aircraft.csv".into()),
            (90, "batch adsb-fi".into())
        ]
    );
}

#[test]
fn loads_a_plain_batches_file_as_observation_lines_skipping_empty_batches() {
    let d = temp_dir("plain");
    write_meta(&d);
    fs::write(
        d.join("heimdall.jsonl"),
        format!(
            "{}\n\n{{\"offset\":7,\"feed\":\"f\",\"observations\":[{OBS}, {OBS}]}}\n",
            r#"{"offset":5,"feed":"empty","observations":[]}"#,
        ),
    )
    .unwrap();
    let rec = Recording::load(&d).unwrap();
    assert_eq!(rec.events.len(), 1);
    match &rec.events[0] {
        Event::Batch {
            offset,
            feed,
            lines,
        } => {
            assert_eq!((*offset, feed.as_str()), (7, "f"));
            assert_eq!(lines, &format!("{OBS}\n{OBS}\n"));
        }
        Event::Forge { .. } => panic!("expected a batch"),
    }
}

#[test]
fn refuses_an_unknown_version() {
    let d = temp_dir("version");
    fs::write(
        d.join("meta.json"),
        r#"{"version":2,"recordedFrom":1,"duration":1}"#,
    )
    .unwrap();
    assert!(load_err(&d).contains("version"));
}

#[test]
fn refuses_a_missing_meta_json() {
    let d = temp_dir("nometa");
    assert!(load_err(&d).contains("meta.json"));
}

#[test]
fn refuses_a_missing_zero_or_negative_duration() {
    for dur in ["", r#","duration":0"#, r#","duration":-5"#] {
        let d = valid("duration");
        fs::write(
            d.join("meta.json"),
            format!(r#"{{"version":1,"recordedFrom":1792000000{dur}}}"#),
        )
        .unwrap();
        assert!(load_err(&d).contains("duration"), "{dur}");
    }
}

#[test]
fn refuses_a_recorded_from_that_is_not_a_unix_time() {
    for from in ["", r#","recordedFrom":5"#, r#","recordedFrom":5000000000"#] {
        let d = valid("from");
        fs::write(
            d.join("meta.json"),
            format!(r#"{{"version":1,"duration":600{from}}}"#),
        )
        .unwrap();
        assert!(load_err(&d).contains("recordedFrom"), "{from}");
    }
}

#[test]
fn refuses_a_recording_with_no_events() {
    let d = temp_dir("noevents");
    write_meta(&d);
    assert!(load_err(&d).contains("no events"));
}

#[test]
fn refuses_observations_that_are_not_a_list_of_objects() {
    for obs in [r#""observations":{}"#, r#""observations":[1]"#, r#""x":1"#] {
        let d = valid("obs");
        fs::write(
            d.join("heimdall.jsonl"),
            format!(r#"{{"offset":0,"feed":"f",{obs}}}"#),
        )
        .unwrap();
        assert!(load_err(&d).contains("heimdall batch 1"), "{obs}");
    }
}

#[test]
fn refuses_an_offset_at_or_past_the_duration() {
    let d = valid("lateb");
    fs::write(
        d.join("heimdall.jsonl"),
        format!(r#"{{"offset":600,"feed":"f","observations":[{OBS}]}}"#),
    )
    .unwrap();
    let e = load_err(&d);
    assert!(e.contains("heimdall batch 1") && e.contains("600"), "{e}");

    let d = valid("latef");
    fs::write(d.join("forge/a.csv"), "k\n").unwrap();
    fs::write(
        d.join("forge/manifest.json"),
        r#"[{"offset":601,"file":"a.csv","path":"forge/a.csv"}]"#,
    )
    .unwrap();
    let e = load_err(&d);
    assert!(e.contains("forge entry 1") && e.contains("601"), "{e}");
}

#[test]
fn refuses_a_corrupt_gzip_with_gzips_message() {
    let d = valid("badgz");
    fs::write(d.join("heimdall.jsonl.gz"), "not gzip").unwrap();
    assert!(load_err(&d).contains("gzip"));
}

#[test]
fn refuses_a_forge_file_name_that_is_not_plain() {
    for name in ["../evil.csv", "sub/a.csv", ".hidden.csv"] {
        let d = valid("forgename");
        fs::write(d.join("forge/a.csv"), "k\n").unwrap();
        fs::write(
            d.join("forge/manifest.json"),
            format!(r#"[{{"offset":0,"file":"{name}","path":"forge/a.csv"}}]"#),
        )
        .unwrap();
        let e = load_err(&d);
        assert!(e.contains(name), "{e}");
    }
}

#[test]
fn refuses_a_forge_path_outside_the_recording() {
    for path in ["../x.csv", "forge/../../x.csv", "/etc/passwd"] {
        let d = valid("forgepath");
        fs::write(
            d.join("forge/manifest.json"),
            format!(r#"[{{"offset":0,"file":"a.csv","path":"{path}"}}]"#),
        )
        .unwrap();
        let e = load_err(&d);
        assert!(e.contains(path), "{e}");
    }
}

#[test]
fn refuses_a_forge_path_that_is_not_a_file() {
    for path in ["forge/missing.csv", "forge"] {
        let d = valid("forgemissing");
        fs::write(
            d.join("forge/manifest.json"),
            format!(r#"[{{"offset":0,"file":"a.csv","path":"{path}"}}]"#),
        )
        .unwrap();
        let e = load_err(&d);
        assert!(e.contains(path), "{e}");
    }
}

#[test]
fn refuses_a_feed_name_that_is_not_plain() {
    for feed in ["../id", "a/b", ".x"] {
        let d = valid("feedname");
        fs::write(
            d.join("heimdall.jsonl"),
            format!(r#"{{"offset":0,"feed":"{feed}","observations":[{OBS}]}}"#),
        )
        .unwrap();
        let e = load_err(&d);
        assert!(e.contains(feed), "{e}");
    }
}

#[test]
fn refuses_negative_offsets() {
    let d = valid("negbatch");
    fs::write(
        d.join("heimdall.jsonl"),
        format!(r#"{{"offset":-5,"feed":"f","observations":[{OBS}]}}"#),
    )
    .unwrap();
    assert!(load_err(&d).contains("offset"));

    let d = valid("negforge");
    fs::write(d.join("forge/a.csv"), "k\n").unwrap();
    fs::write(
        d.join("forge/manifest.json"),
        r#"[{"offset":-1,"file":"a.csv","path":"forge/a.csv"}]"#,
    )
    .unwrap();
    assert!(load_err(&d).contains("offset"));
}
