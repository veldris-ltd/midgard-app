//! The PostgreSQL connector against an in-process server that speaks the v3 protocol
//! subset it uses, over a real socket: a table syncs into a typed dataset after a
//! mutual SCRAM-SHA-256 exchange, inside a read-only transaction, in batches; an
//! incremental sync resumes from its checkpoint with the watermark bound as a
//! parameter; and a wrong password, a server that wants MD5 or no authentication, a
//! server that cannot prove the verifier, schema drift, a server error and an egress
//! the sandbox does not permit all fail closed. [FR-F-2] [NFR-SEC-1] [NFR-PRV-1]
#![cfg(all(feature = "backend-fips", feature = "connector-db"))]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use forge_connector_sdk::connectors::postgres::PostgresConnector;
use forge_connector_sdk::wire::{base64_decode, base64_encode, pbkdf2_hmac_sha256};
use forge_connector_sdk::{
    Connector, ConnectorError, ConnectorKind, CredentialRef, CredentialResolver, SandboxPolicy,
    Secret, SyncContext,
};
use forge_dataset_store::{Column, ColumnType, DataValue, Schema};
use pqc_agility::hash::{hmac_sha256, sha256};

const PASSWORD: &str = "correct horse battery";
const SALT: &[u8] = b"midgard-pg-salt!";

