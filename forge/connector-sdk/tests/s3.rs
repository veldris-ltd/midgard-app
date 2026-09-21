//! The S3 connector against an in-process, S3-compatible server that checks every
//! request's Signature Version 4 signature: the objects under a prefix sync into one
//! typed dataset across listing pages, keys that need encoding included; an
//! incremental sync reads only what changed after its checkpoint; and a wrong secret,
//! a malformed object and an egress the sandbox does not permit fail closed.
//! [FR-F-2] [NFR-SEC-1] [NFR-PRV-1]
#![cfg(all(feature = "backend-fips", feature = "connector-objstore"))]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use forge_connector_sdk::connectors::s3::S3Connector;
use forge_connector_sdk::sigv4::{self, Credentials};
use forge_connector_sdk::{
    Connector, ConnectorError, ConnectorKind, CredentialRef, CredentialResolver, SandboxPolicy,
    Secret, SyncContext,
};
use forge_dataset_store::{Column, ColumnType, DataValue, Schema};

const ACCESS_KEY: &str = "forge-reader";
const SECRET_KEY: &str = "s3cr3t/key+for-the-fake";

struct Key(&'static str);

impl CredentialResolver for Key {
    fn resolve(&self, cref: &CredentialRef) -> Result<Secret, ConnectorError> {
        assert_eq!(cref.0, "s3://forge-reader");
        Ok(Secret::new(self.0.as_bytes().to_vec()))
    }
}

type Objects = Arc<Mutex<Vec<(&'static str, &'static str, String)>>>;

struct Fake {
    addr: String,
    objects: Objects,
    log: Arc<Mutex<Vec<String>>>,
}

fn serve(objects: Vec<(&'static str, &'static str, &str)>) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let objects: Objects = Arc::new(Mutex::new(
        objects
            .into_iter()
            .map(|(k, m, b)| (k, m, b.to_string()))
            .collect(),
    ));
    let log = Arc::new(Mutex::new(Vec::new()));
    let (shared_objects, shared_log) = (objects.clone(), log.clone());
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = handle(stream, &shared_objects, &shared_log);
        }
    });
    Fake { addr, objects, log }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/xml\r\n\r\n{body}",
        body.len()
    )
}

fn handle(
    mut stream: TcpStream,
    objects: &Objects,
    log: &Mutex<Vec<String>>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let target = line.split_whitespace().nth(1).unwrap_or("").to_string();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h)?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':').unwrap();
        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
    }
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let (path, query_string) = target.split_once('?').unwrap_or((&target, ""));
    let query: Vec<(String, String)> = query_string
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect();

    // Verify the signature exactly as the service would.
    let signed: Vec<(String, String)> = ["host", "x-amz-content-sha256", "x-amz-date"]
        .iter()
        .map(|h| (h.to_string(), header(h)))
        .collect();
    let expected = sigv4::authorization(
        &sigv4::Request {
            method: "GET",
            path,
            query: &query,
            headers: &signed,
            payload_sha256: &header("x-amz-content-sha256"),
        },
        "eu-west-2",
        "s3",
        &header("x-amz-date"),
        Credentials {
            access_key: ACCESS_KEY,
            secret_key: SECRET_KEY,
        },
    );
    if header("authorization") != expected {
        return respond(
            &mut stream,
            "403 Forbidden",
            "<Error><Code>SignatureDoesNotMatch</Code><Message>The request signature we \
             calculated does not match the signature you provided.</Message></Error>",
        );
    }

    let objects = objects.lock().unwrap();
    if path == "/fleet-exports" {
        let prefix = query
            .iter()
            .find(|(k, _)| k == "prefix")
            .map_or("", |(_, v)| v.as_str());
        let max: usize = query
            .iter()
            .find(|(k, _)| k == "max-keys")
            .map_or(1000, |(_, v)| v.parse().unwrap());
        let start: usize = query
            .iter()
            .find(|(k, _)| k == "continuation-token")
            .map_or(0, |(_, v)| v.parse().unwrap());
        let mut keys: Vec<_> = objects
            .iter()
            .filter(|(k, _, _)| k.starts_with(prefix))
            .collect();
        keys.sort_by_key(|(k, _, _)| *k);
        let page: Vec<_> = keys.iter().skip(start).take(max).collect();
        let truncated = start + page.len() < keys.len();
        log.lock().unwrap().push(format!("list {start}"));
        let mut body = String::from("<ListBucketResult>");
        for (key, modified, _) in &page {
            let key = key.replace('&', "&amp;");
            body.push_str(&format!(
                "<Contents><Key>{key}</Key><LastModified>{modified}</LastModified>\
                 <Size>1</Size></Contents>"
            ));
        }
        body.push_str(&format!("<IsTruncated>{truncated}</IsTruncated>"));
        if truncated {
            body.push_str(&format!(
                "<NextContinuationToken>{}</NextContinuationToken>",
                start + page.len()
            ));
        }
        body.push_str("</ListBucketResult>");
        return respond(&mut stream, "200 OK", &body);
    }
    let key = decode(path.strip_prefix("/fleet-exports/").unwrap_or(""));
    log.lock().unwrap().push(format!("get {key}"));
    match objects.iter().find(|(k, _, _)| *k == key) {
        Some((_, _, body)) => respond(&mut stream, "200 OK", body),
        None => respond(
            &mut stream,
            "404 Not Found",
            "<Error><Code>NoSuchKey</Code><Message>no such key</Message></Error>",
        ),
    }
}

fn schema() -> Schema {
    Schema::new(vec![
        Column::new("tail", ColumnType::Text),
        Column::nullable("hours", ColumnType::Int),
        Column::new("note", ColumnType::Text),
    ])
}

