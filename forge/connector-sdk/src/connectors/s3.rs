//! An **S3-compatible object store** connector: lists a bucket prefix with
//! ListObjectsV2 and reads each object with GetObject, over path-style HTTP signed with
//! AWS Signature Version 4. [FR-F-2] [NFR-SEC-1] [NFR-PRV-1]
//!
//! - **Egress** is checked against the sandbox for exactly the endpoint's `host:port`
//!   before each request.
//! - **The secret key** is a [`CredentialRef`] resolved at sync time and used only to
//!   derive the signing key; it never leaves the process.
//! - **Objects** are delimited text (RFC 4180), each with the job's schema; a malformed
//!   object fails the whole sync, so no partial dataset is committed.
//! - **Incremental**: objects are read in (last-modified, key) order, and the checkpoint
//!   is the last one read. A later sync reads only objects after it — new objects, and
//!   objects rewritten since.
//!
//! TLS is the egress broker's job (see `deploy/`), so the endpoint is plain HTTP on the
//! broker's loopback port.

use std::time::{SystemTime, UNIX_EPOCH};

use forge_dataset_store::{Dataset, Schema};

use crate::connector::{Connector, ConnectorDescriptor, ConnectorKind, SyncContext, SyncOutcome};
use crate::connectors::file::DelimitedConnector;
use crate::credential::CredentialRef;
use crate::error::ConnectorError;
use crate::sigv4::{self, Credentials, EMPTY_PAYLOAD_SHA256};
use crate::{http, net};

/// Keys listed per ListObjectsV2 page unless configured otherwise.
pub const DEFAULT_PAGE_SIZE: u32 = 1_000;

/// An S3-compatible source: delimited objects under a prefix.
#[derive(Debug, Clone)]
pub struct S3Connector {
    host: String,
    port: u16,
    region: String,
    bucket: String,
    prefix: String,
    access_key: String,
    secret_key: CredentialRef,
    schema: Schema,
    delimiter: char,
    has_header: bool,
    page_size: u32,
}

/// One listed object.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Listed {
    last_modified: String,
    key: String,
}

impl Listed {
    fn checkpoint(&self) -> String {
        format!("{} {}", self.last_modified, self.key)
    }
}

impl S3Connector {
    /// Bind the objects under `prefix` in `bucket` at the endpoint `host:port`, in
    /// `region`, read with `access_key` and the secret `secret_key` resolves to, as CSV
    /// with a header row.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: impl Into<String>,
        port: u16,
        region: impl Into<String>,
        bucket: impl Into<String>,
        prefix: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: CredentialRef,
        schema: Schema,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            region: region.into(),
            bucket: bucket.into(),
            prefix: prefix.into(),
            access_key: access_key.into(),
            secret_key,
            schema,
            delimiter: ',',
            has_header: true,
            page_size: DEFAULT_PAGE_SIZE,
        }
    }

    /// Read objects with `delimiter`, with or without a header row.
    pub fn delimited(mut self, delimiter: char, has_header: bool) -> Self {
        self.delimiter = delimiter;
        self.has_header = has_header;
        self
    }

    /// List `keys` keys per page (1 to 1000).
    pub fn page_size(mut self, keys: u32) -> Self {
        self.page_size = keys.clamp(1, 1_000);
        self
    }

    fn host_header(&self) -> String {
        if self.port == 80 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// A signed GET of `path` with `query`.
    fn get(
        &self,
        ctx: &SyncContext,
        secret: &str,
        path: &str,
        query: &[(String, String)],
    ) -> Result<http::Response, ConnectorError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let (amz_date, _) = sigv4::amz_date(now);
        let mut headers = vec![
            ("host".to_string(), self.host_header()),
            (
                "x-amz-content-sha256".to_string(),
                EMPTY_PAYLOAD_SHA256.to_string(),
            ),
            ("x-amz-date".to_string(), amz_date.clone()),
        ];
        let authorization = sigv4::authorization(
            &sigv4::Request {
                method: "GET",
                path,
                query,
                headers: &headers,
                payload_sha256: EMPTY_PAYLOAD_SHA256,
            },
            &self.region,
            "s3",
            &amz_date,
            Credentials {
                access_key: &self.access_key,
                secret_key: secret,
            },
        );
        headers.push(("authorization".to_string(), authorization));

        let query_string = sigv4::canonical_query(query);
        let target = if query_string.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{query_string}")
        };
        let stream = net::connect(ctx.sandbox, &self.host, self.port)?;
        let response = http::get(stream, &target, &headers)?;
        if response.status == 200 {
            return Ok(response);
        }
        let body = String::from_utf8_lossy(&response.body);
        let code = element(&body, "Code").unwrap_or_default();
        let message = element(&body, "Message").unwrap_or_default();
        let detail = format!(
            "{}:{} {path}: HTTP {} {code}: {message}",
            self.host, self.port, response.status
        );
        Err(match code.as_str() {
            "SignatureDoesNotMatch" | "InvalidAccessKeyId" | "AccessDenied" => {
                ConnectorError::Credential(detail)
            }
            _ => ConnectorError::Io(detail),
        })
    }

    /// Every object under the prefix, in (last-modified, key) order.
    fn list(&self, ctx: &SyncContext, secret: &str) -> Result<Vec<Listed>, ConnectorError> {
        let path = format!("/{}", sigv4::uri_encode(&self.bucket, true));
        let mut objects = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut query = vec![
                ("list-type".to_string(), "2".to_string()),
                ("max-keys".to_string(), self.page_size.to_string()),
                ("prefix".to_string(), self.prefix.clone()),
            ];
            if let Some(token) = &token {
                query.push(("continuation-token".to_string(), token.clone()));
            }
            let response = self.get(ctx, secret, &path, &query)?;
            let body = String::from_utf8(response.body)
                .map_err(|_| ConnectorError::Parse("a listing that is not UTF-8".into()))?;
            for contents in elements(&body, "Contents") {
                let key = element(contents, "Key")
                    .ok_or_else(|| ConnectorError::Parse("a listed object has no key".into()))?;
                let last_modified = element(contents, "LastModified").ok_or_else(|| {
                    ConnectorError::Parse(format!("listed object {key} has no LastModified"))
                })?;
                if !key.ends_with('/') {
                    objects.push(Listed { last_modified, key });
                }
            }
            if element(&body, "IsTruncated").as_deref() == Some("true") {
                token = Some(element(&body, "NextContinuationToken").ok_or_else(|| {
                    ConnectorError::Parse("a truncated listing has no continuation token".into())
                })?);
            } else {
                break;
            }
        }
        objects.sort();
        Ok(objects)
    }
}

