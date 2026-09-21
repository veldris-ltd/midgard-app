//! The typed connector seam: what a source binding is, the sandbox it runs under,
//! and the sync context handed to it. A connector is a *pure function of its
//! declared inputs* — source config + resolved credentials → a typed dataset — so
//! it is testable offline and cannot reach ambient authority. [FR-F-2]

use forge_dataset_store::Dataset;

use crate::credential::CredentialResolver;
use crate::error::ConnectorError;

/// The family a connector belongs to. Reference connectors ship per family behind
/// feature flags; custom connectors declare their own. [FR-F-2]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorKind {
    /// Relational / SQL database.
    Database,
    /// Object store (S3-class).
    ObjectStore,
    /// HTTP / RPC API.
    Api,
    /// Local or mounted file.
    File,
    /// Continuous event stream.
    Stream,
}

impl ConnectorKind {
    /// The contract string used in lineage labels.
    pub fn as_str(self) -> &'static str {
        match self {
            ConnectorKind::Database => "db",
            ConnectorKind::ObjectStore => "object-store",
            ConnectorKind::Api => "api",
            ConnectorKind::File => "file",
            ConnectorKind::Stream => "stream",
        }
    }
}

/// Static metadata a connector advertises about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorDescriptor {
    /// The source family.
    pub kind: ConnectorKind,
    /// A short, non-secret source label, e.g. `sales.csv` or `pg:sales-ro`. Becomes
    /// part of the ingest lineage source.
    pub source: String,
    /// Whether this connector yields a continuous stream (vs. a bounded batch).
    /// [FR-F-5]
    pub streaming: bool,
}

impl ConnectorDescriptor {
    /// The canonical lineage source label, e.g. `connector:file:sales.csv`.
    pub fn lineage_source(&self) -> String {
        format!("connector:{}:{}", self.kind.as_str(), self.source)
    }
}

/// What a connector is allowed to touch while it runs. The default is fully
/// air-gapped — no outbound access — matching the suite's sovereign posture; a
/// deployment must *explicitly* widen it. [NFR-PRV-1]
#[derive(Debug, Clone, Default)]
pub struct SandboxPolicy {
    allow_network: bool,
    allowed_hosts: Vec<String>,
}

impl SandboxPolicy {
    /// The default: deny all outbound access.
    pub fn air_gapped() -> Self {
        Self::default()
    }

    /// Permit outbound access to a specific host (and enable networking).
    pub fn permit_host(mut self, host: impl Into<String>) -> Self {
        self.allow_network = true;
        self.allowed_hosts.push(host.into());
        self
    }

    /// Check whether the connector may reach `host`; returns a [`ConnectorError`]
    /// otherwise. A connector calls this before any egress.
    pub fn check_egress(&self, host: &str) -> Result<(), ConnectorError> {
        if self.allow_network && self.allowed_hosts.iter().any(|h| h == host) {
            Ok(())
        } else {
            Err(ConnectorError::SandboxDenied {
                host: host.to_string(),
            })
        }
    }
}

/// The per-run context handed to a connector: the credential resolver, the sandbox
/// policy, and where the previous sync left off. The connector borrows these; it
/// stores neither secrets nor ambient handles.
pub struct SyncContext<'a> {
    /// Resolves credential references to secret material (KMS-backed in production).
    pub credentials: &'a dyn CredentialResolver,
    /// The egress policy this run enforces.
    pub sandbox: &'a SandboxPolicy,
    /// The checkpoint the previous sync of this source returned, if any — an
    /// incremental connector resumes from it. Opaque to everything but the connector.
    pub checkpoint: Option<&'a str>,
}

impl<'a> SyncContext<'a> {
    /// Build a sync context with no checkpoint: a first, or full, sync.
    pub fn new(credentials: &'a dyn CredentialResolver, sandbox: &'a SandboxPolicy) -> Self {
        Self {
            credentials,
            sandbox,
            checkpoint: None,
        }
    }

    /// Resume from `checkpoint`.
    pub fn with_checkpoint(mut self, checkpoint: &'a str) -> Self {
        self.checkpoint = Some(checkpoint);
        self
    }
}

/// What one sync produced: the dataset, and the checkpoint the next sync resumes from.
/// [FR-F-2]
#[derive(Debug)]
pub struct SyncOutcome {
    /// The rows this sync read.
    pub dataset: Dataset,
    /// Where the next sync resumes; `None` for a connector that always reads everything.
    pub checkpoint: Option<String>,
}

/// A typed source binding. `sync` pulls the source into a validated [`Dataset`];
/// the SDK seals and commits it (see [`crate::sync_to_store`]).
pub trait Connector {
    /// Static metadata about this connector.
    fn descriptor(&self) -> ConnectorDescriptor;

    /// The outbound hosts this connector needs to reach, if any. Local connectors
    /// (file, in-memory) return the default empty set — they make no egress. The
    /// ingest funnel checks every host here against the [`SandboxPolicy`] BEFORE the
    /// connector runs, so the sandbox is enforced uniformly for *every* connector,
    /// not only the ones that remember to call `check_egress` themselves. [RT-55]
    /// [NFR-PRV-1]
    fn egress_hosts(&self) -> Vec<String> {
        Vec::new()
    }

    /// Pull the source into a typed dataset, honouring the sandbox and resolving
    /// any credentials through `ctx`. Must not reach ambient authority.
    fn sync(&self, ctx: &SyncContext) -> Result<Dataset, ConnectorError>;

    /// Pull what is new since `ctx.checkpoint`, and say where the next sync resumes. A
    /// connector that cannot sync incrementally reads everything and keeps the
    /// checkpoint it was given.
    fn sync_incremental(&self, ctx: &SyncContext) -> Result<SyncOutcome, ConnectorError> {
        Ok(SyncOutcome {
            dataset: self.sync(ctx)?,
            checkpoint: ctx.checkpoint.map(str::to_string),
        })
    }
}