fn connector(addr: &str) -> S3Connector {
    let (host, port) = addr.rsplit_once(':').unwrap();
    S3Connector::new(
        host,
        port.parse().unwrap(),
        "eu-west-2",
        "fleet-exports",
        "aircraft/",
        ACCESS_KEY,
        CredentialRef::new("s3://forge-reader"),
        schema(),
    )
    .page_size(2)
}

fn exports() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "aircraft/2026-09-01.csv",
            "2026-09-01T10:00:00.000Z",
            "tail,hours,note\nMG-101,6412,\"based north, active\"\n",
        ),
        (
            "aircraft/2026-09-02 late & batch.csv",
            "2026-09-02T10:00:00.000Z",
            "tail,hours,note\r\nMG-102,,\"said \"\"hold\"\"\"\r\n",
        ),
        (
            "aircraft/2026-09-03.csv",
            "2026-09-03T10:00:00.000Z",
            "tail,hours,note\nMG-103,120,new\nMG-104,5,\"two\nlines\"\n",
        ),
        (
            "other/ignored.csv",
            "2026-09-04T10:00:00.000Z",
            "tail,hours,note\nX,1,x\n",
        ),
    ]
}

#[test]
fn objects_under_a_prefix_sync_across_pages_into_one_dataset() {
    let fake = serve(exports());
    let conn = connector(&fake.addr);
    let sandbox = SandboxPolicy::air_gapped().permit_host(fake.addr.clone());
    let ctx = SyncContext::new(&Key(SECRET_KEY), &sandbox);

    assert_eq!(conn.descriptor().kind, ConnectorKind::ObjectStore);
    let outcome = conn.sync_incremental(&ctx).unwrap();
    let rows = outcome.dataset.rows();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0][2], DataValue::Text("based north, active".into()));
    assert_eq!(rows[1][1], DataValue::Null);
    assert_eq!(rows[1][2], DataValue::Text("said \"hold\"".into()));
    assert_eq!(rows[3][2], DataValue::Text("two\nlines".into()));
    assert_eq!(
        outcome.checkpoint.as_deref(),
        Some("2026-09-03T10:00:00.000Z aircraft/2026-09-03.csv")
    );

    let log = fake.log.lock().unwrap().clone();
    assert_eq!(
        log,
        [
            "list 0",
            "list 2",
            "get aircraft/2026-09-01.csv",
            "get aircraft/2026-09-02 late & batch.csv",
            "get aircraft/2026-09-03.csv"
        ]
    );
}

#[test]
fn an_incremental_sync_reads_only_what_changed_after_its_checkpoint() {
    let fake = serve(exports());
    let conn = connector(&fake.addr);
    let sandbox = SandboxPolicy::air_gapped().permit_host(fake.addr.clone());
    let key = Key(SECRET_KEY);

    let checkpoint = "2026-09-02T10:00:00.000Z aircraft/2026-09-02 late & batch.csv";
    let later = conn
        .sync_incremental(&SyncContext::new(&key, &sandbox).with_checkpoint(checkpoint))
        .unwrap();
    assert_eq!(later.dataset.rows().len(), 2, "only the third object");

    // Rewriting an older object moves it after the checkpoint.
    fake.objects.lock().unwrap()[0] = (
        "aircraft/2026-09-01.csv",
        "2026-09-05T08:00:00.000Z",
        "tail,hours,note\nMG-101,6500,rewritten\n".to_string(),
    );
    let after_rewrite = conn
        .sync_incremental(
            &SyncContext::new(&key, &sandbox)
                .with_checkpoint("2026-09-03T10:00:00.000Z aircraft/2026-09-03.csv"),
        )
        .unwrap();
    assert_eq!(after_rewrite.dataset.rows().len(), 1);
    assert_eq!(after_rewrite.dataset.rows()[0][1], DataValue::Int(6500));

    let nothing = conn
        .sync_incremental(
            &SyncContext::new(&key, &sandbox)
                .with_checkpoint("2026-09-05T08:00:00.000Z aircraft/2026-09-01.csv"),
        )
        .unwrap();
    assert!(nothing.dataset.rows().is_empty());
    assert_eq!(
        nothing.checkpoint.as_deref(),
        Some("2026-09-05T08:00:00.000Z aircraft/2026-09-01.csv")
    );
}

#[test]
fn a_wrong_secret_a_bad_object_and_a_denied_egress_fail_closed() {
    let fake = serve(exports());
    let sandbox = SandboxPolicy::air_gapped().permit_host(fake.addr.clone());

    let e = connector(&fake.addr)
        .sync(&SyncContext::new(&Key("not the secret"), &sandbox))
        .expect_err("refused");
    assert!(
        matches!(&e, ConnectorError::Credential(m) if m.contains("SignatureDoesNotMatch")),
        "{e}"
    );

    fake.objects.lock().unwrap().push((
        "aircraft/2026-09-06.csv",
        "2026-09-06T10:00:00.000Z",
        "tail,hours,note\nMG-105,not-a-number,x\n".to_string(),
    ));
    let e = connector(&fake.addr)
        .sync(&SyncContext::new(&Key(SECRET_KEY), &sandbox))
        .expect_err("refused");
    assert!(
        e.to_string().contains("aircraft/2026-09-06.csv") && e.to_string().contains("not-a-number"),
        "{e}"
    );

    let before = fake.log.lock().unwrap().len();
    let e = connector(&fake.addr)
        .sync(&SyncContext::new(
            &Key(SECRET_KEY),
            &SandboxPolicy::air_gapped(),
        ))
        .expect_err("denied");
    assert!(matches!(e, ConnectorError::SandboxDenied { .. }), "{e}");
    thread::sleep(Duration::from_millis(100));
    assert_eq!(
        fake.log.lock().unwrap().len(),
        before,
        "nothing reached the store"
    );
}
