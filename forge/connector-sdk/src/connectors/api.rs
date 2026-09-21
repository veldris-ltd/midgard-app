//! A reference **API** connector: a source that must reach a network host. It
//! enforces the [`SandboxPolicy`] egress check **before any fetch** — under the
//! air-gapped default (or any policy that does not permit the host) the sync is
//! refused with [`ConnectorError::SandboxDenied`] and nothing is fetched. When a
//! deployment explicitly permits the host, the actual transfer runs through an
//! injected [`Fetcher`] (production supplies a real PQ-mTLS/HTTP client; tests supply
//! a fixture), so the egress gate is the single, always-enforced choke point.
//! [FR-F-2] [NFR-PRV-1]

use forge_dataset_store::{Dataset, Schema};

use crate::connector::{Connector, ConnectorDescriptor, ConnectorKind, SyncContext};
use crate::connectors::file::DelimitedConnector;
use crate::error::ConnectorError;

/// Performs the actual byte transfer once egress is permitted. Abstracted so the
/// connector never embeds an ambient network client — production plugs in a real
/// HTTP/PQ-mTLS client, tests inject a fixture.
pub trait Fetcher: Send + Sync {
    /// Fetch the resource at `url`, returning its raw body.
    fn get(&self, url: &str) -> Result<String, ConnectorError>;
}

/// A connector that pulls a delimited payload from an HTTP/RPC endpoint.
pub struct ApiConnector {
    host: String,
    url: String,
    schema: Schema,
    delimiter: char,
    has_header: bool,
    fetcher: Box<dyn Fetcher>,
}

impl ApiConnector {
    /// Bind an API source. `host` is what the sandbox authorises; `url` is the full
    /// resource; `fetcher` performs the transfer once egress is allowed.
    pub fn new(
        host: impl Into<String>,
        url: impl Into<String>,
        schema: Schema,
        delimiter: char,
        has_header: bool,
        fetcher: Box<dyn Fetcher>,
    ) -> Self {
        Self {
            host: host.into(),
            url: url.into(),
            schema,
            delimiter,
            has_header,
            fetcher,
        }
    }
}

impl Connector for ApiConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: ConnectorKind::Api,
            source: self.host.clone(),
            streaming: false,
        }
    }

    /// This connector reaches one network host — declared so the ingest funnel
    /// enforces the sandbox against it too, in addition to the inline check below
    /// (which also guards a direct `sync` call). [RT-55]
    fn egress_hosts(&self) -> Vec<String> {
        vec![self.host.clone()]
    }

    fn sync(&self, ctx: &SyncContext) -> Result<Dataset, ConnectorError> {
        // The choke point: no bytes move until the sandbox permits this host. Under
        // the air-gapped default this always fails closed. [NFR-PRV-1]
        ctx.sandbox.check_egress(&self.host)?;
        let body = self.fetcher.get(&self.url)?;
        DelimitedConnector::parse_body(&self.schema, &body, self.delimiter, self.has_header)
    }
}
