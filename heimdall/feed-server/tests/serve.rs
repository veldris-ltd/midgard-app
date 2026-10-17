//! `heimdall-feed` and `heimdall-source` end to end, as real processes on real sockets:
//! certified sources submit signed observations that fuse into one tracked entity; an
//! observation for another feed, one signed with an uncertified key, a submission by
//! an analyst and a read by a source are all refused and audited; the analyst reads
//! the fused picture; and after a hard kill the sealed picture and the ledger are
//! recovered and fusion continues on the recovered tracks.
//! [FR-H-1] [FR-H-3] [NFR-PQ-3] [RC-13] [NFR-SEC-1]
#![cfg(feature = "backend-fips")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

use attest::Keystore;
use heimdall_feed_gateway::{encode_signed_observation, sign_observation};
use heimdall_feed_server::{outcome, INGEST, READ_HISTORY};
use heimdall_picture::{FeedId, Observation};
use iam::{Idp, IDP_SIG_ALG};
use policy::Role;
use pq_mtls::{encode_anchor, encode_cert, ServiceId, WorkloadIdentity};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::{OsRng, SignatureScheme};
use service_runtime::json::{parse, Json};
use service_runtime::{client, ServiceIdentity};

const POLICY: &str = r#"{
  "version": 1,
  "rules": [
    { "id": "source-channel", "role": "feed-source", "action": "OpenChannel",
      "resource": "*", "effect": "permit" },
    { "id": "source-ingest", "role": "feed-source", "action": "Ingest",
      "resource": "*", "effect": "permit" },
    { "id": "analyst-channel", "role": "picture-reader", "action": "OpenChannel",
      "resource": "*", "effect": "permit" },
    { "id": "analyst-read", "role": "picture-reader", "action": "ReadPicture",
      "resource": "*", "effect": "permit" },
    { "id": "analyst-history", "role": "picture-reader", "action": "ReadHistory",
      "resource": "*", "effect": "permit" }
  ]
}"#;

const PURPOSE: &str = "situational-awareness";

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "heimdall-feed-server-{tag}-{}-{uniq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// What `mg-admin mesh issue` writes, for `service` under `idp`.
fn issue(idp: &Idp, dir: &Path, service: &str, roles: &[&str]) {
    std::fs::create_dir_all(dir).unwrap();
    let key = Keystore::generate(service, IDP_SIG_ALG, &mut OsRng).unwrap();
    let audit = Keystore::generate(&format!("{service}-audit"), IDP_SIG_ALG, &mut OsRng).unwrap();
    let cert = idp
        .certify(&WorkloadIdentity {
            id: ServiceId(service.into()),
            roles: roles.iter().map(|r| Role(r.to_string())).collect(),
            sig_alg: IDP_SIG_ALG,
            verifying_key: key.verifying_key.clone(),
        })
        .unwrap();
    key.write(&dir.join(format!("{service}.keystore"))).unwrap();
    audit
        .write(&dir.join(format!("{service}.audit.keystore")))
        .unwrap();
    std::fs::write(dir.join(format!("{service}.cert")), encode_cert(&cert)).unwrap();
    std::fs::write(dir.join("mesh.anchor"), encode_anchor(&idp.trust_anchor())).unwrap();
}

/// A field of a JSON log line (the sink writes every field as a string).
fn field(line: &str, key: &str) -> Option<String> {
    let start = line.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let len = line[start..].find('"')?;
    Some(line[start..start + len].to_string())
}

