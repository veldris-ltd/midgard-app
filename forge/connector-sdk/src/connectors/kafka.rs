//! A **Kafka** connector: a hand-rolled client for the part of the Kafka protocol a
//! bounded, resumable read of one topic needs. [FR-F-2] [FR-F-5] [NFR-SEC-1]
//! [NFR-PRV-1]
//!
//! - **Egress** is checked against the sandbox for exactly the bootstrap `host:port`,
//!   and every partition's leader must be that broker — a leader elsewhere is refused
//!   rather than dialled.
//! - **Authentication**, when configured, is SASL SCRAM-SHA-256, mutual: the broker
//!   must prove it holds the password's verifier. The password is a [`CredentialRef`].
//! - **Bounded reads**: each sync reads every partition from its checkpointed offset
//!   up to the high watermark it found when it began, so a sync finishes even while
//!   producers write. The checkpoint is each partition's next offset (`0=42,1=17`);
//!   partitions are assigned by the client, and no consumer group commits anything.
//! - **Record batches** (format v2) are checked against their CRC-32C. Compressed
//!   batches and transactional reads with aborted transactions are refused, not
//!   guessed at; control batches are skipped.
//! - **Values** are delimited records (one RFC 4180 record per message) in the job's
//!   schema; a null value (a tombstone) is skipped.
//!
//! Versions used: ApiVersions v0, SaslHandshake v1, SaslAuthenticate v0, Metadata v1,
//! ListOffsets v1, Fetch v4. TLS is the egress broker's job (see `deploy/`).

use std::io::{Read, Write};
use std::net::TcpStream;

use forge_dataset_store::{Dataset, Row, Schema};
use pqc_agility::{OsRng, Rng};

use crate::connector::{Connector, ConnectorDescriptor, ConnectorKind, SyncContext, SyncOutcome};
use crate::connectors::file::DelimitedConnector;
use crate::credential::CredentialRef;
use crate::error::ConnectorError;
use crate::net;
use crate::wire::{base64_encode, ScramSha256};

const API_FETCH: i16 = 1;
const API_LIST_OFFSETS: i16 = 2;
const API_METADATA: i16 = 3;
const API_SASL_HANDSHAKE: i16 = 17;
const API_VERSIONS: i16 = 18;
const API_SASL_AUTHENTICATE: i16 = 36;

/// The largest broker response accepted.
const MAX_RESPONSE: usize = 128 * 1024 * 1024;
/// Bytes requested per fetch unless configured otherwise.
pub const DEFAULT_FETCH_BYTES: i32 = 4 * 1024 * 1024;
const CLIENT_ID: &str = "forge-worker";

/// Where a partition with no checkpoint starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartFrom {
    /// The oldest retained record.
    Earliest,
    /// Only records produced after the first sync.
    Latest,
}

/// A Kafka topic source.
#[derive(Debug, Clone)]
pub struct KafkaConnector {
    host: String,
    port: u16,
    topic: String,
    schema: Schema,
    delimiter: char,
    start: StartFrom,
    sasl: Option<(String, CredentialRef)>,
    fetch_bytes: i32,
}

impl KafkaConnector {
    /// Bind `topic` on the broker at `host:port`, each message's value one delimited
    /// record in `schema`, starting from the earliest offset.
    pub fn new(
        host: impl Into<String>,
        port: u16,
        topic: impl Into<String>,
        schema: Schema,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            topic: topic.into(),
            schema,
            delimiter: ',',
            start: StartFrom::Earliest,
            sasl: None,
            fetch_bytes: DEFAULT_FETCH_BYTES,
        }
    }

    /// Authenticate as `user` with SASL SCRAM-SHA-256 and the password `password`
    /// resolves to.
    pub fn scram(mut self, user: impl Into<String>, password: CredentialRef) -> Self {
        self.sasl = Some((user.into(), password));
        self
    }

    /// Start partitions with no checkpoint from `start`.
    pub fn start_from(mut self, start: StartFrom) -> Self {
        self.start = start;
        self
    }

    /// Split values on `delimiter`.
    pub fn delimiter(mut self, delimiter: char) -> Self {
        self.delimiter = delimiter;
        self
    }

    /// Ask for up to `bytes` per fetch (at least 1 KiB).
    pub fn fetch_bytes(mut self, bytes: i32) -> Self {
        self.fetch_bytes = bytes.max(1024);
        self
    }
}

