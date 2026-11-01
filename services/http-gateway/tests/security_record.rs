//! The edge's security record: a rejected bearer token is answered with one fixed
//! body, counted, logged with its class and source, and written to the ledger at most
//! once a minute per source; every request leaves one structured JSON log line; and a
//! server error reaches the client only as `internal error` and a reference, its
//! detail kept in the log under that reference. Driven over real sockets, so the
//! source is the accepted connection's peer address.
//! [NFR-SEC-1] [RC-33] [AC-M-4] [VLD-ANB-MIDGARD-0052] [VLD-ANB-MIDGARD-0053]
//! [VLD-ANB-MIDGARD-0054]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use audit_writer::{AuditError, AuditWriter, LedgerCheckpoint, LedgerId, LedgerStore, SealedEntry};
use governance::{Governance, GOV_SIG_ALG};
use http_gateway::http::Request;
use http_gateway::json::{parse, Json};
use http_gateway::{serve_listener, SecuredGateway, DEMO_PASSWORD, DEMO_USERNAME};
use iam::{Idp, UserRecord};
use policy::{
    Action, ClassificationLevel, Clearance, DefaultPdp, Effect, PolicySet, Purpose, Role, Rule,
    RuleId, Selector,
};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::OsRng;
use telemetry::{BufferSink, Level, LogRecord};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}

/// Serve `gw` on an ephemeral loopback port.
fn spawn<L: LedgerStore + Send + 'static>(
    gw: SecuredGateway<fabric::InMemoryStore, L>,
) -> (Arc<SecuredGateway<fabric::InMemoryStore, L>>, SocketAddr) {
    let gw = Arc::new(gw);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().unwrap();
    let served = Arc::clone(&gw);
    thread::spawn(move || {
        let _ = serve_listener(listener, served);
    });
    (gw, addr)
}

/// A development gateway logging into a buffer, served on a loopback port.
fn spawn_dev_gateway() -> (Arc<SecuredGateway>, Arc<BufferSink>, SocketAddr) {
    let sink = Arc::new(BufferSink::new());
    let (gw, addr) = spawn(SecuredGateway::dev().with_log_sink(sink.clone()));
    (gw, sink, addr)
}

