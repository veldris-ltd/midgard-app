//! A **PostgreSQL** connector: a hand-rolled client for the part of the v3 frontend/
//! backend protocol a read-only sync needs. [FR-F-2] [NFR-SEC-1] [NFR-PRV-1]
//!
//! - **Egress** is checked against the sandbox before any connection, for exactly the
//!   configured `host:port`.
//! - **Authentication** is SCRAM-SHA-256 only, and mutual: the server must prove it
//!   holds the password's verifier. A server that asks for a cleartext or MD5 password,
//!   or lets the connection in without authenticating it, is refused. The password is
//!   a [`CredentialRef`] resolved at sync time.
//! - **Reads only**: the query runs inside `BEGIN TRANSACTION READ ONLY`, with the
//!   extended query protocol — a watermark is sent as a bound parameter, never spliced
//!   into SQL — and rows arrive in batches of `batch_rows`.
//! - **Typed at the edge**: the result's columns must be exactly the declared schema's
//!   (in any order) with compatible types, or the sync fails as schema drift; each value
//!   is parsed into its declared type.
//! - **Incremental**: with a watermark column, the query compares it with `$1`; the
//!   first sync binds the configured start, later syncs the checkpoint, and the new
//!   checkpoint is the greatest watermark read.
//!
//! TLS is not spoken here: in an enclosure the egress broker terminates it (see
//! `deploy/`), so this client connects to the broker on loopback.

use std::cmp::Ordering;
use std::io::{Read, Write};
use std::net::TcpStream;

use forge_dataset_store::{ColumnType, DataValue, Dataset, Row, Schema};
use pqc_agility::{OsRng, Rng};

use crate::connector::{Connector, ConnectorDescriptor, ConnectorKind, SyncContext, SyncOutcome};
use crate::credential::CredentialRef;
use crate::error::ConnectorError;
use crate::net;
use crate::wire::{base64_encode, ScramSha256};

/// Protocol version 3.0.
const PROTOCOL_3_0: i32 = 196_608;
/// The largest backend message accepted.
const MAX_MESSAGE: usize = 64 * 1024 * 1024;
/// Rows fetched per round trip unless configured otherwise.
pub const DEFAULT_BATCH_ROWS: i32 = 10_000;

/// An incremental sync's watermark: the column compared with `$1`, and the value the
/// first sync starts from.
#[derive(Debug, Clone)]
pub struct Watermark {
    /// The watermark column, which must be declared as `int`, `float` or `text`.
    pub column: String,
    /// The value bound for the first sync.
    pub start: String,
}

/// A PostgreSQL source.
#[derive(Debug, Clone)]
pub struct PostgresConnector {
    host: String,
    port: u16,
    database: String,
    user: String,
    password: CredentialRef,
    query: String,
    schema: Schema,
    watermark: Option<Watermark>,
    batch_rows: i32,
}

impl PostgresConnector {
    /// Bind a query on `database` at `host:port`, run as `user` with the password
    /// `password` resolves to, into `schema`.
    pub fn new(
        host: impl Into<String>,
        port: u16,
        database: impl Into<String>,
        user: impl Into<String>,
        password: CredentialRef,
        query: impl Into<String>,
        schema: Schema,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            database: database.into(),
            user: user.into(),
            password,
            query: query.into(),
            schema,
            watermark: None,
            batch_rows: DEFAULT_BATCH_ROWS,
        }
    }

    /// Sync incrementally on `column`, starting from `start`.
    pub fn incremental(mut self, column: impl Into<String>, start: impl Into<String>) -> Self {
        self.watermark = Some(Watermark {
            column: column.into(),
            start: start.into(),
        });
        self
    }

    /// Fetch `rows` rows per round trip (at least one).
    pub fn batch_rows(mut self, rows: i32) -> Self {
        self.batch_rows = rows.max(1);
        self
    }

    fn watermark_index(&self) -> Result<Option<usize>, ConnectorError> {
        let Some(watermark) = &self.watermark else {
            return Ok(None);
        };
        if !self.query.contains("$1") {
            return Err(ConnectorError::Parse(
                "an incremental query must compare its watermark column with $1".into(),
            ));
        }
        let index = self.schema.index_of(&watermark.column).ok_or_else(|| {
            ConnectorError::Parse(format!(
                "the watermark column '{}' is not a declared column",
                watermark.column
            ))
        })?;
        match self.schema.columns[index].ty {
            ColumnType::Int | ColumnType::Float | ColumnType::Text => Ok(Some(index)),
            other => Err(ConnectorError::Parse(format!(
                "the watermark column '{}' is {}; it must be int, float or text",
                watermark.column,
                other.as_str()
            ))),
        }
    }
}

