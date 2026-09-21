//! The inference gateway handler: it plugs into the proven PQ-mTLS termination
//! ([`gateway::Gateway`]) and makes inference reachable **only** over that
//! channel, writing one PQ-signed audit entry for **every** call through the
//! governance spine. [FR-MI-8] [NFR-SEC-1] [AC-MI-8]
//!
//! The gateway has already mutually authenticated the peer and authorised the
//! channel before [`InferenceHandler::handle`] runs; this layer adds the
//! per-call authorisation-and-audit envelope and the actual serving.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

use audit_writer::{InMemoryLedger, LedgerStore};
use gateway::RequestHandler;
use governance::{GovContext, Governance};
use policy::{ClassificationLevel, Clearance, Purpose, Resource};
use pq_mtls::WorkloadIdentity;

use crate::api::{InferenceRequest, InferenceResponse};
use crate::serve::Server;

/// A monotonic source of audit timestamps. Injected so tests are deterministic
/// and so an operator can bind a real clock. (Mirrors the IdP's injected clock.)
pub trait Clock: Send + Sync {
    /// The current logical/wall time as a `u64` (seconds or a logical counter).
    fn now(&self) -> u64;
}

/// A deterministic clock that advances by one on each read — for tests and
/// air-gapped, reproducible runs.
#[derive(Debug, Default)]
pub struct LogicalClock(AtomicU64);

impl LogicalClock {
    /// Start at `start`.
    pub fn starting_at(start: u64) -> Self {
        Self(AtomicU64::new(start))
    }
}

impl Clock for LogicalClock {
    fn now(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
}

/// The audited inference handler, over any audit ledger — in memory by default, or a
/// durable `FileLedger` for a running service. [RC-09]
pub struct InferenceHandler<S: LedgerStore = InMemoryLedger> {
    server: Arc<Server>,
    governance: Mutex<Governance<S>>,
    resource: Resource,
    clock: Arc<dyn Clock>,
}

impl<S: LedgerStore> InferenceHandler<S> {
    /// Assemble the handler.
    ///
    /// - `server`: the serving core (already bound to an autonomously-selected
    ///   engine).
    /// - `governance`: the enforcement + PQ-signed audit spine.
    /// - `resource`: the policy resource inference is authorised against.
    /// - `clock`: the audit timestamp source.
    pub fn new(
        server: Arc<Server>,
        governance: Governance<S>,
        resource: Resource,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            server,
            governance: Mutex::new(governance),
            resource,
            clock,
        }
    }

    /// Number of audit entries written so far (test/inspection).
    pub fn audit_len(&self) -> usize {
        self.governance
            .lock()
            .expect("gov poisoned")
            .audit_entries()
            .len()
    }

    /// Build the per-call governance context from the authenticated peer and the
    /// request's declared purpose. Clearance/posture binding is refined when the
    /// full policy context flows through the assembly pipeline (Prompt 2.c); for
    /// serving, the inference resource is UNCLASSIFIED and the audit records the
    /// authenticated subject, action, resource, and purpose.
    fn context(peer: &WorkloadIdentity, purpose: &str) -> GovContext {
        GovContext {
            subject: peer.id.0.clone(),
            roles: peer.roles.clone(),
            clearance: Clearance {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: BTreeSet::new(),
            },
            purpose: Purpose(purpose.to_string()),
        }
    }
}

impl<S: LedgerStore + Send> RequestHandler for InferenceHandler<S> {
    fn handle(&self, peer: &WorkloadIdentity, request: &[u8]) -> Vec<u8> {
        let now = self.clock.now();

        // Decode first so a declared purpose can flow into the audit; a malformed
        // request is still an authenticated call and is still audited.
        let decoded = InferenceRequest::decode(request);
        let (purpose, action) = match &decoded {
            Ok(req) => (req.purpose.clone(), "Infer"),
            Err(_) => ("inference".to_string(), "Infer:MALFORMED"),
        };
        let ctx = Self::context(peer, &purpose);

        // Decide AND audit — one PQ-signed lineage entry per call, permit or deny.
        let decision = {
            let mut gov = self.governance.lock().expect("gov poisoned");
            gov.authorize(&ctx, &self.resource, action, now)
        };
        let decision = match decision {
            Ok(d) => d,
            // Audit failure is a fail-closed refusal; never serve unaudited.
            Err(_) => {
                return InferenceResponse::Denied {
                    reason: "AUDIT_FAILURE".to_string(),
                    audit_seq: 0,
                }
                .encode()
            }
        };

        let req = match decoded {
            Ok(req) => req,
            Err(_) => {
                return InferenceResponse::Denied {
                    reason: "MALFORMED_REQUEST".to_string(),
                    audit_seq: decision.audit_seq,
                }
                .encode()
            }
        };

        if !decision.permitted {
            return InferenceResponse::Denied {
                reason: "POLICY_DENIED".to_string(),
                audit_seq: decision.audit_seq,
            }
            .encode();
        }

        // Serve locally, then stamp the audit evidence onto the completion.
        let prepared = self.server.prepare(&req);
        match self.server.infer(prepared) {
            Ok(mut c) => {
                c.audit_seq = decision.audit_seq;
                InferenceResponse::Ok(c).encode()
            }
            Err(e) => InferenceResponse::Denied {
                reason: format!("INFER_ERROR:{e}"),
                audit_seq: decision.audit_seq,
            }
            .encode(),
        }
    }
}
