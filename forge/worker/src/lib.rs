//! # forge-worker — Forge jobs as a running service
//!
//! A job names a source, how its rows become objects and links, and how often it runs.
//! Each run: [FR-F-2] [FR-F-3] [FR-F-4]
//!
//! 1. syncs the source through a Forge connector — a delimited file, a PostgreSQL
//!    query, the objects under an S3 prefix, or a Kafka topic — reaching only the one
//!    `host:port` the job declares, and resuming from the job's checkpoint;
//! 2. commits what it read to the worker's sealed, content-addressed dataset store on
//!    the job's branch, with ingest lineage; [RC-11]
//! 3. projects the rows with Forge's projection onto object and link instances,
//!    carrying the job's marking and purposes onto every object; [NFR-SEC-2]
//! 4. and hands them to the enclosure's gateway as import-format records, where they are
//!    authorised, audited and checked against the ontology before they are written.
//!
//! The jobs file is read strictly:
//!
//! ```json
//! {
//!   "version": 1,
//!   "jobs": [{
//!     "id": "aircraft", "every": 60,
//!     "source": { "file": "aircraft.csv", "delimiter": ",", "header": true,
//!                 "columns": [ { "name": "tail", "type": "text" },
//!                              { "name": "hours", "type": "int", "nullable": true },
//!                              { "name": "base", "type": "text" } ] },
//!     "objects": { "type": "aircraft", "key": "tail", "marking": "internal",
//!                  "compartments": ["OPS-N"], "purposes": ["operations"],
//!                  "properties": { "tail": "tail", "flight_hours": "hours" } },
//!     "links": [ { "type": "aircraft_base", "from": "tail", "to": "base" } ]
//!   }]
//! }
//! ```
//!
//! A source is exactly one of:
//!
//! - `"file": "aircraft.csv"` — relative to the jobs file;
//! - `"postgres": { "host", "port", "database", "user", "password", "query",
//!   "watermark": { "column", "start" }, "batchRows" }` — `watermark` and `batchRows`
//!   optional; with a watermark the query compares its column with `$1`;
//! - `"s3": { "host", "port", "region", "bucket", "prefix", "accessKey", "secretKey" }` —
//!   `prefix` optional;
//! - `"kafka": { "host", "port", "topic", "user", "password", "startFrom" }` — `user` and
//!   `password` together or not at all, `startFrom` `earliest` (default) or `latest`.
//!
//! `password` and `secretKey` name secrets, not their values: files in the worker's
//! secrets directory. `delimiter` and `header` apply to files and S3 objects, and
//! `delimiter` to Kafka values. Column types are `text`, `int`, `float` and `bool`.
//! `properties` maps each object property to the column it comes from; `key`, and a
//! link's `from` and `to`, name the columns holding object keys. Unknown or repeated
//! keys, an unknown column or type, a marking that is not a band, a job run less than
//! every second and a repeated job id all refuse the file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use audit_writer::{SealedJournal, Sealer};
use fabric::{LinkInstance, ObjectInstance, ObjectStore, Value};
use forge_connector_sdk::{
    Connector, ConnectorError, CredentialRef, CredentialResolver, FileConnector, KafkaConnector,
    PostgresConnector, S3Connector, SandboxPolicy, Secret, StartFrom, SyncContext,
};
use forge_dataset_store::{Column, ColumnType, DurableDatasetStore, Lineage, Schema};
use forge_projection::{
    project_links, project_objects, LinkMapping, ObjectMapping, PropertyMapping,
};
use kms::TrustRoot;
use policy::{Compartment, Marking, Purpose};
use pqc_agility::OsRng;
use service_runtime::json::{parse, Json};
use service_runtime::marking::{band, level_of};
use service_runtime::strict_json::{
    array, identifier, object, only_keys, optional_array, optional_bool, strings, text, unique,
};

pub mod egress;

/// The one jobs file version this build reads.
pub const JOBS_FILE_VERSION: i64 = 1;

/// Where a job reads from.
#[derive(Debug, Clone)]
pub enum Source {
    /// A delimited file on disk.
    File {
        /// The file.
        path: PathBuf,
        /// The field delimiter.
        delimiter: char,
        /// Whether the first line is a header.
        header: bool,
    },
    /// A PostgreSQL query.
    Postgres {
        /// Server host.
        host: String,
        /// Server port.
        port: u16,
        /// Database.
        database: String,
        /// User.
        user: String,
        /// The secret holding the password.
        password: String,
        /// The query.
        query: String,
        /// The watermark column and its start, for incremental syncs.
        watermark: Option<(String, String)>,
        /// Rows per round trip.
        batch_rows: i32,
    },
    /// Delimited objects under an S3 prefix.
    S3 {
        /// Endpoint host.
        host: String,
        /// Endpoint port.
        port: u16,
        /// Region.
        region: String,
        /// Bucket.
        bucket: String,
        /// Key prefix.
        prefix: String,
        /// Access key id.
        access_key: String,
        /// The secret holding the secret access key.
        secret_key: String,
        /// The field delimiter.
        delimiter: char,
        /// Whether each object has a header line.
        header: bool,
    },
    /// A Kafka topic.
    Kafka {
        /// Bootstrap broker host.
        host: String,
        /// Bootstrap broker port.
        port: u16,
        /// Topic.
        topic: String,
        /// SASL SCRAM-SHA-256 user and the secret holding the password.
        sasl: Option<(String, String)>,
        /// Where partitions with no checkpoint start.
        start: StartFrom,
        /// The field delimiter of each value.
        delimiter: char,
    },
}

