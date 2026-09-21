//! The Kafka connector against an in-process broker speaking the protocol subset it
//! uses, over a real socket, with real v2 record batches and their CRC-32C: a topic's
//! partitions sync after a mutual SASL SCRAM-SHA-256 exchange, across several fetches,
//! skipping tombstones and tolerating a partial trailing batch; an incremental sync
//! resumes mid-batch from its checkpoint and picks up new records; and a wrong
//! password, a broker without SCRAM, a compressed or corrupt batch, a leader on another
//! broker, a checkpoint the log no longer holds and a denied egress all fail closed.
//! [FR-F-2] [FR-F-5] [NFR-SEC-1] [NFR-PRV-1]
#![cfg(all(feature = "backend-fips", feature = "connector-stream"))]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use forge_connector_sdk::connectors::kafka::{crc32c, KafkaConnector, StartFrom};
use forge_connector_sdk::wire::{base64_decode, base64_encode, pbkdf2_hmac_sha256};
use forge_connector_sdk::{
    Connector, ConnectorError, ConnectorKind, CredentialRef, CredentialResolver, SandboxPolicy,
    Secret, SyncContext,
};
use forge_dataset_store::{Column, ColumnType, DataValue, Schema};
use pqc_agility::hash::{hmac_sha256, sha256};

const USER: &str = "forge";
const PASSWORD: &str = "kafka passphrase";
const SALT: &[u8] = b"kafka-scram-salt";
const TOPIC: &str = "aircraft-positions";

struct Password(&'static str);

impl CredentialResolver for Password {
    fn resolve(&self, cref: &CredentialRef) -> Result<Secret, ConnectorError> {
        assert_eq!(cref.0, "kafka://forge");
        Ok(Secret::new(self.0.as_bytes().to_vec()))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Batches {
    Plain,
    Compressed,
    CorruptCrc,
}

#[derive(Clone)]
struct Behaviour {
    scram: bool,
    batches: Batches,
    leader_elsewhere: bool,
    log_start: i64,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            scram: true,
            batches: Batches::Plain,
            leader_elsewhere: false,
            log_start: 0,
        }
    }
}

type Log = Arc<Mutex<Vec<Vec<Option<String>>>>>;

struct Fake {
    addr: String,
    port: u16,
    partitions: Log,
    calls: Arc<Mutex<Vec<String>>>,
}

fn serve(behaviour: Behaviour) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let partitions: Log = Arc::new(Mutex::new(vec![
        vec![
            Some("MG-101,6412,north".into()),
            Some("MG-102,,south".into()),
            None,
            Some("MG-103,5,\"a, b\"".into()),
        ],
        vec![Some("MG-201,1,x".into()), Some("MG-202,2,y".into())],
    ]));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (shared, shared_calls) = (partitions.clone(), calls.clone());
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (b, log, calls) = (behaviour.clone(), shared.clone(), shared_calls.clone());
            thread::spawn(move || {
                let _ = session(stream, port, &b, &log, &calls);
            });
        }
    });
    Fake {
        addr: format!("127.0.0.1:{port}"),
        port,
        partitions,
        calls,
    }
}

struct In<'a>(&'a [u8], usize);

impl In<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let s = &self.0[self.1..self.1 + n];
        self.1 += n;
        s
    }
    fn i8(&mut self) -> i8 {
        self.take(1)[0] as i8
    }
    fn i16(&mut self) -> i16 {
        let b = self.take(2);
        i16::from_be_bytes([b[0], b[1]])
    }
    fn i32(&mut self) -> i32 {
        let b = self.take(4);
        i32::from_be_bytes([b[0], b[1], b[2], b[3]])
    }
    fn i64(&mut self) -> i64 {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8));
        i64::from_be_bytes(a)
    }
    fn string(&mut self) -> String {
        let n = self.i16() as usize;
        String::from_utf8(self.take(n).to_vec()).unwrap()
    }
    fn bytes(&mut self) -> Vec<u8> {
        let n = self.i32() as usize;
        self.take(n).to_vec()
    }
}

#[derive(Default)]
struct Out(Vec<u8>);