/// Parse a checkpoint: `partition=next offset` pairs, comma-separated.
fn parse_checkpoint(checkpoint: &str) -> Result<Vec<(i32, i64)>, ConnectorError> {
    checkpoint
        .split(',')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            pair.split_once('=')
                .and_then(|(p, o)| Some((p.parse().ok()?, o.parse().ok()?)))
                .ok_or_else(|| {
                    ConnectorError::Parse(format!("a malformed Kafka checkpoint '{checkpoint}'"))
                })
        })
        .collect()
}

impl Connector for KafkaConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: ConnectorKind::Stream,
            source: format!("kafka:{}:{}/{}", self.host, self.port, self.topic),
            streaming: true,
        }
    }

    fn egress_hosts(&self) -> Vec<String> {
        vec![format!("{}:{}", self.host, self.port)]
    }

    fn sync(&self, ctx: &SyncContext) -> Result<Dataset, ConnectorError> {
        self.sync_incremental(ctx).map(|outcome| outcome.dataset)
    }

    fn sync_incremental(&self, ctx: &SyncContext) -> Result<SyncOutcome, ConnectorError> {
        let resumed = ctx
            .checkpoint
            .map(parse_checkpoint)
            .transpose()?
            .unwrap_or_default();
        let stream = net::connect(ctx.sandbox, &self.host, self.port)?;
        let mut broker = Broker {
            stream,
            peer: format!("{}:{}", self.host, self.port),
            correlation: 0,
        };
        broker.check_versions()?;
        if let Some((user, password)) = &self.sasl {
            let secret = ctx.credentials.resolve(password)?;
            let password = std::str::from_utf8(secret.expose()).map_err(|_| {
                ConnectorError::Credential("the Kafka password is not UTF-8".into())
            })?;
            broker.authenticate(user, password)?;
        }

        let partitions = broker.partitions(&self.topic, &self.host, self.port)?;
        let latest = broker.offsets(&self.topic, &partitions, -1)?;
        let earliest = broker.offsets(&self.topic, &partitions, -2)?;

        let mut rows: Vec<Row> = Vec::new();
        let mut next_offsets = Vec::with_capacity(partitions.len());
        for (i, &partition) in partitions.iter().enumerate() {
            let end = latest[i];
            let start = match resumed.iter().find(|(p, _)| *p == partition) {
                Some(&(_, offset)) => offset,
                None if self.start == StartFrom::Latest => end,
                None => earliest[i],
            };
            if start < earliest[i] {
                return Err(ConnectorError::Io(format!(
                    "{}: partition {partition} no longer holds offset {start} (it starts at {}); \
                     records were deleted before they were synced",
                    self.topic, earliest[i]
                )));
            }
            let mut offset = start;
            while offset < end {
                let (records, next) =
                    broker.fetch(&self.topic, partition, offset, self.fetch_bytes)?;
                if next <= offset {
                    return Err(ConnectorError::Parse(format!(
                        "{}: partition {partition} made no progress at offset {offset}; a batch \
                         may be larger than the fetch size",
                        self.topic
                    )));
                }
                for (record_offset, value) in records {
                    if record_offset >= end {
                        break;
                    }
                    let Some(value) = value else { continue };
                    let text = String::from_utf8(value).map_err(|_| {
                        ConnectorError::Parse(format!(
                            "{}: partition {partition} offset {record_offset} is not UTF-8",
                            self.topic
                        ))
                    })?;
                    let parsed =
                        DelimitedConnector::parse_body(&self.schema, &text, self.delimiter, false)
                            .map_err(|e| {
                                ConnectorError::Parse(format!(
                                    "{}: partition {partition} offset {record_offset}: {e}",
                                    self.topic
                                ))
                            })?;
                    if parsed.rows().len() != 1 {
                        return Err(ConnectorError::Parse(format!(
                            "{}: partition {partition} offset {record_offset} holds {} records; a \
                             message must hold exactly one",
                            self.topic,
                            parsed.rows().len()
                        )));
                    }
                    rows.extend(parsed.rows().iter().cloned());
                }
                offset = next.min(end);
            }
            next_offsets.push(format!("{partition}={offset}"));
        }
        drop(broker);

        Ok(SyncOutcome {
            dataset: Dataset::new(self.schema.clone(), rows)?,
            checkpoint: Some(next_offsets.join(",")),
        })
    }
}