impl Connector for PostgresConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: ConnectorKind::Database,
            source: format!("postgres:{}:{}/{}", self.host, self.port, self.database),
            streaming: false,
        }
    }

    fn egress_hosts(&self) -> Vec<String> {
        vec![format!("{}:{}", self.host, self.port)]
    }

    fn sync(&self, ctx: &SyncContext) -> Result<Dataset, ConnectorError> {
        self.sync_incremental(ctx).map(|outcome| outcome.dataset)
    }

    fn sync_incremental(&self, ctx: &SyncContext) -> Result<SyncOutcome, ConnectorError> {
        let watermark_index = self.watermark_index()?;
        let from = self
            .watermark
            .as_ref()
            .map(|w| ctx.checkpoint.unwrap_or(&w.start).to_string());

        let secret = ctx.credentials.resolve(&self.password)?;
        let password = std::str::from_utf8(secret.expose())
            .map_err(|_| ConnectorError::Credential("the database password is not UTF-8".into()))?;

        let stream = net::connect(ctx.sandbox, &self.host, self.port)?;
        let mut session = Session {
            stream,
            peer: format!("{}:{}", self.host, self.port),
        };
        session.startup(&self.user, &self.database, password)?;
        session.simple("BEGIN TRANSACTION READ ONLY")?;
        let rows = session.query(&self.query, from.as_deref(), &self.schema, self.batch_rows)?;
        session.terminate();

        let checkpoint = match (watermark_index, from) {
            (Some(index), Some(from)) => Some(greatest(&rows, index).unwrap_or(from)),
            _ => None,
        };
        Ok(SyncOutcome {
            dataset: Dataset::new(self.schema.clone(), rows)?,
            checkpoint,
        })
    }
}

/// The greatest non-null value in column `index`, as the text a later sync binds.
fn greatest(rows: &[Row], index: usize) -> Option<String> {
    rows.iter()
        .map(|row| &row[index])
        .filter(|v| !matches!(v, DataValue::Null))
        .max_by(|a, b| match (a, b) {
            (DataValue::Int(x), DataValue::Int(y)) => x.cmp(y),
            (DataValue::Float(x), DataValue::Float(y)) => {
                x.partial_cmp(y).unwrap_or(Ordering::Equal)
            }
            (DataValue::Text(x), DataValue::Text(y)) => x.cmp(y),
            _ => Ordering::Equal,
        })
        .map(|v| match v {
            DataValue::Int(i) => i.to_string(),
            DataValue::Float(f) => f.to_string(),
            DataValue::Text(t) => t.clone(),
            _ => String::new(),
        })
}

/// One connection to the server.
struct Session {
    stream: TcpStream,
    peer: String,
}

impl Session {
    fn io(&self, e: std::io::Error) -> ConnectorError {
        ConnectorError::Io(format!("{}: {e}", self.peer))
    }

    fn send(&mut self, tag: Option<u8>, body: &[u8]) -> Result<(), ConnectorError> {
        let mut frame = Vec::with_capacity(body.len() + 5);
        frame.extend(tag);
        let len = i32::try_from(body.len() + 4)
            .map_err(|_| ConnectorError::Parse("a message is too large to send".into()))?;
        frame.extend_from_slice(&len.to_be_bytes());
        frame.extend_from_slice(body);
        self.stream.write_all(&frame).map_err(|e| self.io(e))
    }

    fn recv(&mut self) -> Result<(u8, Vec<u8>), ConnectorError> {
        let mut head = [0u8; 5];
        self.stream.read_exact(&mut head).map_err(|e| self.io(e))?;
        let len = i32::from_be_bytes([head[1], head[2], head[3], head[4]]);
        let len = usize::try_from(len)
            .ok()
            .filter(|&l| (4..=MAX_MESSAGE).contains(&l))
            .ok_or_else(|| {
                ConnectorError::Parse(format!("{}: a malformed message length {len}", self.peer))
            })?;
        let mut body = vec![0u8; len - 4];
        self.stream.read_exact(&mut body).map_err(|e| self.io(e))?;
        Ok((head[0], body))
    }

