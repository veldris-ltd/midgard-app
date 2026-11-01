//! The inference gateway handler: it plugs into the proven PQ-mTLS termination
//! ([`gateway::Gateway`]) and makes inference reachable **only** over that
//! channel, writing one PQ-signed audit entry for **every** call through the
//! governance spine. [FR-MI-8] [NFR-SEC-1] [AC-MI-8]
//!
//! The gateway has already mutually authenticated the peer and authorised the
//! channel before [`InferenceHandler::handle`] runs; this layer adds the
//! per-call authorisation-and-audit envelope and the actual serving.
//!
//! Every permitted call is also admitted against an [`InferenceLimits`] budget: a
//! prompt over the byte bound is refused `PROMPT_TOO_LARGE`, a call over the global or
//! per-identity concurrency bound is refused `BUSY` (each with an audit entry of its
//! own), and `max_tokens` is clamped to the ceiling before the engine sees it. The
//! admission slot is held until the engine returns, error or timeout included.
//! [VLD-ANB-MIDGARD-0016]
//!
//! A [`ServingState`] disengages serving at run time: while it is stopped, a permitted
//! call is refused with the stop's reason code (`MODEL_REVOKED` or `SERVING_STOPPED`)
//! on the same admission path, audited and counted like the other refusals, while the
//! calls already admitted finish. [VLD-ANB-MIDGARD-0057]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::{Mutex, MutexGuard};

use audit_writer::{InMemoryLedger, LedgerStore};
use gateway::RequestHandler;
use governance::{GovContext, Governance};
use policy::{ClassificationLevel, Clearance, Purpose, Resource};
use pq_mtls::WorkloadIdentity;
use telemetry::{Logger, StdoutJsonSink};

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

/// The budget one inference call is admitted against. [VLD-ANB-MIDGARD-0016]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InferenceLimits {
    /// The most completion tokens a call may ask for; a larger `max_tokens` is
    /// clamped to it, not refused.
    pub max_tokens: u32,
    /// The longest prompt, in bytes (the serving plane has no tokenizer); a longer
    /// one is refused `PROMPT_TOO_LARGE`.
    pub max_prompt_bytes: usize,
    /// Calls in flight across every identity; one more is refused `BUSY`.
    pub max_concurrent: usize,
    /// Calls in flight for one certified identity; one more is refused `BUSY`.
    pub max_concurrent_per_identity: usize,
}

impl Default for InferenceLimits {
    /// 1024 completion tokens, a 256 KiB prompt, 4 calls in flight (twice the
    /// sidecar's two slots) and 2 for any one identity.
    fn default() -> Self {
        Self {
            max_tokens: 1024,
            max_prompt_bytes: 256 * 1024,
            max_concurrent: 4,
            max_concurrent_per_identity: 2,
        }
    }
}

impl std::fmt::Display for InferenceLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "max-tokens {}, max-prompt-bytes {}, max-concurrent {}, \
             max-concurrent-per-identity {}",
            self.max_tokens,
            self.max_prompt_bytes,
            self.max_concurrent,
            self.max_concurrent_per_identity
        )
    }
}

/// Calls in flight, globally and per subject id.
#[derive(Default)]
struct InFlight {
    total: usize,
    by_subject: BTreeMap<String, usize>,
}

/// The concurrency bound: a slot is taken before the engine is called and given back
/// when its [`AdmissionGuard`] drops. [VLD-ANB-MIDGARD-0016]
struct Admission {
    max_concurrent: usize,
    max_per_subject: usize,
    in_flight: Mutex<InFlight>,
}

impl Admission {
    fn new(limits: &InferenceLimits) -> Arc<Self> {
        Arc::new(Self {
            max_concurrent: limits.max_concurrent,
            max_per_subject: limits.max_concurrent_per_identity,
            in_flight: Mutex::new(InFlight::default()),
        })
    }

    /// A slot for `subject`, or `None` when the global or the subject's bound is
    /// reached.
    fn try_acquire(self: &Arc<Self>, subject: &str) -> Option<AdmissionGuard> {
        let mut in_flight = self
            .in_flight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let held = in_flight.by_subject.get(subject).copied().unwrap_or(0);
        if in_flight.total >= self.max_concurrent || held >= self.max_per_subject {
            return None;
        }
        in_flight.total += 1;
        in_flight.by_subject.insert(subject.to_string(), held + 1);
        Some(AdmissionGuard {
            admission: self.clone(),
            subject: subject.to_string(),
        })
    }
}

