use midgard_replay::recording::{Event, Recording};
use std::{fs, process::Command};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("replay-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("forge")).unwrap();
    d
}

fn write_meta(d: &std::path::Path) {
    fs::write(
        d.join("meta.json"),
        r#"{"version":1,"recordedFrom":1792000000,"duration":600}"#,
    )
    .unwrap();
}

fn load_err(d: &std::path::Path) -> String {
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
    assert!(Command::new("/usr/bin/gzip")
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
fn refuses_a_forge_file_name_that_is_not_plain() {
    for name in ["../evil.csv", "sub/a.csv", ".hidden.csv"] {
        let d = temp_dir("forgename");
        write_meta(&d);
        fs::write(
            d.join("forge/manifest.json"),
            format!(r#"[{{"offset":0,"file":"{name}","path":"forge/000000-a.csv"}}]"#),
        )
        .unwrap();
        let e = load_err(&d);
        assert!(e.contains(name), "{e}");
    }
}

#[test]
fn refuses_a_forge_path_outside_the_recording() {
    for path in ["../x.csv", "forge/../../x.csv", "/etc/passwd"] {
        let d = temp_dir("forgepath");
        write_meta(&d);
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
        let d = temp_dir("feedname");
        write_meta(&d);
        fs::write(
            d.join("heimdall.jsonl"),
            format!(r#"{{"offset":0,"feed":"{feed}","observations":[]}}"#),
        )
        .unwrap();
        let e = load_err(&d);
        assert!(e.contains(feed), "{e}");
    }
}

#[test]
fn refuses_negative_offsets() {
    let d = temp_dir("negbatch");
    write_meta(&d);
    fs::write(
        d.join("heimdall.jsonl"),
        r#"{"offset":-5,"feed":"f","observations":[]}"#,
    )
    .unwrap();
    assert!(load_err(&d).contains("offset"));

    let d = temp_dir("negforge");
    write_meta(&d);
    fs::write(
        d.join("forge/manifest.json"),
        r#"[{"offset":-1,"file":"a.csv","path":"forge/a.csv"}]"#,
    )
    .unwrap();
    assert!(load_err(&d).contains("offset"));
}