impl Out {
    fn i8(&mut self, v: i8) -> &mut Self {
        self.0.push(v as u8);
        self
    }
    fn i16(&mut self, v: i16) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn i64(&mut self, v: i64) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn string(&mut self, s: &str) -> &mut Self {
        self.i16(s.len() as i16);
        self.0.extend_from_slice(s.as_bytes());
        self
    }
    fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.i32(b.len() as i32);
        self.0.extend_from_slice(b);
        self
    }
    fn varint(&mut self, v: i64) -> &mut Self {
        let mut z = ((v << 1) ^ (v >> 63)) as u64;
        loop {
            let byte = (z & 0x7f) as u8;
            z >>= 7;
            if z == 0 {
                self.0.push(byte);
                return self;
            }
            self.0.push(byte | 0x80);
        }
    }
}

/// A v2 record batch holding `values`, the first at `base`.
fn batch(base: i64, values: &[Option<String>], attributes: i16, corrupt: bool) -> Vec<u8> {
    let mut tail = Out::default();
    tail.i16(attributes)
        .i32(values.len() as i32 - 1)
        .i64(1_789_000_000_000)
        .i64(1_789_000_000_000)
        .i64(-1)
        .i16(-1)
        .i32(-1)
        .i32(values.len() as i32);
    for (delta, value) in values.iter().enumerate() {
        let mut record = Out::default();
        record.i8(0).varint(delta as i64).varint(0).varint(-1);
        match value {
            Some(v) => {
                record.varint(v.len() as i64);
                record.0.extend_from_slice(v.as_bytes());
            }
            None => {
                record.varint(-1);
            }
        }
        record.varint(0);
        tail.varint(record.0.len() as i64);
        tail.0.extend_from_slice(&record.0);
    }
    let mut crc = crc32c(&tail.0);
    if corrupt {
        crc ^= 1;
    }
    let mut body = Out::default();
    body.i32(0).i8(2);
    body.0.extend_from_slice(&crc.to_be_bytes());
    body.0.extend_from_slice(&tail.0);
    let mut out = Out::default();
    out.i64(base).i32(body.0.len() as i32);
    out.0.extend_from_slice(&body.0);
    out.0
}