/// One connection to the bootstrap broker.
struct Broker {
    stream: TcpStream,
    peer: String,
    correlation: i32,
}

impl Broker {
    fn io(&self, e: std::io::Error) -> ConnectorError {
        ConnectorError::Io(format!("{}: {e}", self.peer))
    }

    /// Send one request and return the response body after its header.
    fn call(&mut self, api_key: i16, version: i16, body: &[u8]) -> Result<Vec<u8>, ConnectorError> {
        self.correlation += 1;
        let mut request = Writer::default();
        request
            .i16(api_key)
            .i16(version)
            .i32(self.correlation)
            .string(CLIENT_ID);
        request.0.extend_from_slice(body);
        let mut frame = (request.0.len() as i32).to_be_bytes().to_vec();
        frame.extend_from_slice(&request.0);
        self.stream.write_all(&frame).map_err(|e| self.io(e))?;

        let mut size = [0u8; 4];
        self.stream.read_exact(&mut size).map_err(|e| self.io(e))?;
        let size = usize::try_from(i32::from_be_bytes(size))
            .ok()
            .filter(|&s| (4..=MAX_RESPONSE).contains(&s))
            .ok_or_else(|| {
                ConnectorError::Parse(format!("{}: a malformed response size", self.peer))
            })?;
        let mut response = vec![0u8; size];
        self.stream
            .read_exact(&mut response)
            .map_err(|e| self.io(e))?;
        let correlation = i32::from_be_bytes([response[0], response[1], response[2], response[3]]);
        if correlation != self.correlation {
            return Err(ConnectorError::Parse(format!(
                "{}: a response for request {correlation}, not {}",
                self.peer, self.correlation
            )));
        }
        response.drain(..4);
        Ok(response)
    }

    fn check_versions(&mut self) -> Result<(), ConnectorError> {
        let response = self.call(API_VERSIONS, 0, &[])?;
        let mut r = Reader::new(&response);
        kafka_error(&self.peer, "ApiVersions", r.i16()?)?;
        let mut supported = Vec::new();
        for _ in 0..r.count()? {
            supported.push((r.i16()?, r.i16()?, r.i16()?));
        }
        for (key, version, name) in [
            (API_FETCH, 4, "Fetch v4"),
            (API_LIST_OFFSETS, 1, "ListOffsets v1"),
            (API_METADATA, 1, "Metadata v1"),
        ] {
            if !supported
                .iter()
                .any(|&(k, min, max)| k == key && (min..=max).contains(&version))
            {
                return Err(ConnectorError::Io(format!(
                    "{} does not support {name}",
                    self.peer
                )));
            }
        }
        Ok(())
    }

    fn authenticate(&mut self, user: &str, password: &str) -> Result<(), ConnectorError> {
        let mut handshake = Writer::default();
        handshake.string("SCRAM-SHA-256");
        let response = self.call(API_SASL_HANDSHAKE, 1, &handshake.0)?;
        let mut r = Reader::new(&response);
        let code = r.i16()?;
        if code != 0 {
            let mut offered = Vec::new();
            for _ in 0..r.count()? {
                offered.push(r.string()?.unwrap_or_default());
            }
            return Err(ConnectorError::Credential(format!(
                "{} does not accept SASL SCRAM-SHA-256 (it offers {})",
                self.peer,
                offered.join(", ")
            )));
        }

        let mut nonce = [0u8; 18];
        OsRng.fill(&mut nonce);
        let mut scram = ScramSha256::new(user, &base64_encode(&nonce));
        let server_first = self.sasl_round(scram.client_first().as_bytes())?;
        let server_first = String::from_utf8(server_first)
            .map_err(|_| ConnectorError::Parse("a SCRAM challenge that is not UTF-8".into()))?;
        let client_final = scram
            .client_final(&server_first, password)
            .map_err(ConnectorError::Credential)?;
        let server_final = self.sasl_round(client_final.as_bytes())?;
        let server_final = String::from_utf8(server_final)
            .map_err(|_| ConnectorError::Parse("a SCRAM outcome that is not UTF-8".into()))?;
        scram
            .verify_server_final(&server_final)
            .map_err(ConnectorError::Credential)
    }

