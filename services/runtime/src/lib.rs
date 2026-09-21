//! # service-runtime — what every Midgard mesh service runs on
//!
//! A mesh service is a process that terminates PQ-mTLS, authorises and audits what it
//! serves, and can be probed and scraped by its orchestrator. This crate holds the
//! parts they share, so each service binary is only its own wiring:
//!
//! - [`ServiceIdentity`] loads the identity `mg-admin mesh issue` wrote — certificate,
//!   trust anchor, identity key — and opens the service's own durable audit ledger,
//!   refusing anything that does not verify.
//! - [`serve_mesh`] is the accept loop; [`spawn_health_port`] serves `/healthz`,
//!   `/readyz` and `/metrics` on a separate plain-HTTP port. [`client::call`] is the
//!   other end: one routed request to a service, as a certified workload.
//! - [`Args`] parses options strictly; [`policy_file`] reads the rule set a service
//!   enforces; [`DirLock`] keeps two processes from writing one ledger.
//! - [`json`] and [`strict_json`] are the std-only JSON codec and strict readers the
//!   deployment files are parsed with.
//!
//! `[NFR-SEC-1] [RC-33] [RC-09]`

pub mod args;
pub mod client;
pub mod health_http;
pub mod identity;
pub mod json;
pub mod lock;
pub mod marking;
pub mod policy_file;
pub mod serve;
pub mod strict_json;

pub use args::Args;
pub use health_http::spawn_health_port;
pub use identity::ServiceIdentity;
pub use lock::DirLock;
pub use serve::serve_mesh;

/// Seconds since the Unix epoch by the system clock — what services stamp audit
/// entries and records with.
pub fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