/// Send `bytes` on a fresh connection: the status, the head and the body.
fn raw_request(addr: SocketAddr, bytes: &[u8]) -> (u16, String, String) {
    let mut stream = TcpStream::connect(addr).expect("connect to the edge");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(bytes).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    let resp = String::from_utf8_lossy(&resp).into_owned();
    let (head, body) = resp.split_once("\r\n\r\n").unwrap_or((&resp, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, head.to_string(), body.to_string())
}

fn get(addr: SocketAddr, path: &str, authorization: Option<&str>) -> (u16, String, String) {
    let auth = authorization
        .map(|a| format!("Authorization: {a}\r\n"))
        .unwrap_or_default();
    raw_request(
        addr,
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Connection: close\r\n\r\n")
            .as_bytes(),
    )
}

fn request(method: &str, path: &str, query: &str, body: &str, token: Option<&str>) -> Request {
    Request {
        method: method.into(),
        path: path.into(),
        query: query.into(),
        body: body.into(),
        authorization: token.map(|t| format!("Bearer {t}")),
        origin: None,
    }
}

/// Log in in-process; the operator's bearer token.
fn operator_token(gw: &SecuredGateway) -> String {
    let body = format!(r#"{{"username":"{DEMO_USERNAME}","password":"{DEMO_PASSWORD}"}}"#);
    let r = gw.handle(&request("POST", "/api/v1/login", "", &body, None), now());
    assert_eq!(r.status, 200, "{}", r.body);
    parse(&r.body)
        .unwrap()
        .str_field("token")
        .unwrap()
        .to_string()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn field<'a>(record: &'a LogRecord, key: &str) -> Option<&'a str> {
    record
        .fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn is_reference(s: &str) -> bool {
    s.len() == 16
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// A token for a principal the gateway has never heard of, signed by another IdP.
fn forged_token() -> String {
    let mut idp = Idp::generate("forger", &mut OsRng).unwrap();
    idp.store_mut().upsert(
        DEMO_USERNAME,
        UserRecord {
            roles: [Role("operator".into())].into_iter().collect(),
            clearance: Clearance {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: BTreeSet::new(),
            },
            purposes: [Purpose("operations".into())].into_iter().collect(),
        },
    );
    hex(&idp
        .issue_token(DEMO_USERNAME, Purpose("operations".into()), now(), 3_600)
        .unwrap())
}

/// A malformed, a forged and a missing bearer token from one source: three identical
/// 401s, three counted rejections, three log lines naming class and source, and one
/// ledger row, because a source's rejections are audited at most once a minute.
/// [VLD-ANB-MIDGARD-0053] [VLD-ANB-MIDGARD-0054] [AC-26.3]
#[test]
fn a_rejected_token_is_uniform_counted_logged_and_audited_once_per_minute() {
    let (gw, sink, addr) = spawn_dev_gateway();
    let forged = format!("Bearer {}", forged_token());
    for auth in [Some("Bearer zz"), Some(forged.as_str()), None] {
        let (status, head, body) = get(addr, "/api/v1/whoami", auth);
        assert_eq!(status, 401, "{auth:?}: {head}");
        assert_eq!(body, r#"{"error":"token rejected"}"#, "{auth:?}");
    }

    let counted: u64 = gw
        .metrics_text()
        .lines()
        .filter(|l| l.starts_with("http_auth_rejections_total{"))
        .filter_map(|l| l.rsplit(' ').next()?.parse::<u64>().ok())
        .sum();
    assert_eq!(counted, 3, "{}", gw.metrics_text());

    let rejected: Vec<LogRecord> = sink
        .records()
        .into_iter()
        .filter(|r| r.level == Level::Warn && r.message == "token rejected")
        .collect();
    let classes: BTreeSet<&str> = rejected.iter().filter_map(|r| field(r, "class")).collect();
    assert_eq!(rejected.len(), 3);
    assert_eq!(classes, ["malformed", "missing", "signature"].into());
    for r in &rejected {
        assert_eq!(field(r, "source"), Some("127.0.0.1"), "{r:?}");
        assert!(is_reference(field(r, "request_id").unwrap_or("")), "{r:?}");
    }

    let token = operator_token(&gw);
    let audit = gw.handle(
        &request("GET", "/api/v1/audit", "limit=200", "", Some(&token)),
        now(),
    );
    assert_eq!(audit.status, 200, "{}", audit.body);
    let rows = match parse(&audit.body).unwrap() {
        Json::Arr(rows) => rows,
        other => panic!("not a list: {}", other.encode()),
    };
    let auth_rows: Vec<&Json> = rows
        .iter()
        .filter(|r| {
            r.str_field("action")
                .is_some_and(|a| a.starts_with("AUTH rejected"))
        })
        .collect();
    assert_eq!(auth_rows.len(), 1, "{}", audit.body);
    assert_eq!(auth_rows[0].str_field("actor"), Some("(unauthenticated)"));
    assert_eq!(auth_rows[0].str_field("resource"), Some("/api/v1/whoami"));
}

/// Each request leaves one JSON line in the log, which the repository's own parser
/// reads back: method, path, status, latency, source, reference, and the subject once
/// authenticated. [RC-33] [VLD-ANB-MIDGARD-0052] [AC-26.4]
#[test]
fn every_request_writes_one_json_log_line_with_source_and_request_id() {
    let (gw, sink, addr) = spawn_dev_gateway();
    let token = operator_token(&gw);
    let (status, _, _) = get(addr, "/api/v1/whoami", Some(&format!("Bearer {token}")));
    assert_eq!(status, 200);

    let lines: Vec<LogRecord> = sink
        .records()
        .into_iter()
        .filter(|r| r.message == "request" && field(r, "path") == Some("/api/v1/whoami"))
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = parse(&lines[0].to_json()).expect("the log line is JSON");
    assert_eq!(line.str_field("level"), Some("info"));
    assert_eq!(line.str_field("target"), Some("http-gateway"));
    assert_eq!(line.str_field("method"), Some("GET"));
    assert_eq!(line.str_field("status"), Some("200"));
    assert_eq!(line.str_field("source"), Some("127.0.0.1"));
    assert_eq!(line.str_field("subject"), Some(DEMO_USERNAME));
    assert!(line
        .str_field("latency_ms")
        .and_then(|l| l.parse::<f64>().ok())
        .is_some());
    let id = line.str_field("request_id").unwrap_or_default();
    assert!(is_reference(id), "{id}");
    let same_id = sink
        .records()
        .iter()
        .filter(|r| r.message == "request" && field(r, "request_id") == Some(id))
        .count();
    assert_eq!(same_id, 1, "one line per request");

    // An unauthenticated request is logged too, with no subject.
    let (status, _, _) = get(addr, "/api/v1/health", None);
    assert_eq!(status, 200);
    let health = sink
        .records()
        .into_iter()
        .find(|r| r.message == "request" && field(r, "path") == Some("/api/v1/health"))
        .expect("the health check is logged");
    assert_eq!(field(&health, "subject"), None);
    assert_eq!(field(&health, "source"), Some("127.0.0.1"));
}

/// A ledger whose disk is full.
struct FullLedger;
impl LedgerStore for FullLedger {
    fn append(&mut self, _: SealedEntry) -> Result<(), AuditError> {
        Err(AuditError::Store(
            "disk full at /var/lib/midgard/gov.ledger".into(),
        ))
    }
    fn last(&self) -> Option<&SealedEntry> {
        None
    }
    fn entries(&self) -> &[SealedEntry] {
        &[]
    }
    fn record_checkpoint(&mut self, _: LedgerCheckpoint) -> Result<(), AuditError> {
        Err(AuditError::Store("disk full".into()))
    }
    fn checkpoint(&self) -> Option<&LedgerCheckpoint> {
        None
    }
}

/// A server error (here the ledger refusing the admission row) reaches the client as
/// `internal error` and a reference; the store's own text is only in the log, under
/// that reference. [VLD-ANB-MIDGARD-0053] [AC-26.5]
#[test]
fn server_errors_carry_a_reference_not_detail() {
    let mut idp = Idp::generate("test-idp", &mut OsRng).unwrap();
    idp.store_mut().upsert(
        "analyst-1",
        UserRecord {
            roles: [Role("Analyst".into())].into_iter().collect(),
            clearance: Clearance {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: BTreeSet::new(),
            },
            purposes: [Purpose("Investigation".into())].into_iter().collect(),
        },
    );
    let token = hex(&idp
        .issue_token("analyst-1", Purpose("Investigation".into()), now(), 3_600)
        .unwrap());
    let (vk, sk) = fips_provider()
        .sig_for(GOV_SIG_ALG)
        .unwrap()
        .keypair(&mut OsRng)
        .unwrap();
    let pdp = DefaultPdp::new(
        PolicySet::new(vec![Rule {
            id: RuleId("analyst-read".into()),
            role: Role("Analyst".into()),
            action: Action("Read".into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        }])
        .unwrap(),
    );
    let governance = Governance::with_audit(
        pdp,
        AuditWriter::new(
            FullLedger,
            Box::new(FipsSig::new(GOV_SIG_ALG)),
            sk,
            vk,
            LedgerId("midgard-governance".into()),
        ),
    );
    let sink = Arc::new(BufferSink::new());
    let gw = SecuredGateway::new(
        fips_provider(),
        idp.verifying_key().clone(),
        governance,
        Vec::new(),
    )
    .with_log_sink(sink.clone());
    let (_gw, addr) = spawn(gw);

    let (status, _, body) = get(addr, "/api/v1/whoami", Some(&format!("Bearer {token}")));
    assert_eq!(status, 500, "{body}");
    assert!(!body.contains("disk") && !body.contains("audit"), "{body}");
    let body = parse(&body).unwrap();
    assert_eq!(body.str_field("error"), Some("internal error"));
    let reference = body.str_field("reference").unwrap_or_default().to_string();
    assert!(is_reference(&reference), "{reference}");

    let records = sink.records();
    let detail = records
        .iter()
        .find(|r| r.level == Level::Error && field(r, "request_id") == Some(&reference))
        .expect("the detail is logged under the reference");
    assert!(
        field(detail, "detail").is_some_and(|d| d.contains("disk full")),
        "{detail:?}"
    );
    assert!(records.iter().any(|r| r.message == "request"
        && field(r, "request_id") == Some(&reference)
        && field(r, "status") == Some("500")));
}