fn session(
    mut s: TcpStream,
    port: u16,
    b: &Behaviour,
    log: &Log,
    calls: &Mutex<Vec<String>>,
) -> std::io::Result<()> {
    let mut authenticated = !b.scram;
    let mut sasl_step = 0;
    let mut client_first_bare = String::new();
    let mut server_first = String::new();
    loop {
        let mut size = [0u8; 4];
        if s.read_exact(&mut size).is_err() {
            return Ok(());
        }
        let mut request = vec![0u8; i32::from_be_bytes(size) as usize];
        s.read_exact(&mut request)?;
        let mut r = In(&request, 0);
        let (api, _version, correlation) = (r.i16(), r.i16(), r.i32());
        r.string();
        calls.lock().unwrap().push(format!("api {api}"));
        if !authenticated && ![17, 18, 36].contains(&api) {
            return Ok(());
        }
        let mut out = Out::default();
        out.i32(correlation);
        match api {
            18 => {
                out.i16(0).i32(6);
                for (key, max) in [(1, 11), (2, 5), (3, 9), (17, 1), (18, 3), (36, 2)] {
                    out.i16(key).i16(0).i16(max);
                }
            }
            17 => {
                let mechanism = r.string();
                if b.scram && mechanism == "SCRAM-SHA-256" {
                    out.i16(0).i32(1).string("SCRAM-SHA-256");
                } else {
                    out.i16(33).i32(1).string("PLAIN");
                }
            }
            36 => {
                let auth = String::from_utf8(r.bytes()).unwrap();
                sasl_step += 1;
                if sasl_step == 1 {
                    client_first_bare = auth.strip_prefix("n,,").unwrap().to_string();
                    let nonce = client_first_bare
                        .split(',')
                        .find_map(|a| a.strip_prefix("r="))
                        .unwrap();
                    server_first = format!("r={nonce}broker,s={},i=4096", base64_encode(SALT));
                    out.i16(0).i16(-1).bytes(server_first.as_bytes());
                } else {
                    let (without_proof, proof) = auth.rsplit_once(",p=").unwrap();
                    let salted = pbkdf2_hmac_sha256(PASSWORD.as_bytes(), SALT, 4096, 32);
                    let client_key = hmac_sha256(&salted, b"Client Key");
                    let message = format!("{client_first_bare},{server_first},{without_proof}");
                    let signature = hmac_sha256(&sha256(&client_key), message.as_bytes());
                    let expected: Vec<u8> = client_key
                        .iter()
                        .zip(signature.iter())
                        .map(|(k, s)| k ^ s)
                        .collect();
                    if base64_decode(proof).as_deref() != Some(expected.as_slice()) {
                        out.i16(58)
                            .string("Authentication failed during authentication")
                            .bytes(&[]);
                    } else {
                        let verifier =
                            hmac_sha256(&hmac_sha256(&salted, b"Server Key"), message.as_bytes());
                        out.i16(0)
                            .i16(-1)
                            .bytes(format!("v={}", base64_encode(&verifier)).as_bytes());
                        authenticated = true;
                    }
                }
            }
            3 => {
                let partitions = log.lock().unwrap().len();
                out.i32(2);
                out.i32(1).string("127.0.0.1").i32(i32::from(port)).i16(-1);
                out.i32(2).string("10.9.9.9").i32(9092).i16(-1);
                out.i32(1)
                    .i32(1)
                    .i16(0)
                    .string(TOPIC)
                    .i8(0)
                    .i32(partitions as i32);
                for p in 0..partitions {
                    let leader = if b.leader_elsewhere && p == 1 { 2 } else { 1 };
                    out.i16(0)
                        .i32(p as i32)
                        .i32(leader)
                        .i32(1)
                        .i32(leader)
                        .i32(1)
                        .i32(leader);
                }
            }
            2 => {
                r.i32();
                r.i32();
                r.string();
                let count = r.i32();
                let log = log.lock().unwrap();
                out.i32(1).string(TOPIC).i32(count);
                for _ in 0..count {
                    let partition = r.i32();
                    let timestamp = r.i64();
                    let offset = if timestamp == -2 {
                        b.log_start
                    } else {
                        log[partition as usize].len() as i64
                    };
                    out.i32(partition).i16(0).i64(-1).i64(offset);
                }
            }
            1 => {
                r.i32();
                r.i32();
                r.i32();
                r.i32();
                r.i8();
                r.i32();
                r.string();
                r.i32();
                let partition = r.i32();
                let offset = r.i64();
                let max_bytes = r.i32();
                let log = log.lock().unwrap();
                let values = &log[partition as usize];
                // Batches of two records; a small fetch gets one batch and a partial second.
                let mut records = Vec::new();
                let mut base = (offset / 2) * 2;
                let mut batches = 0;
                while (base as usize) < values.len() {
                    let end = ((base + 2) as usize).min(values.len());
                    let attributes = if b.batches == Batches::Compressed {
                        1
                    } else {
                        0
                    };
                    let encoded = batch(
                        base,
                        &values[base as usize..end],
                        attributes,
                        b.batches == Batches::CorruptCrc,
                    );
                    if max_bytes < 2048 && batches == 1 {
                        records.extend_from_slice(&encoded[..20]);
                        break;
                    }
                    records.extend_from_slice(&encoded);
                    batches += 1;
                    base = end as i64;
                }
                calls
                    .lock()
                    .unwrap()
                    .push(format!("fetch {partition}@{offset}"));
                out.i32(0).i32(1).string(TOPIC).i32(1);
                out.i32(partition)
                    .i16(0)
                    .i64(values.len() as i64)
                    .i64(values.len() as i64)
                    .i32(-1)
                    .bytes(&records);
            }
            _ => return Ok(()),
        }
        let mut frame = (out.0.len() as i32).to_be_bytes().to_vec();
        frame.extend_from_slice(&out.0);
        s.write_all(&frame)?;
    }
}

fn schema() -> Schema {
    Schema::new(vec![
        Column::new("tail", ColumnType::Text),
        Column::nullable("hours", ColumnType::Int),
        Column::new("base", ColumnType::Text),
    ])
}

fn connector(fake: &Fake) -> KafkaConnector {
    KafkaConnector::new("127.0.0.1", fake.port, TOPIC, schema())
        .scram(USER, CredentialRef::new("kafka://forge"))
        .fetch_bytes(1024)
}