/// One declared job.
#[derive(Debug, Clone)]
pub struct Job {
    /// The job's id — also the dataset branch its versions are committed to.
    pub id: String,
    /// Seconds between runs.
    pub every: u64,
    /// Where it reads from.
    pub source: Source,
    /// The source's columns.
    pub schema: Schema,
    /// How rows become objects.
    pub objects: ObjectMapping,
    /// How rows become links.
    pub links: Vec<LinkMapping>,
}

impl Job {
    /// The connector that reads this job's source.
    pub fn connector(&self) -> Box<dyn Connector> {
        let schema = self.schema.clone();
        match &self.source {
            Source::File {
                path,
                delimiter,
                header,
            } => Box::new(FileConnector::new(path, schema, *delimiter, *header)),
            Source::Postgres {
                host,
                port,
                database,
                user,
                password,
                query,
                watermark,
                batch_rows,
            } => {
                let connector = PostgresConnector::new(
                    host.as_str(),
                    *port,
                    database.as_str(),
                    user.as_str(),
                    CredentialRef::new(password.as_str()),
                    query.as_str(),
                    schema,
                )
                .batch_rows(*batch_rows);
                Box::new(match watermark {
                    Some((column, start)) => connector.incremental(column.as_str(), start.as_str()),
                    None => connector,
                })
            }
            Source::S3 {
                host,
                port,
                region,
                bucket,
                prefix,
                access_key,
                secret_key,
                delimiter,
                header,
            } => Box::new(
                S3Connector::new(
                    host.as_str(),
                    *port,
                    region.as_str(),
                    bucket.as_str(),
                    prefix.as_str(),
                    access_key.as_str(),
                    CredentialRef::new(secret_key.as_str()),
                    schema,
                )
                .delimited(*delimiter, *header),
            ),
            Source::Kafka {
                host,
                port,
                topic,
                sasl,
                start,
                delimiter,
            } => {
                let connector = KafkaConnector::new(host.as_str(), *port, topic.as_str(), schema)
                    .start_from(*start)
                    .delimiter(*delimiter);
                Box::new(match sasl {
                    Some((user, password)) => {
                        connector.scram(user.as_str(), CredentialRef::new(password.as_str()))
                    }
                    None => connector,
                })
            }
        }
    }
}

/// Parse a jobs file; relative source paths resolve against `base`.
pub fn parse_jobs(text_in: &str, base: &Path) -> Result<Vec<Job>, String> {
    let doc = parse(text_in).map_err(|e| format!("not JSON: {e}"))?;
    only_keys(object(&doc, "the file")?, "the file", &["version", "jobs"])?;
    match doc.get("version") {
        Some(Json::Int(JOBS_FILE_VERSION)) => {}
        Some(other) => return Err(format!("unsupported version {}", other.encode())),
        None => return Err("missing \"version\"".into()),
    }
    let jobs = array(&doc, "the file", "jobs")?
        .iter()
        .enumerate()
        .map(|(i, item)| job(&format!("jobs[{i}]"), item, base))
        .collect::<Result<Vec<_>, _>>()?;
    if jobs.is_empty() {
        return Err("\"jobs\" declares no jobs".into());
    }
    unique(jobs.iter().map(|j| j.id.as_str()), "job id")?;
    Ok(jobs)
}