struct Password(&'static str);

impl CredentialResolver for Password {
    fn resolve(&self, cref: &CredentialRef) -> Result<Secret, ConnectorError> {
        assert_eq!(cref.0, "pg://reader");
        Ok(Secret::new(self.0.as_bytes().to_vec()))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Auth {
    Scram,
    ScramWithAWrongVerifier,
    Md5,
    Trust,
}

#[derive(Clone)]
struct Behaviour {
    auth: Auth,
    /// The result's columns: name and type oid.
    columns: Vec<(&'static str, i32)>,
    /// Refuse the query with this SQLSTATE and message.
    fail: Option<(&'static str, &'static str)>,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            auth: Auth::Scram,
            columns: vec![("id", 20), ("name", 25), ("active", 16), ("score", 701)],
            fail: None,
        }
    }
}

/// The table: id, name, active, score.
const TABLE: [[Option<&str>; 4]; 3] = [
    [Some("1"), Some("alpha"), Some("t"), Some("1.5")],
    [Some("2"), Some("beta"), Some("f"), None],
    [Some("3"), Some("gamma"), Some("t"), Some("7")],
];

fn value(row: &[Option<&'static str>; 4], column: &str) -> Option<&'static str> {
    match column {
        "id" => row[0],
        "name" | "callsign" => row[1],
        "active" => row[2],
        "score" => row[3],
        _ => None,
    }
}

struct Fake {
    addr: String,
    log: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

fn serve(behaviour: Behaviour) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let log = Arc::new(Mutex::new(Vec::new()));
    let shared = log.clone();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (behaviour, log) = (behaviour.clone(), shared.clone());
            thread::spawn(move || {
                let _ = session(stream, &behaviour, &log);
            });
        }
    });
    Fake { addr, log }
}

fn read_msg(s: &mut TcpStream) -> std::io::Result<(u8, Vec<u8>)> {
    let mut head = [0u8; 5];
    s.read_exact(&mut head)?;
    let len = i32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
    let mut body = vec![0u8; len - 4];
    s.read_exact(&mut body)?;
    Ok((head[0], body))
}

fn send(s: &mut TcpStream, tag: u8, body: &[u8]) -> std::io::Result<()> {
    let mut frame = vec![tag];
    frame.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
    frame.extend_from_slice(body);
    s.write_all(&frame)
}

fn auth(s: &mut TcpStream, code: i32, data: &[u8]) -> std::io::Result<()> {
    let mut body = code.to_be_bytes().to_vec();
    body.extend_from_slice(data);
    send(s, b'R', &body)
}

fn error(s: &mut TcpStream, code: &str, message: &str) -> std::io::Result<()> {
    let body = format!("SERROR\0C{code}\0M{message}\0\0");
    send(s, b'E', body.as_bytes())
}

fn cstrs(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|&b| b == 0)
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

fn session(mut s: TcpStream, b: &Behaviour, log: &Mutex<Vec<String>>) -> std::io::Result<()> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len)?;
    let mut startup = vec![0u8; i32::from_be_bytes(len) as usize - 4];
    s.read_exact(&mut startup)?;
    let params = cstrs(&startup[4..]);
    log.lock().unwrap().push(format!(
        "startup {}={} {}={}",
        params[0], params[1], params[2], params[3]
    ));

    match b.auth {
        Auth::Md5 => return auth(&mut s, 5, &[1, 2, 3, 4]),
        Auth::Trust => auth(&mut s, 0, &[])?,
        Auth::Scram | Auth::ScramWithAWrongVerifier => {
            auth(&mut s, 10, b"SCRAM-SHA-256\0\0")?;
            let (_, initial) = read_msg(&mut s)?;
            let mechanism_end = initial.iter().position(|&c| c == 0).unwrap();
            let first = String::from_utf8(initial[mechanism_end + 5..].to_vec()).unwrap();
            let bare = first.strip_prefix("n,,").unwrap().to_string();
            let client_nonce = bare
                .split(',')
                .find_map(|a| a.strip_prefix("r="))
                .unwrap()
                .to_string();
            let server_first = format!("r={client_nonce}pgserver,s={},i=4096", base64_encode(SALT));
            auth(&mut s, 11, server_first.as_bytes())?;

            let (_, client_final) = read_msg(&mut s)?;
            let client_final = String::from_utf8(client_final).unwrap();
            let (without_proof, proof) = client_final.rsplit_once(",p=").unwrap();
            let salted = pbkdf2_hmac_sha256(PASSWORD.as_bytes(), SALT, 4096, 32);
            let client_key = hmac_sha256(&salted, b"Client Key");
            let auth_message = format!("{bare},{server_first},{without_proof}");
            let signature = hmac_sha256(&sha256(&client_key), auth_message.as_bytes());
            let expected: Vec<u8> = client_key
                .iter()
                .zip(signature.iter())
                .map(|(k, s)| k ^ s)
                .collect();
            if base64_decode(proof).as_deref() != Some(expected.as_slice()) {
                return error(
                    &mut s,
                    "28P01",
                    "password authentication failed for user \"reader\"",
                );
            }
            let server_key: &[u8] = if b.auth == Auth::ScramWithAWrongVerifier {
                b"Not The Server Key"
            } else {
                b"Server Key"
            };
            let verifier = hmac_sha256(&hmac_sha256(&salted, server_key), auth_message.as_bytes());
            auth(
                &mut s,
                12,
                format!("v={}", base64_encode(&verifier)).as_bytes(),
            )?;
            auth(&mut s, 0, &[])?;
        }
    }
    send(&mut s, b'S', b"server_version\x0016.4\0")?;
    send(&mut s, b'K', &[0, 0, 0, 1, 0, 0, 0, 2])?;
    send(&mut s, b'Z', b"I")?;

    let mut param: Option<String> = None;
    let mut sent = 0usize;
    let mut failed = false;
    loop {
        let Ok((tag, body)) = read_msg(&mut s) else {
            return Ok(());
        };
        if failed && tag != b'S' {
            continue;
        }
        match tag {
            b'Q' => {
                log.lock()
                    .unwrap()
                    .push(format!("simple {}", cstrs(&body)[0]));
                send(&mut s, b'C', b"BEGIN\0")?;
                send(&mut s, b'Z', b"T")?;
            }
            b'P' => {
                log.lock()
                    .unwrap()
                    .push(format!("parse {}", cstrs(&body[1..])[0]));
                send(&mut s, b'1', &[])?;
            }
            b'B' => {
                // portal "", statement "", no parameter formats, then the parameters.
                let count = i16::from_be_bytes([body[4], body[5]]);
                param = (count == 1).then(|| {
                    let len = i32::from_be_bytes([body[6], body[7], body[8], body[9]]) as usize;
                    String::from_utf8(body[10..10 + len].to_vec()).unwrap()
                });
                log.lock().unwrap().push(format!("bind {param:?}"));
                send(&mut s, b'2', &[])?;
            }
            b'D' => {
                if let Some((code, message)) = b.fail {
                    error(&mut s, code, message)?;
                    failed = true;
                    continue;
                }
                let mut description = (b.columns.len() as i16).to_be_bytes().to_vec();
                for (name, oid) in &b.columns {
                    description.extend_from_slice(name.as_bytes());
                    description.push(0);
                    description.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
                    description.extend_from_slice(&oid.to_be_bytes());
                    description.extend_from_slice(&[255, 255, 255, 255, 255, 255, 0, 0]);
                }
                send(&mut s, b'T', &description)?;
            }
            b'E' => {
                let max = i32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
                let after: i64 = param.as_deref().map_or(i64::MIN, |p| p.parse().unwrap());
                let rows: Vec<_> = TABLE
                    .iter()
                    .filter(|r| r[0].unwrap().parse::<i64>().unwrap() > after)
                    .collect();
                log.lock().unwrap().push(format!("execute {max}"));
                let end = if max == 0 {
                    rows.len()
                } else {
                    (sent + max).min(rows.len())
                };
                for row in &rows[sent..end] {
                    let mut data = (b.columns.len() as i16).to_be_bytes().to_vec();
                    for (name, _) in &b.columns {
                        match value(row, name) {
                            Some(v) => {
                                data.extend_from_slice(&(v.len() as i32).to_be_bytes());
                                data.extend_from_slice(v.as_bytes());
                            }
                            None => data.extend_from_slice(&(-1i32).to_be_bytes()),
                        }
                    }
                    send(&mut s, b'D', &data)?;
                }
                sent = end;
                if sent < rows.len() {
                    send(&mut s, b's', &[])?;
                } else {
                    send(&mut s, b'C', format!("SELECT {}\0", rows.len()).as_bytes())?;
                }
            }
            b'H' => {}
            b'S' => {
                failed = false;
                sent = 0;
                send(&mut s, b'Z', b"T")?;
            }
            b'X' => {
                log.lock().unwrap().push("terminate".into());
                return Ok(());
            }
            _ => error(&mut s, "08P01", "unexpected message")?,
        }
    }
}