    fn sasl_round(&mut self, bytes: &[u8]) -> Result<Vec<u8>, ConnectorError> {
        let mut request = Writer::default();
        request.bytes(bytes);
        let response = self.call(API_SASL_AUTHENTICATE, 0, &request.0)?;
        let mut r = Reader::new(&response);
        let code = r.i16()?;
        let message = r.string()?.unwrap_or_default();
        if code != 0 {
            return Err(ConnectorError::Credential(format!(
                "{}: SASL authentication failed ({}): {message}",
                self.peer,
                error_name(code)
            )));
        }
        Ok(r.bytes()?.unwrap_or_default())
    }

    /// The topic's partitions, in order; every leader must be this broker.
    fn partitions(
        &mut self,
        topic: &str,
        host: &str,
        port: u16,
    ) -> Result<Vec<i32>, ConnectorError> {
        let mut request = Writer::default();
        request.i32(1).string(topic);
        let response = self.call(API_METADATA, 1, &request.0)?;
        let mut r = Reader::new(&response);
        let mut brokers = Vec::new();
        for _ in 0..r.count()? {
            let node = r.i32()?;
            let broker_host = r.string()?.unwrap_or_default();
            let broker_port = r.i32()?;
            r.string()?; // rack
            brokers.push((node, broker_host, broker_port));
        }
        r.i32()?; // controller
        let mut partitions = Vec::new();
        for _ in 0..r.count()? {
            let code = r.i16()?;
            let name = r.string()?.unwrap_or_default();
            r.i8()?; // internal
            kafka_error(&self.peer, &format!("Metadata for {name}"), code)?;
            for _ in 0..r.count()? {
                let code = r.i16()?;
                let partition = r.i32()?;
                let leader = r.i32()?;
                for _ in 0..r.count()? {
                    r.i32()?;
                }
                for _ in 0..r.count()? {
                    r.i32()?;
                }
                kafka_error(&self.peer, &format!("{name} partition {partition}"), code)?;
                let at_bootstrap = brokers.iter().any(|(node, h, p)| {
                    *node == leader && h == host && i64::from(*p) == i64::from(port)
                });
                if !at_bootstrap {
                    return Err(ConnectorError::Io(format!(
                        "{name} partition {partition} is led by broker {leader}, not the \
                         bootstrap broker {host}:{port}; reading from other brokers is not \
                         supported"
                    )));
                }
                partitions.push(partition);
            }
        }
        if partitions.is_empty() {
            return Err(ConnectorError::Io(format!(
                "{}: topic {topic} has no partitions",
                self.peer
            )));
        }
        partitions.sort_unstable();
        Ok(partitions)
    }

    /// Each partition's offset at `timestamp` (-1 latest, -2 earliest), in order.
    fn offsets(
        &mut self,
        topic: &str,
        partitions: &[i32],
        timestamp: i64,
    ) -> Result<Vec<i64>, ConnectorError> {
        let mut request = Writer::default();
        request
            .i32(-1)
            .i32(1)
            .string(topic)
            .i32(partitions.len() as i32);
        for &p in partitions {
            request.i32(p).i64(timestamp);
        }
        let response = self.call(API_LIST_OFFSETS, 1, &request.0)?;
        let mut r = Reader::new(&response);
        let mut found = Vec::new();
        for _ in 0..r.count()? {
            r.string()?;
            for _ in 0..r.count()? {
                let partition = r.i32()?;
                let code = r.i16()?;
                r.i64()?; // timestamp
                let offset = r.i64()?;
                kafka_error(
                    &self.peer,
                    &format!("ListOffsets {topic} partition {partition}"),
                    code,
                )?;
                found.push((partition, offset));
            }
        }
        partitions
            .iter()
            .map(|p| {
                found
                    .iter()
                    .find(|(q, _)| q == p)
                    .map(|&(_, o)| o)
                    .ok_or_else(|| {
                        ConnectorError::Parse(format!("no offset returned for partition {p}"))
                    })
            })
            .collect()
    }