    /// Authenticate with SCRAM-SHA-256 and wait until the server is ready.
    fn startup(
        &mut self,
        user: &str,
        database: &str,
        password: &str,
    ) -> Result<(), ConnectorError> {
        let mut body = PROTOCOL_3_0.to_be_bytes().to_vec();
        for (key, value) in [
            ("user", user),
            ("database", database),
            ("application_name", "forge-worker"),
        ] {
            put_cstr(&mut body, key);
            put_cstr(&mut body, value);
        }
        body.push(0);
        self.send(None, &body)?;

        let mut scram: Option<ScramSha256> = None;
        let mut verified = false;
        loop {
            let (tag, body) = self.recv()?;
            match tag {
                b'R' => {
                    let mut r = Reader::new(&body);
                    let code = r.i32()?;
                    let data = r.rest();
                    match code {
                        0 => {}
                        10 => {
                            let offered = mechanisms(data);
                            if !offered.iter().any(|m| m == "SCRAM-SHA-256") {
                                return Err(ConnectorError::Credential(format!(
                                    "{} offers {}; only SCRAM-SHA-256 is accepted",
                                    self.peer,
                                    offered.join(", ")
                                )));
                            }
                            // PostgreSQL takes the user from the startup message and
                            // ignores the SCRAM username.
                            let client = ScramSha256::new("", &nonce());
                            let first = client.client_first();
                            let mut msg = Vec::new();
                            put_cstr(&mut msg, "SCRAM-SHA-256");
                            msg.extend_from_slice(&(first.len() as i32).to_be_bytes());
                            msg.extend_from_slice(first.as_bytes());
                            self.send(Some(b'p'), &msg)?;
                            scram = Some(client);
                        }
                        11 => {
                            let client = scram.as_mut().ok_or_else(|| {
                                ConnectorError::Parse("a SCRAM challenge before SCRAM began".into())
                            })?;
                            let server_first = std::str::from_utf8(data).map_err(|_| {
                                ConnectorError::Parse("a SCRAM challenge that is not UTF-8".into())
                            })?;
                            let reply = client
                                .client_final(server_first, password)
                                .map_err(ConnectorError::Credential)?;
                            self.send(Some(b'p'), reply.as_bytes())?;
                        }
                        12 => {
                            let client = scram.as_ref().ok_or_else(|| {
                                ConnectorError::Parse("a SCRAM outcome before SCRAM began".into())
                            })?;
                            let server_final = std::str::from_utf8(data).map_err(|_| {
                                ConnectorError::Parse("a SCRAM outcome that is not UTF-8".into())
                            })?;
                            client
                                .verify_server_final(server_final)
                                .map_err(ConnectorError::Credential)?;
                            verified = true;
                        }
                        3 | 5 => {
                            return Err(ConnectorError::Credential(format!(
                                "{} asks for a {} password; only SCRAM-SHA-256 is accepted",
                                self.peer,
                                if code == 3 { "cleartext" } else { "MD5" }
                            )))
                        }
                        other => {
                            return Err(ConnectorError::Credential(format!(
                                "{} asks for authentication method {other}; only SCRAM-SHA-256 \
                                 is accepted",
                                self.peer
                            )))
                        }
                    }
                }
                b'S' | b'K' | b'N' => {}
                b'Z' if verified => return Ok(()),
                b'Z' => {
                    return Err(ConnectorError::Credential(format!(
                        "{} let the connection in without authenticating it with SCRAM-SHA-256",
                        self.peer
                    )))
                }
                b'E' => return Err(ConnectorError::Credential(server_error(&self.peer, &body))),
                other => {
                    return Err(ConnectorError::Parse(format!(
                        "{}: an unexpected message '{}' during startup",
                        self.peer,
                        char::from(other)
                    )))
                }
            }
        }
    }

    /// Run a statement with the simple query protocol and wait until the server is
    /// ready again.
    fn simple(&mut self, sql: &str) -> Result<(), ConnectorError> {
        let mut body = Vec::new();
        put_cstr(&mut body, sql);
        self.send(Some(b'Q'), &body)?;
        let mut failure = None;
        loop {
            let (tag, body) = self.recv()?;
            match tag {
                b'E' => failure = Some(server_error(&self.peer, &body)),
                b'Z' => return failure.map_or(Ok(()), |e| Err(ConnectorError::Io(e))),
                _ => {}
            }
        }
    }