impl Connector for S3Connector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: ConnectorKind::ObjectStore,
            source: format!(
                "s3:{}:{}/{}/{}",
                self.host, self.port, self.bucket, self.prefix
            ),
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
        let secret = ctx.credentials.resolve(&self.secret_key)?;
        let secret = std::str::from_utf8(secret.expose())
            .map_err(|_| ConnectorError::Credential("the secret key is not UTF-8".into()))?;

        let after = ctx.checkpoint.map(|c| {
            let (last_modified, key) = c.split_once(' ').unwrap_or((c, ""));
            Listed {
                last_modified: last_modified.to_string(),
                key: key.to_string(),
            }
        });
        let pending: Vec<Listed> = self
            .list(ctx, secret)?
            .into_iter()
            .filter(|o| after.as_ref().map_or(true, |a| o > a))
            .collect();

        let mut rows = Vec::new();
        for object in &pending {
            let path = format!(
                "/{}/{}",
                sigv4::uri_encode(&self.bucket, true),
                sigv4::uri_encode(&object.key, false)
            );
            let response = self.get(ctx, secret, &path, &[])?;
            let body = String::from_utf8(response.body).map_err(|_| {
                ConnectorError::Parse(format!("object {} is not UTF-8 text", object.key))
            })?;
            let dataset = DelimitedConnector::parse_body(
                &self.schema,
                &body,
                self.delimiter,
                self.has_header,
            )
            .map_err(|e| ConnectorError::Parse(format!("object {}: {e}", object.key)))?;
            rows.extend(dataset.rows().iter().cloned());
        }

        let checkpoint = pending
            .last()
            .map(Listed::checkpoint)
            .or_else(|| ctx.checkpoint.map(str::to_string));
        Ok(SyncOutcome {
            dataset: Dataset::new(self.schema.clone(), rows)?,
            checkpoint,
        })
    }
}

/// The text of the first `<tag>` element, unescaped.
fn element(xml: &str, tag: &str) -> Option<String> {
    elements(xml, tag).next().map(unescape)
}

/// The inner text of each `<tag>…</tag>` element, in order.
fn elements<'a>(xml: &'a str, tag: &str) -> impl Iterator<Item = &'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut rest = xml;
    std::iter::from_fn(move || {
        let start = rest.find(&open)? + open.len();
        let end = rest[start..].find(&close)? + start;
        let inner = &rest[start..end];
        rest = &rest[end + close.len()..];
        Some(inner)
    })
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
