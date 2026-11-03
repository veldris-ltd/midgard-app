//! # service-runtime — what every Midgard mesh service runs on
//!
//! A mesh service is a process that terminates PQ-mTLS, authorises and audits what it
//! serves, and can be probed and scraped by its orchestrator. This crate holds the
//! parts they share, so each service binary is only its own wiring:
//!
//! - [`ServiceIdentity`] loads the identity `mg-admin mesh issue` wrote — certificate,
//!   trust anchor, identity key — and opens the service's own durable audit ledger,
//!   refusing anything that does not verify.
//! - [`serve_mesh`] is the accept loop, on the bounded listener every listener shares
//!   ([`serve_bounded`]), with the [`MeshLimits`] every service applies;
//!   [`spawn_health_port`] serves `/healthz`, `/readyz` and `/metrics` on a separate
//!   plain-HTTP port. [`client::call`] is the other end: one routed request to a
//!   service, as a certified workload.
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
pub mod sync;

pub use args::Args;
pub use health_http::spawn_health_port;
pub use identity::{checkpoint_service_ledger, ServiceIdentity};
pub use lock::DirLock;
pub use serve::{
    apply_mesh_limits, mesh_limits_from_args, mesh_limits_from_args_over, serve_bounded,
    serve_mesh, serve_mesh_with, BoundedListener, ConnectionGate, ListenerLimits, MeshLimits,
    CONNECTION_STACK,
};

/// Harden the process before anything else runs: lock all of its memory into RAM
/// (`mlockall`), so key material never reaches swap and dropping one secret cannot
/// unlock a page another shares. Every mesh service calls this first thing in `main`.
///
/// Best-effort: if the lock is refused the service keeps running with per-secret locks
/// and logs one line naming the fix. `MIDGARD_NO_MLOCKALL=1` skips the lock (logged), as
/// an escape hatch for hosts whose memory limits cannot absorb a fully resident process.
/// [VLD-ANB-MIDGARD-0036] [NFR-PQ-2]
pub fn harden_process(service: &str) {
    if std::env::var_os("MIDGARD_NO_MLOCKALL").is_some_and(|v| v == "1") {
        eprintln!(
            "{service}: MIDGARD_NO_MLOCKALL=1, process memory is not locked; key material \
             is locked per secret only"
        );
        return;
    }
    if let Err(why) = pqc_agility::lock_process_memory() {
        eprintln!(
            "{service}: could not lock process memory ({why}); continuing with per-secret \
             locks. Set RLIMIT_MEMLOCK to unlimited or grant CAP_IPC_LOCK to keep key \
             material out of swap."
        );
    }
}

/// Seconds since the Unix epoch by the system clock — what services stamp audit
/// entries and records with.
pub fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