/// One admitted call's slot, given back on drop.
struct AdmissionGuard {
    admission: Arc<Admission>,
    subject: String,
}

impl Drop for AdmissionGuard {
    fn drop(&mut self) {
        let mut in_flight = self
            .admission
            .in_flight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        in_flight.total = in_flight.total.saturating_sub(1);
        if let Some(held) = in_flight.by_subject.get_mut(&self.subject) {
            *held = held.saturating_sub(1);
            if *held == 0 {
                in_flight.by_subject.remove(&self.subject);
            }
        }
    }
}

/// The refusal code while the served model's approval is withdrawn: its id is revoked,
/// or its manifest no longer verifies against the current trust anchor.
/// [VLD-ANB-MIDGARD-0057]
pub const MODEL_REVOKED: &str = "MODEL_REVOKED";

/// The refusal code while an operator has disengaged inference. [VLD-ANB-MIDGARD-0057]
pub const SERVING_STOPPED: &str = "SERVING_STOPPED";

/// Whether the handler serves: a run-time stop, shared between the handler and whatever
/// watches the model's approval and the operator's stop switch. Serving by default.
/// [VLD-ANB-MIDGARD-0057]
pub struct ServingState {
    serving: AtomicBool,
    reason: Mutex<Option<String>>,
    admission: Mutex<Option<Arc<Admission>>>,
}

impl Default for ServingState {
    fn default() -> Self {
        Self {
            serving: AtomicBool::new(true),
            reason: Mutex::new(None),
            admission: Mutex::new(None),
        }
    }
}

impl ServingState {
    /// A state that serves until it is stopped.
    pub fn new() -> Self {
        Self::default()
    }

    /// Refuse new calls with the reason code `reason` ([`MODEL_REVOKED`] or
    /// [`SERVING_STOPPED`]); calls already admitted finish.
    pub fn stop(&self, reason: &str) {
        *self
            .reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(reason.to_string());
        self.serving.store(false, Ordering::SeqCst);
    }