    /// Fetch from `offset`: the records at or after it, and the offset to fetch next.
    #[allow(clippy::type_complexity)]
    fn fetch(
        &mut self,
        topic: &str,
        partition: i32,
        offset: i64,
        max_bytes: i32,
    ) -> Result<(Vec<(i64, Option<Vec<u8>>)>, i64), ConnectorError> {
        let mut request = Writer::default();
        request
            .i32(-1) // replica
            .i32(500) // max wait
            .i32(1) // min bytes
            .i32(max_bytes)
            .i8(1) // read committed
            .i32(1)
            .string(topic)
            .i32(1)
            .i32(partition)
            .i64(offset)
            .i32(max_bytes);
        let response = self.call(API_FETCH, 4, &request.0)?;
        let mut r = Reader::new(&response);
        r.i32()?; // throttle
        let mut data = None;
        for _ in 0..r.count()? {
            r.string()?;
            for _ in 0..r.count()? {
                let p = r.i32()?;
                let code = r.i16()?;
                r.i64()?; // high watermark
                r.i64()?; // last stable offset
                let aborted = r.i32()?;
                if aborted > 0 {
                    return Err(ConnectorError::Parse(format!(
                        "{topic} partition {p} holds aborted transactions, which are not supported"
                    )));
                }
                for _ in 0..aborted.max(0) {
                    r.i64()?;
                    r.i64()?;
                }
                kafka_error(
                    &self.peer,
                    &format!("Fetch {topic} partition {p} at {offset}"),
                    code,
                )?;
                let records = r.bytes()?;
                if p == partition {
                    data = records;
                }
            }
        }
        record_batches(&data.unwrap_or_default(), offset)
    }
}

/// Decode record batches (format v2): the records at or after `from`, and the offset
/// after the last complete batch. A trailing partial batch is left for the next fetch.
#[allow(clippy::type_complexity)]
pub fn record_batches(
    data: &[u8],
    from: i64,
) -> Result<(Vec<(i64, Option<Vec<u8>>)>, i64), ConnectorError> {
    let mut records = Vec::new();
    let mut next = from;
    let mut r = Reader::new(data);
    while r.remaining() >= 12 {
        let base_offset = r.i64()?;
        let length = usize::try_from(r.i32()?)
            .map_err(|_| ConnectorError::Parse("a negative record batch length".into()))?;
        if r.remaining() < length {
            break;
        }
        let batch = r.take(length)?;
        let mut b = Reader::new(batch);
        b.i32()?; // partition leader epoch
        let magic = b.i8()?;
        if magic != 2 {
            return Err(ConnectorError::Parse(format!(
                "record batch format {magic} is not supported (only v2)"
            )));
        }
        let crc = b.u32()?;
        if crc32c(&batch[b.position()..]) != crc {
            return Err(ConnectorError::Parse(format!(
                "the record batch at offset {base_offset} fails its CRC-32C check"
            )));
        }
        let attributes = b.i16()?;
        let last_offset_delta = b.i32()?;
        next = next.max(base_offset + i64::from(last_offset_delta) + 1);
        if attributes & 0x07 != 0 {
            return Err(ConnectorError::Parse(format!(
                "the record batch at offset {base_offset} is compressed; compression is not \
                 supported"
            )));
        }
        if attributes & 0x20 != 0 {
            continue; // a control batch: transaction markers, no data
        }
        b.take(8 + 8 + 8 + 2 + 4)?; // timestamps, producer id and epoch, base sequence
        let count = b.i32()?;
        for _ in 0..count.max(0) {
            let len = usize::try_from(b.varint()?)
                .map_err(|_| ConnectorError::Parse("a negative record length".into()))?;
            let mut record = Reader::new(b.take(len)?);
            record.i8()?; // attributes
            let offset = base_offset + record.varint()?;
            record.varint()?; // timestamp delta
            let key_len = record.varint()?;
            if key_len > 0 {
                record.take(key_len as usize)?;
            }
            let value_len = record.varint()?;
            let value = if value_len < 0 {
                None
            } else {
                Some(record.take(value_len as usize)?.to_vec())
            };
            if offset >= from {
                records.push((offset, value));
            }
        }
    }
    Ok((records, next))
}