    /// Run `sql` with the extended query protocol, binding `param` as `$1` when given,
    /// and read every row into `schema`.
    fn query(
        &mut self,
        sql: &str,
        param: Option<&str>,
        schema: &Schema,
        batch_rows: i32,
    ) -> Result<Vec<Row>, ConnectorError> {
        let mut parse = vec![0u8];
        put_cstr(&mut parse, sql);
        parse.extend_from_slice(&0i16.to_be_bytes());
        self.send(Some(b'P'), &parse)?;

        let mut bind = vec![0u8, 0u8];
        bind.extend_from_slice(&0i16.to_be_bytes());
        match param {
            Some(value) => {
                bind.extend_from_slice(&1i16.to_be_bytes());
                bind.extend_from_slice(&(value.len() as i32).to_be_bytes());
                bind.extend_from_slice(value.as_bytes());
            }
            None => bind.extend_from_slice(&0i16.to_be_bytes()),
        }
        bind.extend_from_slice(&0i16.to_be_bytes());
        self.send(Some(b'B'), &bind)?;
        self.send(Some(b'D'), &[b'P', 0])?;
        self.execute(batch_rows)?;

        let mut positions: Option<Vec<usize>> = None;
        let mut rows = Vec::new();
        let outcome = loop {
            let (tag, body) = self.recv()?;
            match tag {
                b'1' | b'2' | b'N' => {}
                b'T' => match describe(&body, schema) {
                    Ok(p) => positions = Some(p),
                    Err(e) => break Err(e),
                },
                b'n' => {
                    break Err(ConnectorError::Parse(
                        "the query returns no rows: only a SELECT can be synced".into(),
                    ))
                }
                b'D' => {
                    let Some(positions) = &positions else {
                        break Err(ConnectorError::Parse(
                            "a row arrived before its description".into(),
                        ));
                    };
                    match decode_row(&body, positions, schema, rows.len()) {
                        Ok(row) => rows.push(row),
                        Err(e) => break Err(e),
                    }
                }
                b's' => self.execute(batch_rows)?,
                b'C' => break Ok(()),
                b'E' => break Err(ConnectorError::Io(server_error(&self.peer, &body))),
                other => {
                    break Err(ConnectorError::Parse(format!(
                        "{}: an unexpected message '{}' during a query",
                        self.peer,
                        char::from(other)
                    )))
                }
            }
        };

        // Close the extended query either way, and wait for the server.
        self.send(Some(b'S'), &[])?;
        loop {
            let (tag, _) = self.recv()?;
            if tag == b'Z' {
                break;
            }
        }
        outcome.map(|()| rows)
    }

    fn execute(&mut self, batch_rows: i32) -> Result<(), ConnectorError> {
        let mut execute = vec![0u8];
        execute.extend_from_slice(&batch_rows.to_be_bytes());
        self.send(Some(b'E'), &execute)?;
        self.send(Some(b'H'), &[])
    }

    fn terminate(mut self) {
        let _ = self.send(Some(b'X'), &[]);
    }
}

/// Map the result's columns onto the schema: the positions of each declared column, or
/// schema drift.
fn describe(body: &[u8], schema: &Schema) -> Result<Vec<usize>, ConnectorError> {
    let mut r = Reader::new(body);
    let count = usize::try_from(r.i16()?).unwrap_or(0);
    let mut columns = Vec::with_capacity(count);
    for _ in 0..count {
        let name = r.cstr()?;
        r.take(6)?; // table oid, attribute number
        let oid = r.i32()?;
        r.take(8)?; // type size, type modifier, format
        columns.push((name, oid));
    }
    let returned: Vec<&str> = columns.iter().map(|(n, _)| n.as_str()).collect();
    let declared: Vec<&str> = schema.columns.iter().map(|c| c.name.as_str()).collect();
    let drift = || {
        ConnectorError::Parse(format!(
            "schema drift: the query returns columns [{}] but the job declares [{}]",
            returned.join(", "),
            declared.join(", ")
        ))
    };
    if columns.len() != schema.width() {
        return Err(drift());
    }
    schema
        .columns
        .iter()
        .map(|column| {
            let position = columns
                .iter()
                .position(|(name, _)| *name == column.name)
                .ok_or_else(drift)?;
            let oid = columns[position].1;
            if compatible(oid, column.ty) {
                Ok(position)
            } else {
                Err(ConnectorError::Parse(format!(
                    "schema drift: column '{}' has PostgreSQL type {} ({oid}), which cannot be \
                     read as {}",
                    column.name,
                    type_name(oid),
                    column.ty.as_str()
                )))
            }
        })
        .collect()
}

/// Whether a column of PostgreSQL type `oid` can be read as `ty`. Anything reads as
/// text; numbers and booleans only from their own types.
fn compatible(oid: i32, ty: ColumnType) -> bool {
    match ty {
        ColumnType::Text => true,
        ColumnType::Int => matches!(oid, 20 | 21 | 23),
        ColumnType::Float => matches!(oid, 20 | 21 | 23 | 700 | 701 | 1700),
        ColumnType::Bool => oid == 16,
        ColumnType::Bytes => false,
    }
}