fn tails(rows: &[Vec<DataValue>]) -> Vec<String> {
    rows.iter()
        .map(|r| match &r[0] {
            DataValue::Text(t) => t.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn a_topic_syncs_across_fetches_after_mutual_scram() {
    let fake = serve(Behaviour::default());
    let sandbox = SandboxPolicy::air_gapped().permit_host(fake.addr.clone());
    let conn = connector(&fake);
    assert_eq!(conn.descriptor().kind, ConnectorKind::Stream);

    let outcome = conn
        .sync_incremental(&SyncContext::new(&Password(PASSWORD), &sandbox))
        .unwrap();
    let rows = outcome.dataset.rows();
    assert_eq!(
        tails(rows),
        ["MG-101", "MG-102", "MG-103", "MG-201", "MG-202"]
    );
    assert_eq!(rows[1][1], DataValue::Null);
    assert_eq!(rows[2][2], DataValue::Text("a, b".into()));
    assert_eq!(outcome.checkpoint.as_deref(), Some("0=4,1=2"));

    let calls = fake.calls.lock().unwrap().clone();
    assert_eq!(
        &calls[..4],
        ["api 18", "api 17", "api 36", "api 36"],
        "{calls:?}"
    );
    assert!(
        calls.iter().filter(|c| c.starts_with("fetch 0@")).count() >= 2,
        "small fetches return one batch at a time: {calls:?}"
    );
}

#[test]
fn an_incremental_sync_resumes_mid_batch_and_picks_up_new_records() {
    let fake = serve(Behaviour::default());
    let sandbox = SandboxPolicy::air_gapped().permit_host(fake.addr.clone());
    let conn = connector(&fake);
    let password = Password(PASSWORD);

    let resumed = conn
        .sync_incremental(&SyncContext::new(&password, &sandbox).with_checkpoint("0=1,1=2"))
        .unwrap();
    assert_eq!(tails(resumed.dataset.rows()), ["MG-102", "MG-103"]);
    assert_eq!(resumed.checkpoint.as_deref(), Some("0=4,1=2"));

    let latest = conn
        .clone()
        .start_from(StartFrom::Latest)
        .sync_incremental(&SyncContext::new(&password, &sandbox))
        .unwrap();
    assert!(latest.dataset.rows().is_empty());
    assert_eq!(latest.checkpoint.as_deref(), Some("0=4,1=2"));

    fake.partitions.lock().unwrap()[0].push(Some("MG-104,9,east".into()));
    let new = conn
        .sync_incremental(&SyncContext::new(&password, &sandbox).with_checkpoint("0=4,1=2"))
        .unwrap();
    assert_eq!(tails(new.dataset.rows()), ["MG-104"]);
    assert_eq!(new.checkpoint.as_deref(), Some("0=5,1=2"));
}

#[test]
fn authentication_and_integrity_failures_fail_closed() {
    let run = |behaviour: Behaviour, password: &'static str, checkpoint: Option<&str>| {
        let fake = serve(behaviour);
        let sandbox = SandboxPolicy::air_gapped().permit_host(fake.addr.clone());
        let resolver = Password(password);
        let ctx = SyncContext::new(&resolver, &sandbox);
        let ctx = match checkpoint {
            Some(c) => ctx.with_checkpoint(c),
            None => ctx,
        };
        connector(&fake)
            .sync_incremental(&ctx)
            .expect_err("fails closed")
            .to_string()
    };

    let e = run(Behaviour::default(), "wrong", None);
    assert!(e.contains("SASL_AUTHENTICATION_FAILED"), "{e}");
    let e = run(
        Behaviour {
            scram: false,
            ..Behaviour::default()
        },
        PASSWORD,
        None,
    );
    assert!(e.contains("SCRAM-SHA-256") && e.contains("PLAIN"), "{e}");
    let e = run(
        Behaviour {
            batches: Batches::Compressed,
            ..Behaviour::default()
        },
        PASSWORD,
        None,
    );
    assert!(e.contains("compressed"), "{e}");
    let e = run(
        Behaviour {
            batches: Batches::CorruptCrc,
            ..Behaviour::default()
        },
        PASSWORD,
        None,
    );
    assert!(e.contains("CRC-32C"), "{e}");
    let e = run(
        Behaviour {
            leader_elsewhere: true,
            ..Behaviour::default()
        },
        PASSWORD,
        None,
    );
    assert!(e.contains("led by broker 2"), "{e}");
    let e = run(
        Behaviour {
            log_start: 2,
            ..Behaviour::default()
        },
        PASSWORD,
        Some("0=1,1=2"),
    );
    assert!(e.contains("no longer holds offset 1"), "{e}");

    let fake = serve(Behaviour::default());
    let e = connector(&fake)
        .sync(&SyncContext::new(
            &Password(PASSWORD),
            &SandboxPolicy::air_gapped(),
        ))
        .expect_err("denied");
    assert!(matches!(e, ConnectorError::SandboxDenied { .. }), "{e}");
    thread::sleep(Duration::from_millis(100));
    assert!(fake.calls.lock().unwrap().is_empty());
}