fn schema() -> Schema {
    Schema::new(vec![
        Column::new("id", ColumnType::Int),
        Column::new("name", ColumnType::Text),
        Column::new("active", ColumnType::Bool),
        Column::nullable("score", ColumnType::Float),
    ])
}

fn connector(addr: &str, query: &str) -> PostgresConnector {
    let (host, port) = addr.rsplit_once(':').unwrap();
    PostgresConnector::new(
        host,
        port.parse().unwrap(),
        "fleet",
        "reader",
        CredentialRef::new("pg://reader"),
        query,
        schema(),
    )
}

const SELECT: &str = "SELECT id, name, active, score FROM aircraft";
const SINCE: &str = "SELECT id, name, active, score FROM aircraft WHERE id > $1 ORDER BY id";

fn failure(result: Result<impl std::fmt::Debug, ConnectorError>) -> String {
    result.expect_err("the sync fails closed").to_string()
}

#[test]
fn a_table_syncs_into_a_typed_dataset_inside_a_read_only_transaction() {
    let fake = serve(Behaviour::default());
    let conn = connector(&fake.addr, SELECT).batch_rows(2);
    let sandbox = SandboxPolicy::air_gapped().permit_host(fake.addr.clone());
    let ctx = SyncContext::new(&Password(PASSWORD), &sandbox);

    assert_eq!(conn.egress_hosts(), [fake.addr.clone()]);
    assert_eq!(conn.descriptor().kind, ConnectorKind::Database);
    let dataset = conn.sync(&ctx).unwrap();
    assert_eq!(dataset.rows().len(), 3);
    assert_eq!(
        dataset.rows()[0],
        [
            DataValue::Int(1),
            DataValue::Text("alpha".into()),
            DataValue::Bool(true),
            DataValue::Float(1.5)
        ]
    );
    assert_eq!(dataset.rows()[1][3], DataValue::Null);

    let log = fake.log();
    assert_eq!(log[0], "startup user=reader database=fleet");
    assert!(
        log.contains(&"simple BEGIN TRANSACTION READ ONLY".to_string()),
        "{log:?}"
    );
    assert!(log.contains(&format!("parse {SELECT}")), "{log:?}");
    assert_eq!(
        log.iter().filter(|l| l.starts_with("execute 2")).count(),
        2,
        "three rows arrive in batches of two: {log:?}"
    );
}