fn type_name(oid: i32) -> &'static str {
    match oid {
        16 => "boolean",
        17 => "bytea",
        20 => "bigint",
        21 => "smallint",
        23 => "integer",
        25 => "text",
        700 => "real",
        701 => "double precision",
        1042 => "character",
        1043 => "character varying",
        1082 => "date",
        1114 => "timestamp",
        1184 => "timestamptz",
        1700 => "numeric",
        2950 => "uuid",
        3802 => "jsonb",
        _ => "another type",
    }
}

fn decode_row(
    body: &[u8],
    positions: &[usize],
    schema: &Schema,
    row: usize,
) -> Result<Row, ConnectorError> {
    let mut r = Reader::new(body);
    let count = usize::try_from(r.i16()?).unwrap_or(0);
    let mut values: Vec<Option<&[u8]>> = Vec::with_capacity(count);
    for _ in 0..count {
        let len = r.i32()?;
        values.push(if len < 0 {
            None
        } else {
            Some(r.take(len as usize)?)
        });
    }
    schema
        .columns
        .iter()
        .zip(positions)
        .map(|(column, &position)| {
            let raw = values.get(position).copied().ok_or_else(|| {
                ConnectorError::Parse(format!("row {row}: fewer values than columns"))
            })?;
            let Some(raw) = raw else {
                return if column.nullable {
                    Ok(DataValue::Null)
                } else {
                    Err(ConnectorError::Parse(format!(
                        "row {row}: NULL in non-nullable column '{}'",
                        column.name
                    )))
                };
            };
            let text = std::str::from_utf8(raw).map_err(|_| {
                ConnectorError::Parse(format!("row {row}: column '{}' is not UTF-8", column.name))
            })?;
            let value = match column.ty {
                ColumnType::Int => text.parse().ok().map(DataValue::Int),
                ColumnType::Float => text.parse().ok().map(DataValue::Float),
                ColumnType::Bool => match text {
                    "t" => Some(DataValue::Bool(true)),
                    "f" => Some(DataValue::Bool(false)),
                    _ => None,
                },
                ColumnType::Text => Some(DataValue::Text(text.to_string())),
                ColumnType::Bytes => None,
            };
            value.ok_or_else(|| {
                ConnectorError::Parse(format!(
                    "row {row}: '{text}' is not a valid {} for column '{}'",
                    column.ty.as_str(),
                    column.name
                ))
            })
        })
        .collect()
}

/// The mechanisms an AuthenticationSASL message offers.
fn mechanisms(data: &[u8]) -> Vec<String> {
    data.split(|&b| b == 0)
        .filter(|m| !m.is_empty())
        .map(|m| String::from_utf8_lossy(m).into_owned())
        .collect()
}

/// An ErrorResponse as `SEVERITY CODE: message`.
fn server_error(peer: &str, body: &[u8]) -> String {
    let (mut severity, mut code, mut message) = (String::new(), String::new(), String::new());
    for field in body.split(|&b| b == 0).filter(|f| !f.is_empty()) {
        let value = String::from_utf8_lossy(&field[1..]).into_owned();
        match field[0] {
            b'S' => severity = value,
            b'C' => code = value,
            b'M' => message = value,
            _ => {}
        }
    }
    format!("{peer}: {severity} {code}: {message}")
}

/// A fresh SCRAM nonce: 18 random bytes as base64 (never a comma).
fn nonce() -> String {
    let mut bytes = [0u8; 18];
    OsRng.fill(&mut bytes);
    base64_encode(&bytes)
}

fn put_cstr(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, p: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], ConnectorError> {
        let end = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| ConnectorError::Parse("a truncated message".into()))?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn i16(&mut self) -> Result<i16, ConnectorError> {
        let b = self.take(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }
    fn i32(&mut self) -> Result<i32, ConnectorError> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn cstr(&mut self) -> Result<String, ConnectorError> {
        let rest = &self.b[self.p..];
        let end = rest
            .iter()
            .position(|&c| c == 0)
            .ok_or_else(|| ConnectorError::Parse("an unterminated string".into()))?;
        let s = String::from_utf8_lossy(&rest[..end]).into_owned();
        self.p += end + 1;
        Ok(s)
    }
    fn rest(&mut self) -> &'a [u8] {
        let s = &self.b[self.p..];
        self.p = self.b.len();
        s
    }
}