struct Running {
    child: Child,
    mesh: String,
    health: String,
    recovered: String,
    entities: String,
    classifier: String,
    history_points: String,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(identity: &Path, state: &Path) -> Running {
    start_with(identity, state, &[])
}

fn start_with(identity: &Path, state: &Path, extra: &[&str]) -> Running {
    let mut child = Command::new(env!("CARGO_BIN_EXE_heimdall-feed"))
        .args([
            "--identity-dir",
            &identity.display().to_string(),
            "--state-dir",
            &state.display().to_string(),
            "--bind",
            "127.0.0.1:0",
            "--health-bind",
            "127.0.0.1:0",
        ])
        .args(extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("heimdall-feed starts");
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let serving = loop {
        match lines.next() {
            Some(Ok(line)) if line.contains("\"message\":\"serving\"") => break line,
            Some(_) => continue,
            None => {
                let mut stderr = String::new();
                child
                    .stderr
                    .take()
                    .unwrap()
                    .read_to_string(&mut stderr)
                    .unwrap();
                panic!("heimdall-feed exited before serving: {stderr}");
            }
        }
    };
    std::thread::spawn(move || lines.for_each(drop));
    Running {
        child,
        mesh: field(&serving, "mesh").expect("mesh address"),
        health: field(&serving, "health").expect("health address"),
        recovered: field(&serving, "recovered_audit_entries").expect("recovered count"),
        entities: field(&serving, "entities").expect("entity count"),
        classifier: field(&serving, "classifier").expect("classifier"),
        history_points: field(&serving, "history_points").expect("history point count"),
    }
}

fn source(dir: &Path, service: &str, addr: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_heimdall-source"))
        .args([
            "--identity-dir",
            &dir.display().to_string(),
            "--service",
            service,
            "--addr",
            addr,
        ])
        .args(args)
        .output()
        .expect("heimdall-source runs")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn picture(dir: &Path, addr: &str) -> Json {
    let out = source(dir, "analyst", addr, &["--picture"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    parse(text(&out.stdout).trim()).expect("the picture is JSON")
}

fn entity<'a>(picture: &'a Json, id: &str) -> &'a Json {
    match picture.get("entities") {
        Some(Json::Arr(items)) => items
            .iter()
            .find(|e| e.str_field("id") == Some(id))
            .unwrap_or_else(|| panic!("{id} is not in {}", picture.encode())),
        _ => panic!("no entities in {}", picture.encode()),
    }
}

fn get(addr: &str, path: &str) -> u16 {
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: probe\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap()
}

#[test]
fn sources_feed_the_picture_analysts_read_it_and_it_survives_a_restart() {
    let root = scratch("e2e");
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let server_identity = root.join("server");
    let clients = root.join("clients");
    let state = root.join("state");
    issue(&idp, &server_identity, "heimdall-feed", &["feed-gateway"]);
    issue(&idp, &clients, "radar-1", &["feed-source"]);
    issue(&idp, &clients, "radar-2", &["feed-source"]);
    issue(&idp, &clients, "analyst", &["picture-reader"]);
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("policy.json"), POLICY).unwrap();

    let server = start(&server_identity, &state);
    assert_eq!(server.recovered, "0");
    assert_eq!(get(&server.health, "/readyz"), 200);

    let observe = |service: &str, at: &str, attrs: &str, extra: &[&str]| {
        let mut args = vec![
            "--kind",
            "air-track",
            "--entity",
            "AC-1",
            "--observed-at",
            at,
            "--attrs",
            attrs,
        ];
        args.extend_from_slice(extra);
        source(&clients, service, &server.mesh, &args)
    };

    let first = observe("radar-1", "100", "speed=420,callsign=ALPHA1", &[]);
    assert!(first.status.success(), "{}", text(&first.stderr));
    assert!(
        text(&first.stdout).contains("fused into entity AC-1 at audit sequence 0"),
        "{}",
        text(&first.stdout)
    );
    let second = observe("radar-2", "110", "speed=430", &[]);
    assert!(second.status.success(), "{}", text(&second.stderr));

    let impersonation = observe("radar-2", "111", "speed=999", &["--feed-id", "radar-1"]);
    assert_eq!(impersonation.status.code(), Some(1));
    assert!(
        text(&impersonation.stderr).contains("only its own feed"),
        "{}",
        text(&impersonation.stderr)
    );

    // Signed by a key no certificate binds, sent over radar-1's own channel.
    let (_, rogue_key) = FipsSig::new(IDP_SIG_ALG).keypair(&mut OsRng).unwrap();
    let forged = sign_observation(
        &FipsSig::new(IDP_SIG_ALG),
        &rogue_key,
        Observation::new("forged", FeedId("radar-1".into()), "air-track", "AC-1", 112),
    )
    .unwrap();
    let radar_1 = ServiceIdentity::load(&clients, "radar-1").unwrap();
    let response = client::call(
        &radar_1,
        &server.mesh,
        PURPOSE,
        INGEST,
        &encode_signed_observation(&forged),
    )
    .unwrap();
    assert_eq!(response[0], outcome::DENIED);
    assert!(
        text(&response[1..]).contains("rejected"),
        "{}",
        text(&response[1..])
    );

    let analyst_ingest = observe("analyst", "113", "speed=1", &[]);
    assert_eq!(
        analyst_ingest.status.code(),
        Some(1),
        "an analyst is not a source"
    );
    let source_read = source(&clients, "radar-1", &server.mesh, &["--picture"]);
    assert_eq!(
        source_read.status.code(),
        Some(1),
        "a source cannot read the picture"
    );

    let fused = picture(&clients, &server.mesh);
    let ac_1 = entity(&fused, "AC-1");
    assert_eq!(
        ac_1.get("observations"),
        Some(&Json::Int(2)),
        "{}",
        fused.encode()
    );
    assert_eq!(ac_1.get("lastSeen"), Some(&Json::Int(110)));
    assert_eq!(
        ac_1.get("sources"),
        Some(&Json::Arr(vec![Json::s("radar-1"), Json::s("radar-2")]))
    );
    let attributes = ac_1.get("attributes").expect("attributes");
    assert_eq!(attributes.get("speed"), Some(&Json::Int(430)));

    // A hard kill. Audited so far: two submissions, the impersonation and the forgery
    // (each a decision and a refusal), the analyst's submission, the source's read and
    // the analyst's read.
    drop(server);
    let server = start(&server_identity, &state);
    assert_eq!(server.recovered, "9");
    assert_eq!(server.entities, "1");

    let third = source(
        &clients,
        "radar-1",
        &server.mesh,
        &[
            "--kind",
            "air-track",
            "--entity",
            "AC-1",
            "--observed-at",
            "120",
        ],
    );
    assert!(third.status.success(), "{}", text(&third.stderr));
    assert!(text(&third.stdout).contains("audit sequence 9"));
    let fused = picture(&clients, &server.mesh);
    assert_eq!(
        entity(&fused, "AC-1").get("observations"),
        Some(&Json::Int(3)),
        "fusion continues on the recovered track"
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

const RULES: &str = r#"{ "version": 1, "rules": [
  { "label": "emergency", "rationale": "squawk 7700", "when": { "attr": "squawk", "equals": "7700" } },
  { "label": "fast-mover", "rationale": "over 400 kt", "when": { "attr": "speed", "gt": 400 } }
] }"#;

/// A batch over one session; a replayed observation refused and audited; deployment
/// rules classify; a track not seen within the time-to-live leaves the picture; a text
/// attribute keeps its leading zero. [FR-H-1] [FR-H-2] [FR-H-3]
#[test]
fn a_batch_fuses_over_one_session_replays_are_refused_and_stale_tracks_expire() {
    let root = scratch("batch");
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let server_identity = root.join("server");
    let clients = root.join("clients");
    let state = root.join("state");
    issue(&idp, &server_identity, "heimdall-feed", &["feed-gateway"]);
    issue(&idp, &clients, "adsb", &["feed-source"]);
    issue(&idp, &clients, "analyst", &["picture-reader"]);
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("policy.json"), POLICY).unwrap();
    std::fs::write(state.join("rules.json"), RULES).unwrap();

    let server = start_with(&server_identity, &state, &["--track-ttl", "300"]);
    assert_eq!(server.classifier, "rules.json (2 rules)");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // The stale track arrives first, so the first sweep (on the first ingest) removes it.
    let lines = [
        format!(
            r#"{{"entity": "STALE", "kind": "air-track", "observedAt": {}}}"#,
            now - 1000
        ),
        format!(
            r#"{{"entity": "EMRG", "kind": "air-track", "observedAt": {now}, "attrs": {{"squawk": "7700", "speed": 180.5}}}}"#
        ),
        format!(
            r#"{{"entity": "JET", "kind": "air-track", "observedAt": {now}, "attrs": {{"squawk": "0123", "speed": 446.6, "on_ground": false}}}}"#
        ),
        format!(
            r#"{{"entity": "JET", "kind": "air-track", "observedAt": {now}, "attrs": {{"speed": 999}}}}"#
        ),
        r#"{"entity": "", "kind": "air-track"}"#.to_string(),
    ];
    let file = root.join("batch.jsonl");
    std::fs::write(&file, lines.join("\n")).unwrap();

    let out = source(
        &clients,
        "adsb",
        &server.mesh,
        &["--batch", &file.display().to_string()],
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "one replay and one bad line fail the batch"
    );
    let summary = text(&out.stderr);
    assert!(
        summary.contains("batch: 3 accepted, 1 refused, 1 unreadable"),
        "{summary}"
    );
    assert!(summary.contains("was already fused"), "{summary}");

    let fused = picture(&clients, &server.mesh);
    let ids: Vec<&str> = match fused.get("entities") {
        Some(Json::Arr(items)) => items.iter().filter_map(|e| e.str_field("id")).collect(),
        _ => panic!("{}", fused.encode()),
    };
    assert_eq!(
        ids,
        ["EMRG", "JET"],
        "the stale track expired; the replay did not fuse"
    );
    let emergency = entity(&fused, "EMRG");
    assert_eq!(
        emergency
            .get("classification")
            .and_then(|c| c.str_field("label")),
        Some("emergency")
    );
    let jet = entity(&fused, "JET");
    assert_eq!(
        jet.get("classification").and_then(|c| c.str_field("label")),
        Some("fast-mover")
    );
    assert_eq!(jet.get("observations"), Some(&Json::Int(1)));
    let attributes = jet.get("attributes").expect("attributes");
    assert_eq!(
        attributes.get("squawk"),
        Some(&Json::s("0123")),
        "text keeps its leading zero"
    );
    assert_eq!(attributes.get("on_ground"), Some(&Json::Bool(false)));

    // Recovery: the sealed picture holds the expiry, and the ledger the sweep and the
    // refusal (three submissions, the replay's decision and refusal, the sweep, a read).
    drop(server);
    let server = start_with(&server_identity, &state, &["--track-ttl", "300"]);
    assert_eq!(server.entities, "2");
    assert_eq!(server.recovered, "7");
    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

/// A track read back: its id, layer and points.
type ReadTrack = (String, String, Vec<Json>);

/// Read every page of a history query over one session: the pages' raw sizes and the
/// merged points per track (`[t, lat, lon, alt]`), in the order served.
fn read_history_pages(session: &mut client::Session, query: &str) -> (Vec<usize>, Vec<ReadTrack>) {
    let mut sizes = Vec::new();
    let mut tracks: Vec<ReadTrack> = Vec::new();
    let mut after = Json::Null;
    loop {
        let mut q = parse(query).unwrap();
        if let Json::Obj(pairs) = &mut q {
            pairs.push(("after".into(), after.clone()));
        }
        let response = session.call(READ_HISTORY, q.encode().as_bytes()).unwrap();
        assert_eq!(response[0], outcome::ACCEPTED, "{}", text(&response[1..]));
        sizes.push(response.len());
        let page = parse(&text(&response[1..])).expect("a history page is JSON");
        let Some(Json::Arr(items)) = page.get("tracks") else {
            panic!("{}", page.encode())
        };
        for track in items {
            let id = track.str_field("id").unwrap().to_string();
            let layer = track.str_field("layer").unwrap().to_string();
            let Some(Json::Arr(points)) = track.get("points") else {
                panic!("{}", track.encode())
            };
            match tracks.iter_mut().find(|(i, _, _)| *i == id) {
                Some((_, _, all)) => all.extend(points.iter().cloned()),
                None => tracks.push((id, layer, points.clone())),
            }
        }
        match page.get("next") {
            Some(Json::Null) => break,
            Some(next) => after = next.clone(),
            None => panic!("no next in {}", page.encode()),
        }
    }
    (sizes, tracks)
}

fn t_of(point: &Json) -> i64 {
    match point {
        Json::Arr(v) => v[0].as_i64().unwrap(),
        _ => panic!("{}", point.encode()),
    }
}

/// Positions fused over time become track history: an analyst reads both tracks'
/// points in time order, paging when the history outgrows a response, decimated on
/// request; a source may not read it; a bad window is refused; every read is audited;
/// and the history survives a restart. [FR-H-1] [RC-13] [NFR-SEC-1]
#[test]
fn positions_become_track_history_that_pages_is_audited_and_survives_a_restart() {
    let root = scratch("history");
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let server_identity = root.join("server");
    let clients = root.join("clients");
    let state = root.join("state");
    issue(&idp, &server_identity, "heimdall-feed", &["feed-gateway"]);
    issue(&idp, &clients, "adsb", &["feed-source"]);
    issue(&idp, &clients, "analyst", &["picture-reader"]);
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("policy.json"), POLICY).unwrap();
    let flags = ["--history-page-bytes", "600", "--history-retention", "7200"];

    let server = start_with(&server_identity, &state, &flags);
    assert_eq!(server.history_points, "0");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // Two aircraft, 20 positions each, ten seconds apart; the second in reverse
    // order of arrival. Plus one observation with no position.
    let mut lines = Vec::new();
    for i in 0..20u64 {
        let t = now - 200 + i * 10;
        lines.push(format!(
            r#"{{"entity": "AC-1", "kind": "air-track", "observedAt": {t}, "attrs": {{"lat": {}, "lon": -1.25, "alt_ft": 10000}}}}"#,
            51.0 + i as f64 * 0.01
        ));
        let t = now - 10 - i * 10;
        lines.push(format!(
            r#"{{"entity": "SHIP-2", "kind": "sea-track", "observedAt": {t}, "attrs": {{"lat": 50.123456, "lon": {}}}}}"#,
            -2 - i as i64
        ));
    }
    lines.push(format!(
        r#"{{"entity": "AC-1", "kind": "air-track", "observedAt": {}, "attrs": {{"speed": 400}}}}"#,
        now - 5
    ));
    let file = root.join("positions.jsonl");
    std::fs::write(&file, lines.join("\n")).unwrap();
    let out = source(
        &clients,
        "adsb",
        &server.mesh,
        &["--batch", &file.display().to_string()],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));