/// CRC-32C (Castagnoli), as record batches carry it.
pub fn crc32c(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0x82F6_3B78
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn kafka_error(peer: &str, what: &str, code: i16) -> Result<(), ConnectorError> {
    if code == 0 {
        return Ok(());
    }
    let detail = format!("{peer}: {what}: {} ({code})", error_name(code));
    Err(match code {
        31 | 33 | 58 => ConnectorError::Credential(detail),
        _ => ConnectorError::Io(detail),
    })
}

fn error_name(code: i16) -> &'static str {
    match code {
        1 => "OFFSET_OUT_OF_RANGE",
        3 => "UNKNOWN_TOPIC_OR_PARTITION",
        6 => "NOT_LEADER_OR_FOLLOWER",
        29 => "TOPIC_AUTHORIZATION_FAILED",
        31 => "CLUSTER_AUTHORIZATION_FAILED",
        33 => "UNSUPPORTED_SASL_MECHANISM",
        35 => "UNSUPPORTED_VERSION",
        58 => "SASL_AUTHENTICATION_FAILED",
        _ => "an error",
    }
}

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
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
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, p: 0 }
    }
    fn position(&self) -> usize {
        self.p
    }
    fn remaining(&self) -> usize {
        self.b.len() - self.p
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], ConnectorError> {
        let end = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| ConnectorError::Parse("a truncated Kafka message".into()))?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn i8(&mut self) -> Result<i8, ConnectorError> {
        Ok(self.take(1)?[0] as i8)
    }
    fn i16(&mut self) -> Result<i16, ConnectorError> {
        let b = self.take(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }
    fn i32(&mut self) -> Result<i32, ConnectorError> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u32(&mut self) -> Result<u32, ConnectorError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i64(&mut self) -> Result<i64, ConnectorError> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(i64::from_be_bytes(a))
    }
    /// An array length; a null array counts as empty.
    fn count(&mut self) -> Result<i32, ConnectorError> {
        Ok(self.i32()?.max(0))
    }
    fn string(&mut self) -> Result<Option<String>, ConnectorError> {
        let len = self.i16()?;
        if len < 0 {
            return Ok(None);
        }
        Ok(Some(
            String::from_utf8_lossy(self.take(len as usize)?).into_owned(),
        ))
    }
    fn bytes(&mut self) -> Result<Option<Vec<u8>>, ConnectorError> {
        let len = self.i32()?;
        if len < 0 {
            return Ok(None);
        }
        Ok(Some(self.take(len as usize)?.to_vec()))
    }
    /// A zigzag-encoded variable-length integer.
    fn varint(&mut self) -> Result<i64, ConnectorError> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.take(1)?[0];
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(((value >> 1) as i64) ^ -((value & 1) as i64));
            }
        }
        Err(ConnectorError::Parse("a varint longer than 64 bits".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32c_matches_the_check_value() {
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(b""), 0);
    }

    #[test]
    fn checkpoints_round_trip_and_malformed_ones_are_refused() {
        assert_eq!(parse_checkpoint("0=42,1=17").unwrap(), [(0, 42), (1, 17)]);
        assert!(parse_checkpoint("").unwrap().is_empty());
        for bad in ["0", "0=x", "a=1", "0=1,,x"] {
            assert!(parse_checkpoint(bad).is_err(), "{bad}");
        }
    }
}