#[test]
fn an_incremental_sync_resumes_from_its_checkpoint() {
    let fake = serve(Behaviour::default());
    let conn = connector(&fake.addr, SINCE).incremental("id", "0");
    let sandbox = SandboxPolicy::air_gapped().permit_host(fake.addr.clone());
    let password = Password(PASSWORD);

    let first = conn
        .sync_incremental(&SyncContext::new(&password, &sandbox))
        .unwrap();
    assert_eq!(first.dataset.rows().len(), 3);
    assert_eq!(first.checkpoint.as_deref(), Some("3"));

    let later = conn
        .sync_incremental(&SyncContext::new(&password, &sandbox).with_checkpoint("1"))
        .unwrap();
    assert_eq!(later.dataset.rows().len(), 2);
    assert_eq!(later.checkpoint.as_deref(), Some("3"));

    let nothing_new = conn
        .sync_incremental(&SyncContext::new(&password, &sandbox).with_checkpoint("3"))
        .unwrap();
    assert!(nothing_new.dataset.rows().is_empty());
    assert_eq!(
        nothing_new.checkpoint.as_deref(),
        Some("3"),
        "the checkpoint holds"
    );

    let binds: Vec<String> = fake
        .log()
        .into_iter()
        .filter(|l| l.starts_with("bind"))
        .collect();
    assert_eq!(
        binds,
        ["bind Some(\"0\")", "bind Some(\"1\")", "bind Some(\"3\")"],
        "the watermark is bound as a parameter, never spliced into the SQL"
    );
}

#[test]
fn authentication_fails_closed() {
    let run = |auth: Auth, password: &'static str| {
        let fake = serve(Behaviour {
            auth,
            ..Behaviour::default()
        });
        let sandbox = SandboxPolicy::air_gapped().permit_host(fake.addr.clone());
        failure(
            connector(&fake.addr, SELECT).sync(&SyncContext::new(&Password(password), &sandbox)),
        )
    };
    let e = run(Auth::Scram, "wrong horse");
    assert!(e.contains("password authentication failed"), "{e}");
    let e = run(Auth::Md5, PASSWORD);
    assert!(e.contains("MD5") && e.contains("SCRAM-SHA-256"), "{e}");
    let e = run(Auth::Trust, PASSWORD);
    assert!(e.contains("without authenticating"), "{e}");
    let e = run(Auth::ScramWithAWrongVerifier, PASSWORD);
    assert!(e.contains("does not verify"), "{e}");
}

#[test]
fn schema_drift_and_server_errors_fail_closed() {
    let run = |behaviour: Behaviour, query: &str| {
        let fake = serve(behaviour);
        let sandbox = SandboxPolicy::air_gapped().permit_host(fake.addr.clone());
        failure(connector(&fake.addr, query).sync(&SyncContext::new(&Password(PASSWORD), &sandbox)))
    };

    let renamed = Behaviour {
        columns: vec![("id", 20), ("callsign", 25), ("active", 16), ("score", 701)],
        ..Behaviour::default()
    };
    let e = run(renamed, SELECT);
    assert!(e.contains("schema drift") && e.contains("callsign"), "{e}");

    let retyped = Behaviour {
        columns: vec![("id", 20), ("name", 25), ("active", 16), ("score", 25)],
        ..Behaviour::default()
    };
    let e = run(retyped, SELECT);
    assert!(e.contains("'score'") && e.contains("text"), "{e}");

    let missing_table = Behaviour {
        fail: Some(("42P01", "relation \"aircraft\" does not exist")),
        ..Behaviour::default()
    };
    let e = run(missing_table, SELECT);
    assert!(e.contains("42P01"), "{e}");

    let fake = serve(Behaviour::default());
    let sandbox = SandboxPolicy::air_gapped().permit_host(fake.addr.clone());
    let e = failure(
        connector(&fake.addr, SELECT)
            .incremental("id", "0")
            .sync(&SyncContext::new(&Password(PASSWORD), &sandbox)),
    );
    assert!(e.contains("$1"), "{e}");
}

#[test]
fn egress_is_checked_before_anything_connects() {
    let fake = serve(Behaviour::default());
    let other = SandboxPolicy::air_gapped().permit_host("db.elsewhere:5432");
    for sandbox in [SandboxPolicy::air_gapped(), other] {
        let e = connector(&fake.addr, SELECT)
            .sync(&SyncContext::new(&Password(PASSWORD), &sandbox))
            .expect_err("denied");
        assert!(matches!(e, ConnectorError::SandboxDenied { .. }), "{e}");
    }
    thread::sleep(Duration::from_millis(100));
    assert!(fake.log().is_empty(), "nothing reached the server");
}