    let analyst = ServiceIdentity::load(&clients, "analyst").unwrap();
    let mut session = client::Session::open(&analyst, &server.mesh, PURPOSE).unwrap();
    let window = format!(r#"{{"from": {}, "to": {now}}}"#, now - 300);
    let (sizes, tracks) = read_history_pages(&mut session, &window);
    assert!(
        sizes.len() > 2,
        "a 600-byte budget pages ({} pages)",
        sizes.len()
    );
    assert!(sizes.iter().all(|s| *s < 600 + 200), "{sizes:?}");
    let ids: Vec<&str> = tracks.iter().map(|(id, _, _)| id.as_str()).collect();
    assert_eq!(ids, ["AC-1", "SHIP-2"]);
    for (id, layer, points) in &tracks {
        assert_eq!(points.len(), 20, "{id}");
        let times: Vec<i64> = points.iter().map(t_of).collect();
        assert!(times.windows(2).all(|w| w[0] < w[1]), "{id} {times:?}");
        assert_eq!(layer, if id == "AC-1" { "air" } else { "sea" });
    }
    let first_ac = tracks[0].2[0].encode();
    assert_eq!(first_ac, format!("[{},51,-1.25,3048]", now - 200));
    let first_ship = tracks[1].2[0].encode();
    assert_eq!(first_ship, format!("[{},50.12346,-21,null]", now - 200));

    // Decimated to a point a minute, one layer.
    let (decimated_pages, decimated) = read_history_pages(
        &mut session,
        &format!(
            r#"{{"from": {}, "to": {now}, "step": 60, "layer": "air"}}"#,
            now - 300
        ),
    );
    assert_eq!(decimated.len(), 1);
    let times: Vec<i64> = decimated[0].2.iter().map(t_of).collect();
    let start = (now - 200) as i64;
    assert_eq!(times, [start, start + 60, start + 120, start + 180]);

    // Refusals: `to` before `from`, a window over the retention, and a source.
    let refused = session
        .call(
            READ_HISTORY,
            format!(r#"{{"from": {now}, "to": 1}}"#).as_bytes(),
        )
        .unwrap();
    assert_eq!(refused[0], outcome::MALFORMED, "{}", text(&refused[1..]));
    let too_long = session
        .call(
            READ_HISTORY,
            format!(r#"{{"from": 0, "to": {now}}}"#).as_bytes(),
        )
        .unwrap();
    assert_eq!(too_long[0], outcome::MALFORMED, "{}", text(&too_long[1..]));
    assert!(
        text(&too_long[1..]).contains("kept for 7200"),
        "{}",
        text(&too_long[1..])
    );
    let adsb = ServiceIdentity::load(&clients, "adsb").unwrap();
    let denied = client::call(
        &adsb,
        &server.mesh,
        PURPOSE,
        READ_HISTORY,
        window.as_bytes(),
    )
    .unwrap();
    assert_eq!(denied[0], outcome::DENIED, "a source cannot read history");
    drop(session);

    // Every read was audited once: 41 submissions, the history pages, the decimated
    // pages, two malformed reads and the source's refused read.
    drop(server);
    let server = start_with(&server_identity, &state, &flags);
    let audited = 41 + sizes.len() + decimated_pages.len() + 3;
    assert_eq!(server.recovered, audited.to_string());
    assert_eq!(server.history_points, "40", "history is recovered");
    let mut session = client::Session::open(&analyst, &server.mesh, PURPOSE).unwrap();
    let (_, recovered) = read_history_pages(
        &mut session,
        &format!(
            r#"{{"from": {}, "to": {now}, "entity": "SHIP-2"}}"#,
            now - 300
        ),
    );
    assert_eq!(recovered.len(), 1);
    assert_eq!(
        recovered[0].2, tracks[1].2,
        "the same points after the restart"
    );
    drop(session);
    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}
