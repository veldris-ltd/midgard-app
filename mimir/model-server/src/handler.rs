//! The inference gateway handler: it plugs into the proven PQ-mTLS termination
//! ([`gateway::Gateway`]) and makes inference reachable **only** over that
//! channel, writing one PQ-signed audit entry for **every** call through the
//! governance spine. [FR-MI-8] [NFR-SEC-1] [AC-MI-8]
//!
//! The gateway has already mutually authenticated the peer and authorised the
//! channel before [`InferenceHandler::handle`] runs; this layer adds the
//! per-call authorisation-and-audit envelope and the actual serving.

use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

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
    /// How the most recent calls were served, keyed by their audit entry; held in
    /// memory only (the ledger keeps exactly one signed entry per call). [AC-MI-8]
    recent: Mutex<VecDeque<ServeStat>>,
}

/// How many recent calls [`InferenceHandler::recent_calls`] keeps.
pub const RECENT_CALLS: usize = 500;

/// How one call was served: the operator's view of a session, joined to its audit
/// entry by `audit_seq`. [FR-MI-8]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeStat {
    pub audit_seq: u64,
    pub at: u64,
    pub caller: String,
    pub purpose: String,
    pub workload: String,
    /// The calling product's label for the call, if it gave one.
    pub context: Option<String>,
    /// `served`, `denied` or `failed`.
    pub outcome: String,
    pub model: Option<String>,
    pub prompt_tokens: u32,
    pub tokens: u32,
    pub first_token_ms: u32,
    pub total_ms: u64,
    pub detail: Option<String>,
}

/// One sealed ledger entry, as an observer sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditView {
    pub seq: u64,
    pub actor: String,
    pub action: String,
    pub resource: String,
    pub ts: String,
    /// Hex of the entry hash the PQ signature covers.
    pub entry_hash: String,
    pub sig_alg: String,
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
            recent: Mutex::new(VecDeque::new()),
        }
    }

    /// The most recent calls (newest last), up to [`RECENT_CALLS`].
    pub fn recent_calls(&self) -> Vec<ServeStat> {
        self.recent
            .lock()
            .expect("recent poisoned")
            .iter()
            .cloned()
            .collect()
    }

    /// The last `n` sealed ledger entries (oldest first).
    pub fn recent_audit(&self, n: usize) -> Vec<AuditView> {
        let gov = self.governance.lock().expect("gov poisoned");
        let entries = gov.audit_entries();
        entries[entries.len().saturating_sub(n)..]
            .iter()
            .map(|e| AuditView {
                seq: e.seq,
                actor: e.record.actor.0.clone(),
                action: e.record.action.0.clone(),
                resource: e.record.resource.0.clone(),
                ts: e.record.ts.0.clone(),
                entry_hash: e.entry_hash.iter().fold(String::new(), |mut s, b| {
                    use std::fmt::Write;
                    let _ = write!(s, "{b:02x}");
                    s
                }),
                sig_alg: format!("{:?}", e.sig_alg),
            })
            .collect()
    }

    /// Authorise **and audit** an observer read (`action`, e.g. `ReadSessions`) by
    /// `peer`: true when policy permits it. An audit failure is a refusal. [NFR-SEC-1]
    pub fn authorize_observer(&self, peer: &WorkloadIdentity, purpose: &str, action: &str) -> bool {
        let ctx = Self::context(peer, purpose);
        let now = self.clock.now();
        self.governance
            .lock()
            .expect("gov poisoned")
            .authorize(&ctx, &self.resource, action, now)
            .is_ok_and(|d| d.permitted)
    }

    fn remember(&self, stat: ServeStat) {
        let mut recent = self.recent.lock().expect("recent poisoned");
        recent.push_back(stat);
        while recent.len() > RECENT_CALLS {
            recent.pop_front();
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

        let stat = |outcome: &str| ServeStat {
            audit_seq: decision.audit_seq,
            at: now,
            caller: ctx.subject.clone(),
            purpose: req.purpose.clone(),
            workload: format!("{:?}", req.workload_class),
            context: req.context.clone(),
            outcome: outcome.to_string(),
            model: None,
            prompt_tokens: 0,
            tokens: 0,
            first_token_ms: 0,
            total_ms: 0,
            detail: None,
        };
        if !decision.permitted {
            self.remember(stat("denied"));
            return InferenceResponse::Denied {
                reason: "POLICY_DENIED".to_string(),
                audit_seq: decision.audit_seq,
            }
            .encode();
        }

        // Serve locally, then stamp the audit evidence onto the completion.
        let started = Instant::now();
        let prepared = self.server.prepare(&req);
        let result = self.server.infer(prepared);
        let total_ms = started.elapsed().as_millis() as u64;
        match result {
            Ok(mut c) => {
                c.audit_seq = decision.audit_seq;
                self.remember(ServeStat {
                    model: Some(c.model_id.clone()),
                    prompt_tokens: c.prompt_tokens,
                    tokens: c.completion_tokens,
                    first_token_ms: c.first_token_ms,
                    total_ms,
                    ..stat("served")
                });
                InferenceResponse::Ok(c).encode()
            }
            Err(e) => {
                self.remember(ServeStat {
                    total_ms,
                    detail: Some(e.to_string().chars().take(240).collect()),
                    ..stat("failed")
                });
                InferenceResponse::Denied {
                    reason: format!("INFER_ERROR:{e}"),
                    audit_seq: decision.audit_seq,
                }
                .encode()
            }
        }
    }
}