    /// Serve again.
    pub fn resume(&self) {
        self.serving.store(true, Ordering::SeqCst);
        *self
            .reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// Whether new calls are served.
    pub fn is_serving(&self) -> bool {
        self.serving.load(Ordering::SeqCst)
    }

    /// The reason code of the current stop, if stopped.
    pub fn reason(&self) -> Option<String> {
        self.reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Calls admitted and not yet returned by the engine, across every identity: what a
    /// stop waits on to drain. Zero before a handler is attached.
    pub fn in_flight(&self) -> usize {
        let admission = self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        admission.map_or(0, |a| {
            a.in_flight
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .total
        })
    }

    /// Count in-flight calls against `admission`, the handler's.
    fn attach(&self, admission: &Arc<Admission>) {
        *self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(admission.clone());
    }
}

/// Told the reason code of every admission refusal (`BUSY`, `PROMPT_TOO_LARGE`,
/// `MODEL_REVOKED`, `SERVING_STOPPED`), for the service's metrics.
/// [VLD-ANB-MIDGARD-0016] [VLD-ANB-MIDGARD-0057]
pub type RefusalHook = Arc<dyn Fn(&str) + Send + Sync>;

/// The name the inference handler's governance lock is logged and audited under.
const GOVERNANCE_LOCK: &str = "inference-governance";

/// The audited inference handler, over any audit ledger — in memory by default, or a
/// durable `FileLedger` for a running service. [RC-09]
pub struct InferenceHandler<S: LedgerStore = InMemoryLedger> {
    server: Arc<Server>,
    governance: Mutex<Governance<S>>,
    resource: Resource,
    clock: Arc<dyn Clock>,
    limits: InferenceLimits,
    admission: Arc<Admission>,
    serving: Arc<ServingState>,
    on_refusal: Option<RefusalHook>,
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
        let admission = Admission::new(&InferenceLimits::default());
        let serving = Arc::new(ServingState::new());
        serving.attach(&admission);
        Self {
            server,
            governance: Mutex::new(governance),
            resource,
            clock,
            limits: InferenceLimits::default(),
            admission,
            serving,
            on_refusal: None,
        }
    }

    /// Admit calls against `limits` instead of [`InferenceLimits::default`].
    /// [VLD-ANB-MIDGARD-0016]
    pub fn with_limits(mut self, limits: InferenceLimits) -> Self {
        self.admission = Admission::new(&limits);
        self.serving.attach(&self.admission);
        self.limits = limits;
        self
    }

    /// Serve only while `state` does, instead of always; its
    /// [`in_flight`](ServingState::in_flight) counts this handler's admitted calls.
    /// [VLD-ANB-MIDGARD-0057]
    pub fn with_serving_state(mut self, state: Arc<ServingState>) -> Self {
        state.attach(&self.admission);
        self.serving = state;
        self
    }

    /// Call `hook` with the reason code of every admission refusal.
    pub fn on_refusal(mut self, hook: RefusalHook) -> Self {
        self.on_refusal = Some(hook);
        self
    }

    /// The limits calls are admitted against.
    pub fn limits(&self) -> &InferenceLimits {
        &self.limits
    }

    /// Refuse a permitted call with `reason`, writing the refusal as an audit entry of
    /// its own (`Infer:<reason>`); an audit failure is still a refusal.
    /// [VLD-ANB-MIDGARD-0016]
    fn refuse(&self, subject: &str, reason: &str, now: u64) -> Vec<u8> {
        if let Some(hook) = &self.on_refusal {
            hook(reason);
        }
        let recorded = self.governance().record_event(
            subject,
            &format!("Infer:{reason}"),
            &self.resource.id.0,
            now,
        );
        match recorded {
            Ok(audit_seq) => InferenceResponse::Denied {
                reason: reason.to_string(),
                audit_seq,
            },
            Err(_) => InferenceResponse::Denied {
                reason: "AUDIT_FAILURE".to_string(),
                audit_seq: 0,
            },
        }
        .encode()
    }

    /// Refuse a permitted call while serving is stopped, with the stop's reason code.
    /// [VLD-ANB-MIDGARD-0057]
    fn refuse_stopped(&self, subject: &str, now: u64) -> Vec<u8> {
        let reason = self
            .serving
            .reason()
            .unwrap_or_else(|| SERVING_STOPPED.to_string());
        self.refuse(subject, &reason, now)
    }

    /// Number of audit entries written so far (test/inspection).
    pub fn audit_len(&self) -> usize {
        self.governance().audit_entries().len()
    }

    /// The governance spine, recovering from poisoning rather than failing every later
    /// `Infer`: a recovery is logged at error level, recorded in the ledger through the
    /// recovered guard, and the poison cleared so it is reported once.
    /// [VLD-ANB-MIDGARD-0030]
    fn governance(&self) -> MutexGuard<'_, Governance<S>> {
        match self.governance.lock() {
            Ok(governance) => governance,
            Err(poisoned) => {
                let logger = Logger::new("model-server", &StdoutJsonSink);
                logger.error("poisoned lock recovered", &[("lock", GOVERNANCE_LOCK)]);
                let mut governance = poisoned.into_inner();
                self.governance.clear_poison();
                let resource = format!("lock:{GOVERNANCE_LOCK}");
                if let Err(e) = governance.record_event(
                    "system",
                    "RECOVERED poisoned lock",
                    &resource,
                    self.clock.now(),
                ) {
                    logger.error(
                        "poisoned lock recovery not recorded",
                        &[("lock", GOVERNANCE_LOCK), ("reason", &e.to_string())],
                    );
                }
                governance
            }
        }
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
            let mut gov = self.governance();
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

        // Admission: refuse while serving is stopped, bound the prompt, take a
        // concurrency slot (held until the engine returns, a timeout included), and
        // clamp the generation budget. [VLD-ANB-MIDGARD-0016] [VLD-ANB-MIDGARD-0057]
        if !self.serving.is_serving() {
            return self.refuse_stopped(&ctx.subject, now);
        }
        if req.prompt.len() > self.limits.max_prompt_bytes {
            return self.refuse(&ctx.subject, "PROMPT_TOO_LARGE", now);
        }
        let Some(slot) = self.admission.try_acquire(&ctx.subject) else {
            return self.refuse(&ctx.subject, "BUSY", now);
        };
        // A stop that landed while the slot was taken: either this sees it, or the stop's
        // drain counts this call. [VLD-ANB-MIDGARD-0057]
        if !self.serving.is_serving() {
            drop(slot);
            return self.refuse_stopped(&ctx.subject, now);
        }
        let _slot = slot;
        let mut req = req;
        req.max_tokens = req.max_tokens.min(self.limits.max_tokens);

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