fn job(at: &str, item: &Json, base: &Path) -> Result<Job, String> {
    only_keys(
        object(item, at)?,
        at,
        &["id", "every", "source", "objects", "links"],
    )?;
    let id = identifier(item, at, "id")?.to_string();
    let at = format!("job {id}");
    let every = match item.get("every") {
        Some(Json::Int(n)) if *n >= 1 => {
            u64::try_from(*n).map_err(|_| format!("{at}: bad \"every\""))?
        }
        Some(other) => {
            return Err(format!(
                "{at}: \"every\" must be a whole number of seconds, at least 1, not {}",
                other.encode()
            ))
        }
        None => return Err(format!("{at}: missing \"every\"")),
    };

    let source_item = item
        .get("source")
        .ok_or_else(|| format!("{at}: missing \"source\""))?;
    let source_at = format!("{at}: source");
    only_keys(
        object(source_item, &source_at)?,
        &source_at,
        &[
            "file",
            "postgres",
            "s3",
            "kafka",
            "delimiter",
            "header",
            "columns",
        ],
    )?;
    let columns = array(source_item, &source_at, "columns")?
        .iter()
        .enumerate()
        .map(|(i, c)| column(&format!("{source_at}: columns[{i}]"), c))
        .collect::<Result<Vec<_>, _>>()?;
    if columns.is_empty() {
        return Err(format!("{source_at}: \"columns\" declares no columns"));
    }
    unique(columns.iter().map(|c| c.name.as_str()), "column")
        .map_err(|e| format!("{source_at}: {e}"))?;
    let schema = Schema::new(columns);
    let has_column = |name: &str, what: &str| {
        if schema.index_of(name).is_some() {
            Ok(())
        } else {
            Err(format!(
                "{at}: {what} names \"{name}\", which is not a source column"
            ))
        }
    };
    let source = source(&source_at, source_item, base, &has_column)?;

    let objects = item
        .get("objects")
        .ok_or_else(|| format!("{at}: missing \"objects\""))?;
    let objects_at = format!("{at}: objects");
    only_keys(
        object(objects, &objects_at)?,
        &objects_at,
        &[
            "type",
            "key",
            "marking",
            "compartments",
            "purposes",
            "properties",
        ],
    )?;
    let type_id = identifier(objects, &objects_at, "type")?;
    let key = text(objects, &objects_at, "key")?;
    has_column(key, "\"key\"")?;
    let marking_band = text(objects, &objects_at, "marking")?;
    let level = level_of(marking_band).ok_or_else(|| {
        format!(
            "{objects_at}: \"marking\" must be open, internal, restricted or confidential, not \
             \"{marking_band}\""
        )
    })?;
    let compartments: BTreeSet<Compartment> = match objects.get("compartments") {
        None => BTreeSet::new(),
        Some(_) => strings(objects, &objects_at, "compartments")?
            .into_iter()
            .map(Compartment)
            .collect(),
    };
    let purposes: BTreeSet<Purpose> = strings(objects, &objects_at, "purposes")?
        .into_iter()
        .map(Purpose)
        .collect();
    if purposes.is_empty() {
        return Err(format!(
            "{objects_at}: \"purposes\" needs at least one purpose, or nobody can read them"
        ));
    }
    let properties = match objects.get("properties") {
        Some(Json::Obj(fields)) => {
            service_runtime::strict_json::no_repeated_keys(fields, &objects_at)?;
            fields
                .iter()
                .map(|(property, column)| match column {
                    Json::Str(column) => {
                        has_column(column, &format!("property \"{property}\""))?;
                        Ok(PropertyMapping::new(column.as_str(), property.as_str()))
                    }
                    other => Err(format!(
                        "{objects_at}: property \"{property}\" must name a column, not {}",
                        other.encode()
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        None => Vec::new(),
        Some(other) => {
            return Err(format!(
                "{objects_at}: \"properties\" must be an object, not {}",
                other.encode()
            ))
        }
    };

    let links = optional_array(item, &at, "links")?
        .iter()
        .enumerate()
        .map(|(i, link)| {
            let link_at = format!("{at}: links[{i}]");
            only_keys(object(link, &link_at)?, &link_at, &["type", "from", "to"])?;
            let link_type = identifier(link, &link_at, "type")?;
            let from = text(link, &link_at, "from")?;
            let to = text(link, &link_at, "to")?;
            has_column(from, "a link's \"from\"")?;
            has_column(to, "a link's \"to\"")?;
            Ok(LinkMapping::new(link_type, from, to))
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(Job {
        id,
        every,
        source,
        objects: ObjectMapping::new(
            type_id,
            key,
            properties,
            Marking {
                level,
                compartments,
            },
            purposes,
        ),
        links,
        schema,
    })
}

/// The one source a job's `source` object declares.
fn source(
    at: &str,
    item: &Json,
    base: &Path,
    has_column: &dyn Fn(&str, &str) -> Result<(), String>,
) -> Result<Source, String> {
    let declared: Vec<&str> = ["file", "postgres", "s3", "kafka"]
        .into_iter()
        .filter(|k| item.get(k).is_some())
        .collect();
    let [kind] = declared.as_slice() else {
        return Err(format!(
            "{at}: declare exactly one of \"file\", \"postgres\", \"s3\" or \"kafka\""
        ));
    };
    let delimiter = match item.get("delimiter") {
        None => ',',
        Some(Json::Str(d)) if d.chars().count() == 1 => d.chars().next().unwrap_or(','),
        Some(other) => {
            return Err(format!(
                "{at}: \"delimiter\" must be one character, not {}",
                other.encode()
            ))
        }
    };
    let header = optional_bool(item, at, "header", true)?;

    match *kind {
        "file" => Ok(Source::File {
            path: base.join(text(item, at, "file")?),
            delimiter,
            header,
        }),
        "postgres" => {
            let pg = item.get("postgres").unwrap_or(&Json::Null);
            let at = format!("{at}: postgres");
            only_keys(
                object(pg, &at)?,
                &at,
                &[
                    "host",
                    "port",
                    "database",
                    "user",
                    "password",
                    "query",
                    "watermark",
                    "batchRows",
                ],
            )?;
            let watermark = match pg.get("watermark") {
                None => None,
                Some(w) => {
                    let w_at = format!("{at}: watermark");
                    only_keys(object(w, &w_at)?, &w_at, &["column", "start"])?;
                    let column = text(w, &w_at, "column")?;
                    has_column(column, "the watermark")?;
                    Some((column.to_string(), text(w, &w_at, "start")?.to_string()))
                }
            };
            let batch_rows = match pg.get("batchRows") {
                None => forge_connector_sdk::connectors::postgres::DEFAULT_BATCH_ROWS,
                Some(Json::Int(n)) if (1..=i64::from(i32::MAX)).contains(n) => *n as i32,
                Some(other) => {
                    return Err(format!(
                        "{at}: \"batchRows\" must be a positive whole number, not {}",
                        other.encode()
                    ))
                }
            };
            Ok(Source::Postgres {
                host: text(pg, &at, "host")?.to_string(),
                port: port(pg, &at)?,
                database: text(pg, &at, "database")?.to_string(),
                user: text(pg, &at, "user")?.to_string(),
                password: secret_name(pg, &at, "password")?,
                query: text(pg, &at, "query")?.to_string(),
                watermark,
                batch_rows,
            })
        }
        "s3" => {
            let s3 = item.get("s3").unwrap_or(&Json::Null);
            let at = format!("{at}: s3");
            only_keys(
                object(s3, &at)?,
                &at,
                &[
                    "host",
                    "port",
                    "region",
                    "bucket",
                    "prefix",
                    "accessKey",
                    "secretKey",
                ],
            )?;
            let prefix = match s3.get("prefix") {
                None => String::new(),
                Some(Json::Str(p)) => p.clone(),
                Some(other) => {
                    return Err(format!(
                        "{at}: \"prefix\" must be a string, not {}",
                        other.encode()
                    ))
                }
            };
            Ok(Source::S3 {
                host: text(s3, &at, "host")?.to_string(),
                port: port(s3, &at)?,
                region: text(s3, &at, "region")?.to_string(),
                bucket: text(s3, &at, "bucket")?.to_string(),
                prefix,
                access_key: text(s3, &at, "accessKey")?.to_string(),
                secret_key: secret_name(s3, &at, "secretKey")?,
                delimiter,
                header,
            })
        }
        _ => {
            let kafka = item.get("kafka").unwrap_or(&Json::Null);
            let at = format!("{at}: kafka");
            only_keys(
                object(kafka, &at)?,
                &at,
                &["host", "port", "topic", "user", "password", "startFrom"],
            )?;
            let sasl = match (kafka.get("user"), kafka.get("password")) {
                (None, None) => None,
                (Some(_), Some(_)) => Some((
                    text(kafka, &at, "user")?.to_string(),
                    secret_name(kafka, &at, "password")?,
                )),
                _ => return Err(format!("{at}: \"user\" and \"password\" go together")),
            };
            let start = match kafka.get("startFrom").and_then(Json::as_str) {
                None if kafka.get("startFrom").is_none() => StartFrom::Earliest,
                Some("earliest") => StartFrom::Earliest,
                Some("latest") => StartFrom::Latest,
                _ => {
                    return Err(format!(
                        "{at}: \"startFrom\" must be \"earliest\" or \"latest\""
                    ))
                }
            };
            Ok(Source::Kafka {
                host: text(kafka, &at, "host")?.to_string(),
                port: port(kafka, &at)?,
                topic: text(kafka, &at, "topic")?.to_string(),
                sasl,
                start,
                delimiter,
            })
        }
    }
}

fn port(item: &Json, at: &str) -> Result<u16, String> {
    match item.get("port") {
        Some(Json::Int(n)) => u16::try_from(*n)
            .ok()
            .filter(|&p| p > 0)
            .ok_or_else(|| format!("{at}: \"port\" {n} is not a port")),
        Some(other) => Err(format!(
            "{at}: \"port\" must be a number, not {}",
            other.encode()
        )),
        None => Err(format!("{at}: missing \"port\"")),
    }
}

/// A secret's name: a plain file name in the secrets directory.
fn secret_name(item: &Json, at: &str, key: &str) -> Result<String, String> {
    let name = text(item, at, key)?;
    if valid_secret_name(name) {
        Ok(name.to_string())
    } else {
        Err(format!(
            "{at}: \"{key}\" names a secret, which may contain only letters, digits, _, - and ., \
             not \"{name}\""
        ))
    }
}

fn valid_secret_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn column(at: &str, item: &Json) -> Result<Column, String> {
    only_keys(object(item, at)?, at, &["name", "type", "nullable"])?;
    let name = identifier(item, at, "name")?;
    let ty = match text(item, at, "type")? {
        "text" => ColumnType::Text,
        "int" => ColumnType::Int,
        "float" => ColumnType::Float,
        "bool" => ColumnType::Bool,
        other => {
            return Err(format!(
                "{at}: \"type\" must be text, int, float or bool, not \"{other}\""
            ))
        }
    };
    Ok(if optional_bool(item, at, "nullable", false)? {
        Column::nullable(name, ty)
    } else {
        Column::new(name, ty)
    })
}

/// Credentials from a directory holding one secret per file, named for its reference —
/// a mounted Kubernetes Secret, for instance. A reference must be a plain file name, so
/// nothing outside the directory can be read; one trailing line break is dropped.
/// [NFR-SEC-1]
#[derive(Debug, Clone)]
pub struct SecretsDir(pub PathBuf);

impl CredentialResolver for SecretsDir {
    fn resolve(&self, cref: &CredentialRef) -> Result<Secret, ConnectorError> {
        let name = &cref.0;
        if !valid_secret_name(name) {
            return Err(ConnectorError::Credential(format!(
                "'{name}' is not a secret name"
            )));
        }
        let mut bytes = std::fs::read(self.0.join(name)).map_err(|e| {
            ConnectorError::Credential(format!(
                "secret '{name}' cannot be read from {}: {e}",
                self.0.display()
            ))
        })?;
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        Ok(Secret::new(bytes))
    }
}

/// The AAD domain of the checkpoint journal.
const CHECKPOINTS_DOMAIN: &[u8] = b"midgard-forge-checkpoints-v1";

/// Each job's checkpoint, in a sealed journal, so a restart resumes where the last
/// accepted sync stopped. [RC-11]
pub struct Checkpoints {
    journal: SealedJournal,
    latest: BTreeMap<String, String>,
}

impl Checkpoints {
    /// Open (creating if absent) the journal at `path`, recovering every checkpoint.
    pub fn open(path: impl AsRef<Path>, sealer: Box<dyn Sealer>) -> Result<Self, String> {
        let (journal, records) = SealedJournal::open(path, sealer, CHECKPOINTS_DOMAIN)
            .map_err(|e| format!("the checkpoint journal does not open: {e:?}"))?;
        let mut latest = BTreeMap::new();
        for record in records {
            let text = String::from_utf8(record)
                .map_err(|_| "a checkpoint record is not UTF-8".to_string())?;
            let (job, checkpoint) = text
                .split_once('\0')
                .ok_or_else(|| "a malformed checkpoint record".to_string())?;
            latest.insert(job.to_string(), checkpoint.to_string());
        }
        Ok(Self { journal, latest })
    }

    /// The job's checkpoint, if it has one.
    pub fn get(&self, job: &str) -> Option<&str> {
        self.latest.get(job).map(String::as_str)
    }

    /// Record the job's checkpoint, durably, unless it is unchanged.
    pub fn record(&mut self, job: &str, checkpoint: &str) -> Result<(), String> {
        if self.get(job) == Some(checkpoint) {
            return Ok(());
        }
        self.journal
            .append(format!("{job}\0{checkpoint}").as_bytes())
            .map_err(|e| format!("cannot record the checkpoint: {e:?}"))?;
        self.latest.insert(job.to_string(), checkpoint.to_string());
        Ok(())
    }
}

/// An object store that only collects what projection writes, so the instances can be
/// sent to the enclosure rather than kept here.
#[derive(Debug, Default)]
pub struct Collector {
    /// Projected objects, in row order.
    pub objects: Vec<ObjectInstance>,
    /// Projected links, in row order.
    pub links: Vec<LinkInstance>,
}

impl ObjectStore for Collector {
    fn put_object(&mut self, obj: ObjectInstance) {
        self.objects.push(obj);
    }
    fn get_object(&self, id: &str) -> Option<&ObjectInstance> {
        self.objects.iter().find(|o| o.id == id)
    }
    fn objects_of_type(&self, type_id: &str) -> Vec<&ObjectInstance> {
        self.objects
            .iter()
            .filter(|o| o.type_id == type_id)
            .collect()
    }
    fn by_property(&self, _type_id: &str, _property: &str, _key: &str) -> Vec<&ObjectInstance> {
        Vec::new()
    }
    fn put_link(&mut self, link: LinkInstance) {
        self.links.push(link);
    }
    fn neighbors(&self, _source_id: &str, _link_type: &str) -> Vec<&ObjectInstance> {
        Vec::new()
    }
    fn remove_object(&mut self, id: &str) -> Option<ObjectInstance> {
        let at = self.objects.iter().position(|o| o.id == id)?;
        self.links
            .retain(|l| l.source_id != id && l.target_id != id);
        Some(self.objects.remove(at))
    }
}

/// The collected instances as import-format JSON Lines: one object or link per line,
/// keyed by the row's key value, which the enclosure prefixes with the type.
pub fn records(collected: &Collector) -> String {
    let mut out = String::new();
    for o in &collected.objects {
        let properties = o
            .properties
            .iter()
            .filter_map(|(name, value)| {
                let value = match value {
                    Value::Text(t) => Json::Str(t.clone()),
                    Value::Int(i) => Json::Int(*i),
                    // A non-finite number (a transform's 0/0, say) has no JSON form and
                    // would be written as null, which the enclosure refuses for the whole
                    // sync: left out, as an absent value is.
                    Value::Float(f) if !f.is_finite() => return None,
                    Value::Float(f) => Json::Num(*f),
                    Value::Bool(b) => Json::Bool(*b),
                    Value::Timestamp(t) => Json::Int(i64::try_from(*t).unwrap_or(i64::MAX)),
                };
                Some((name.clone(), value))
            })
            .collect();
        let mut fields = vec![
            ("object", Json::s(&o.type_id)),
            ("key", Json::s(&o.id)),
            ("marking", Json::s(band(o.marking.level))),
        ];
        if !o.marking.compartments.is_empty() {
            fields.push((
                "compartments",
                Json::Arr(
                    o.marking
                        .compartments
                        .iter()
                        .map(|c| Json::s(&c.0))
                        .collect(),
                ),
            ));
        }
        fields.push((
            "purposes",
            Json::Arr(o.allowed_purposes.iter().map(|p| Json::s(&p.0)).collect()),
        ));
        fields.push(("properties", Json::Obj(properties)));
        out.push_str(&Json::obj(fields).encode());
        out.push('\n');
    }
    for l in &collected.links {
        let line = Json::obj(vec![
            ("link", Json::s(&l.link_type)),
            ("from", Json::s(&l.source_id)),
            ("to", Json::s(&l.target_id)),
        ]);
        out.push_str(&line.encode());
        out.push('\n');
    }
    out
}

/// What one run of a job produced.
pub struct Run {
    /// The committed dataset version's content id, as hex.
    pub version: String,
    /// Whether the version differs from the branch's previous head.
    pub changed: bool,
    /// Rows read from the source.
    pub rows: usize,
    /// Objects projected.
    pub objects: usize,
    /// Links projected.
    pub links: usize,
    /// The records to sync.
    pub records: String,
    /// Where the next run resumes; `None` for a source read in full every time.
    pub checkpoint: Option<String>,
}

/// Run `job` against the worker's dataset store: sync the source from `checkpoint`
/// with `credentials`, commit what it read with lineage, and project it.
pub fn run_job<R: TrustRoot>(
    job: &Job,
    store: &mut DurableDatasetStore<R>,
    credentials: &dyn CredentialResolver,
    checkpoint: Option<&str>,
) -> Result<Run, String> {
    let connector = job.connector();
    let sandbox = connector
        .egress_hosts()
        .into_iter()
        .fold(SandboxPolicy::air_gapped(), |policy, host| {
            policy.permit_host(host)
        });
    let mut ctx = SyncContext::new(credentials, &sandbox);
    if let Some(checkpoint) = checkpoint {
        ctx = ctx.with_checkpoint(checkpoint);
    }
    let outcome = connector
        .sync_incremental(&ctx)
        .map_err(|e| format!("the source does not sync: {e}"))?;
    let dataset = outcome.dataset;

    let previous = store.branch_head(&job.id);
    let version = store
        .commit(
            &dataset,
            &job.id,
            Lineage::ingest(connector.descriptor().lineage_source()),
            &mut OsRng,
        )
        .map_err(|e| format!("the dataset does not commit: {e:?}"))?;

    let mut collected = Collector::default();
    project_objects(&dataset, &job.objects, &mut collected).map_err(|e| {
        format!(
            "the rows do not project onto {}: {e:?}",
            job.objects.type_id
        )
    })?;
    for link in &job.links {
        project_links(&dataset, link, &mut collected)
            .map_err(|e| format!("the rows do not project onto {}: {e:?}", link.link_type))?;
    }
    Ok(Run {
        version: version.content_id.to_hex(),
        changed: previous != Some(version.content_id),
        rows: dataset.rows().len(),
        objects: collected.objects.len(),
        links: collected.links.len(),
        records: records(&collected),
        checkpoint: outcome.checkpoint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const JOBS: &str = r#"{
      "version": 1,
      "jobs": [{
        "id": "aircraft", "every": 60,
        "source": { "file": "aircraft.csv",
                    "columns": [ { "name": "tail", "type": "text" },
                                 { "name": "hours", "type": "int", "nullable": true },
                                 { "name": "base", "type": "text" } ] },
        "objects": { "type": "aircraft", "key": "tail", "marking": "internal",
                     "compartments": ["OPS-N"], "purposes": ["operations"],
                     "properties": { "tail": "tail", "flight_hours": "hours" } },
        "links": [ { "type": "aircraft_base", "from": "tail", "to": "base" } ]
      }]
    }"#;

    fn with_source(source: &str) -> String {
        JOBS.replacen(r#""file": "aircraft.csv","#, source, 1)
    }

    fn scratch(tag: &str) -> PathBuf {
        let uniq = Box::into_raw(Box::new(0u8)) as usize;
        let dir = std::env::temp_dir().join(format!(
            "forge-worker-lib-{tag}-{}-{uniq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_jobs_file_is_read_strictly() {
        let jobs = parse_jobs(JOBS, Path::new("/data")).unwrap();
        assert_eq!(jobs.len(), 1);
        let job = &jobs[0];
        assert_eq!(job.id, "aircraft");
        assert_eq!(job.every, 60);
        match &job.source {
            Source::File {
                path,
                delimiter,
                header,
            } => {
                assert_eq!(path, &Path::new("/data").join("aircraft.csv"));
                assert_eq!((*delimiter, *header), (',', true));
            }
            other => panic!("not a file source: {other:?}"),
        }
        assert_eq!(job.schema.width(), 3);
        assert_eq!(job.objects.type_id, "aircraft");
        assert_eq!(job.objects.properties.len(), 2);
        assert_eq!(band(job.objects.marking.level), "internal");
        assert_eq!(job.links.len(), 1);

        for (from, to) in [
            (r#""every": 60"#, r#""every": 0"#),
            (r#""version": 1"#, r#""version": 2"#),
            (r#""marking": "internal""#, r#""marking": "secret""#),
            (r#""key": "tail""#, r#""key": "registration""#),
            (r#""flight_hours": "hours""#, r#""flight_hours": "minutes""#),
            (r#""to": "base""#, r#""to": "hangar""#),
            (r#""type": "int""#, r#""type": "bytes""#),
            (r#""purposes": ["operations"]"#, r#""purposes": []"#),
            (r#""id": "aircraft""#, r#""id": "air craft""#),
            (r#""every": 60,"#, r#""every": 60, "cron": "* * *","#),
        ] {
            let bad = JOBS.replacen(from, to, 1);
            assert_ne!(bad, JOBS, "{from}");
            assert!(parse_jobs(&bad, Path::new("/data")).is_err(), "{to}");
        }
        let twice = JOBS.replacen("}]", "}, {\"id\": \"aircraft\"}]", 1);
        assert!(parse_jobs(&twice, Path::new("/data")).is_err());
    }

    #[test]
    fn network_sources_are_read_strictly() {
        let pg = with_source(
            r#""postgres": { "host": "db.internal", "port": 5432, "database": "fleet",
                "user": "reader", "password": "pg-reader",
                "query": "SELECT tail, hours, base FROM aircraft WHERE hours > $1",
                "watermark": { "column": "hours", "start": "0" }, "batchRows": 500 },"#,
        );
        let job = &parse_jobs(&pg, Path::new("/data")).unwrap()[0];
        match &job.source {
            Source::Postgres {
                port,
                password,
                watermark,
                batch_rows,
                ..
            } => {
                assert_eq!(*port, 5432);
                assert_eq!(password, "pg-reader");
                assert_eq!(watermark.as_ref().unwrap().0, "hours");
                assert_eq!(*batch_rows, 500);
            }
            other => panic!("not postgres: {other:?}"),
        }
        assert_eq!(job.connector().egress_hosts(), ["db.internal:5432"]);

        let s3 = with_source(
            r#""s3": { "host": "minio", "port": 9000, "region": "us-east-1",
                "bucket": "exports", "prefix": "aircraft/", "accessKey": "forge",
                "secretKey": "s3-secret" }, "delimiter": ";","#,
        );
        let job = &parse_jobs(&s3, Path::new("/data")).unwrap()[0];
        assert!(matches!(&job.source, Source::S3 { delimiter: ';', .. }));

        let kafka = with_source(
            r#""kafka": { "host": "broker", "port": 9092, "topic": "positions",
                "user": "forge", "password": "kafka-pass", "startFrom": "latest" },"#,
        );
        let job = &parse_jobs(&kafka, Path::new("/data")).unwrap()[0];
        assert!(matches!(
            &job.source,
            Source::Kafka {
                start: StartFrom::Latest,
                sasl: Some(_),
                ..
            }
        ));

        for bad in [
            // Two sources, or none.
            JOBS.replacen(
                r#""file": "aircraft.csv","#,
                r#""file": "a.csv", "kafka": {},"#,
                1,
            ),
            JOBS.replacen(r#""file": "aircraft.csv","#, "", 1),
            pg.replacen("5432", "70000", 1),
            pg.replacen(r#""pg-reader""#, r#""../etc/shadow""#, 1),
            pg.replacen(r#""column": "hours""#, r#""column": "minutes""#, 1),
            pg.replacen(r#""batchRows": 500"#, r#""batchRows": 0"#, 1),
            pg.replacen(r#""host""#, r#""hostname""#, 1),
            s3.replacen(r#""region": "us-east-1","#, "", 1),
            kafka.replacen(r#""password": "kafka-pass","#, "", 1),
            kafka.replacen(r#""latest""#, r#""newest""#, 1),
        ] {
            assert!(parse_jobs(&bad, Path::new("/data")).is_err(), "{bad}");
        }
    }

    #[test]
    fn secrets_come_only_from_their_directory() {
        let dir = scratch("secrets");
        std::fs::write(dir.join("pg-reader"), "correct horse\n").unwrap();
        std::fs::write(dir.join("windows.txt"), "line\r\n").unwrap();
        let secrets = SecretsDir(dir.join(""));
        let read = |name: &str| secrets.resolve(&CredentialRef::new(name));
        assert_eq!(read("pg-reader").unwrap().expose(), b"correct horse");
        assert_eq!(read("windows.txt").unwrap().expose(), b"line");
        for bad in ["missing", "../pg-reader", "", ".hidden", "a/b"] {
            assert!(read(bad).is_err(), "{bad}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoints_survive_a_reopen() {
        let dir = scratch("checkpoints");
        let path = dir.join("checkpoints.journal");
        let sealer = || -> Box<dyn Sealer> {
            let provider = pqc_agility::backend::fips::provider();
            let root = kms::SoftwareTrustRoot::generate(&provider, "forge", &mut FixedRng).unwrap();
            Box::new(kms::KmsSealer::new(kms::Kms::new(provider, root)))
        };
        {
            let mut checkpoints = Checkpoints::open(&path, sealer()).unwrap();
            assert_eq!(checkpoints.get("aircraft"), None);
            checkpoints.record("aircraft", "0=4,1=2").unwrap();
            checkpoints.record("aircraft", "0=5,1=2").unwrap();
            checkpoints.record("aircraft", "0=5,1=2").unwrap();
            checkpoints
                .record("facilities", "2026-09-01T00:00:00.000Z a.csv")
                .unwrap();
        }
        let checkpoints = Checkpoints::open(&path, sealer()).unwrap();
        assert_eq!(checkpoints.get("aircraft"), Some("0=5,1=2"));
        assert_eq!(
            checkpoints.get("facilities"),
            Some("2026-09-01T00:00:00.000Z a.csv")
        );
        assert_eq!(
            checkpoints.journal.len(),
            3,
            "an unchanged checkpoint is not rewritten"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A fixed RNG, so the test's trust root is the same on both opens.
    struct FixedRng;
    impl pqc_agility::Rng for FixedRng {
        fn fill(&mut self, buf: &mut [u8]) {
            for (i, b) in buf.iter_mut().enumerate() {
                *b = (i as u8).wrapping_mul(31).wrapping_add(7);
            }
        }
    }

    #[test]
    fn a_non_finite_number_is_left_out_of_the_record() {
        let jobs = parse_jobs(JOBS, Path::new("/data")).unwrap();
        let mapping = &jobs[0].objects;
        let collected = Collector {
            objects: vec![ObjectInstance {
                id: "AC-1".into(),
                type_id: "aircraft".into(),
                properties: [
                    ("tail".to_string(), Value::Text("AC-1".into())),
                    ("ratio".to_string(), Value::Float(f64::NAN)),
                    ("rate".to_string(), Value::Float(f64::INFINITY)),
                ]
                .into_iter()
                .collect(),
                marking: mapping.marking.clone(),
                allowed_purposes: mapping.allowed_purposes.clone(),
            }],
            links: vec![],
        };
        let out = records(&collected);
        assert!(out.contains("\"properties\":{\"tail\":\"AC-1\"}"), "{out}");
        assert!(!out.contains("null"), "{out}");
    }

    #[test]
    fn collected_instances_become_import_records() {
        let jobs = parse_jobs(JOBS, Path::new("/data")).unwrap();
        let mapping = &jobs[0].objects;
        let collected = Collector {
            objects: vec![ObjectInstance {
                id: "AC-1".into(),
                type_id: "aircraft".into(),
                properties: [
                    ("tail".to_string(), Value::Text("AC-1".into())),
                    ("flight_hours".to_string(), Value::Int(6412)),
                ]
                .into_iter()
                .collect(),
                marking: mapping.marking.clone(),
                allowed_purposes: mapping.allowed_purposes.clone(),
            }],
            links: vec![LinkInstance {
                link_type: "aircraft_base".into(),
                source_id: "AC-1".into(),
                target_id: "FAC-N".into(),
            }],
        };
        assert_eq!(
            records(&collected),
            "{\"object\":\"aircraft\",\"key\":\"AC-1\",\"marking\":\"internal\",\
             \"compartments\":[\"OPS-N\"],\"purposes\":[\"operations\"],\
             \"properties\":{\"flight_hours\":6412,\"tail\":\"AC-1\"}}\n\
             {\"link\":\"aircraft_base\",\"from\":\"AC-1\",\"to\":\"FAC-N\"}\n"
        );
    }
}
