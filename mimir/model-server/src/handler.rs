//! The inference gateway handler: it plugs into the proven PQ-mTLS termination
//! ([`gateway::Gateway`]) and makes inference reachable **only** over that
//! channel, writing one PQ-signed audit decision entry for **every** call through
//! the governance spine. [FR-MI-8] [NFR-SEC-1] [AC-MI-8]
//!
//! The gateway has already mutually authenticated the peer and authorised the
//! channel before [`InferenceHandler::handle`] runs; this layer adds the
//! per-call authorisation-and-audit envelope and the actual serving.
//!
//! The call is authorised against a resource carrying the request's marking (the
//! marking of the data in the prompt), at the lower of the clearance the caller's
//! certificate grants and the model's accreditation, so the PDP's dominance check
//! refuses a prompt the model is not accredited for, or the caller not cleared to send. A served completion writes a second ledger entry,
//! `Infer:COMPLETE`, naming the decision entry and carrying the SHA-256 of the
//! prompt and of the completion, so what the model was given and produced can be
//! traced. [VLD-ANB-MIDGARD-0034]
//!
//! Every permitted call is also admitted against an [`InferenceLimits`] budget: a
//! prompt over the byte bound (the system text and the body together) is refused
//! `PROMPT_TOO_LARGE`, a call over the global or per-identity concurrency bound is
//! refused `BUSY` (each with an audit entry of its own), and `max_tokens` is clamped
//! to the ceiling before the engine sees it. The admission slot is held until the
//! engine returns, error or timeout included.
//! [VLD-ANB-MIDGARD-0016]
//!
//! A [`ServingState`] disengages serving at run time: while it is stopped, a permitted
//! call is refused with the stop's reason code (`MODEL_REVOKED` or `SERVING_STOPPED`)
//! on the same admission path, audited and counted like the other refusals, while the
//! calls already admitted finish. [VLD-ANB-MIDGARD-0057]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::{Mutex, MutexGuard};

use audit_writer::{InMemoryLedger, LedgerStore};
use gateway::RequestHandler;
use governance::Governance;
use policy::{Clearance, Marking, Purpose, Resource};
use pq_mtls::{Authorization, WorkloadIdentity};
use pqc_agility::hash::{Digest, Sha256};
use service_runtime::mesh_context;
use telemetry::{Logger, StdoutJsonSink};

use crate::api::{Completion, InferenceRequest, InferenceResponse};
use crate::artifact::hex;
use crate::marking::{accreditation, marking_label, unclassified};
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
    /// The longest prompt, in bytes (the serving plane has no tokenizer), counting
    /// the system text and the body together; a longer one is refused
    /// `PROMPT_TOO_LARGE`. [VLD-ANB-MIDGARD-0016]
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
    model_clearance: Clearance,
    clock: Arc<dyn Clock>,
    limits: InferenceLimits,
    admission: Arc<Admission>,
    serving: Arc<ServingState>,
    on_refusal: Option<RefusalHook>,
}

/// The SHA-256 (lowercase hex) of a request's prompt as the ledger records it: the
/// system text, a blank line, then the prompt body, always joined, so a verifier
/// recomputes it from the request alone. [VLD-ANB-MIDGARD-0034]
pub fn prompt_sha256(req: &InferenceRequest) -> String {
    let mut h = Sha256::new();
    h.update(req.system_prompt.as_bytes());
    h.update(b"\n\n");
    h.update(req.prompt.as_bytes());
    hex(&h.finalize())
}

/// The SHA-256 (lowercase hex) of a completion's text as the ledger records it.
/// [VLD-ANB-MIDGARD-0034]
pub fn completion_sha256(text: &str) -> String {
    hex(&pqc_agility::hash::sha256(text.as_bytes()))
}

/// The action text of the `Infer:COMPLETE` entry written after a served completion.
/// The marking label and model id hold no spaces, so every field reads back as one
/// `key=value` token. [VLD-ANB-MIDGARD-0034]
fn completion_action(
    decision_seq: u64,
    marking: &Marking,
    req: &InferenceRequest,
    c: &Completion,
) -> String {
    format!(
        "Infer:COMPLETE decision_seq={decision_seq} marking={} model={} prompt_sha256={} \
         completion_sha256={} prompt_tokens={} completion_tokens={}",
        marking_label(marking),
        c.model_id.replace(char::is_whitespace, "_"),
        prompt_sha256(req),
        completion_sha256(&c.text),
        c.prompt_tokens,
        c.completion_tokens,
    )
}

impl<S: LedgerStore> InferenceHandler<S> {
    /// Assemble the handler.
    ///
    /// - `server`: the serving core (already bound to an autonomously-selected
    ///   engine).
    /// - `governance`: the enforcement + PQ-signed audit spine.
    /// - `resource`: the policy resource inference is authorised against: its id
    ///   and purposes, and its marking as the floor every request is raised to (a
    ///   malformed request is authorised against it as it is).
    /// - `clock`: the audit timestamp source.
    ///
    /// The model is accredited UNCLASSIFIED with no compartments until
    /// [`with_model_clearance`](Self::with_model_clearance) says otherwise, so a
    /// marked prompt is refused (fail closed). [VLD-ANB-MIDGARD-0034]
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
            model_clearance: accreditation(&unclassified()),
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

    /// Accredit the model to `clearance`: the clearance every call is authorised
    /// with, which must dominate the request's marking. [VLD-ANB-MIDGARD-0034]
    pub fn with_model_clearance(mut self, clearance: Clearance) -> Self {
        self.model_clearance = clearance;
        self
    }

    /// The action text of every audit entry so far, in order (test/inspection).
    pub fn audit_actions(&self) -> Vec<String> {
        self.governance()
            .audit_entries()
            .iter()
            .map(|e| e.record.action.0.clone())
            .collect()
    }

    /// The resource a request is authorised against: the configured resource with
    /// its marking raised to the request's (the higher level, and every compartment
    /// of either). [VLD-ANB-MIDGARD-0034]
    fn resource_for(&self, marking: &Marking) -> Resource {
        let floor = &self.resource.marking;
        Resource {
            id: self.resource.id.clone(),
            marking: Marking {
                level: floor.level.max(marking.level),
                compartments: floor
                    .compartments
                    .union(&marking.compartments)
                    .cloned()
                    .collect(),
            },
            allowed_purposes: self.resource.allowed_purposes.clone(),
        }
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

    /// The clearance a call is authorised at: the lower of what the channel authorised
    /// the caller at and the model's accreditation, so a prompt is served only if the
    /// caller may send it and the model may see it. Level the lower of the two,
    /// compartments those both hold. [VLD-ANB-MIDGARD-0063] [VLD-ANB-MIDGARD-0034]
    fn call_clearance(&self, certified: &Clearance) -> Clearance {
        Clearance {
            level: certified.level.min(self.model_clearance.level),
            compartments: certified
                .compartments
                .intersection(&self.model_clearance.compartments)
                .cloned()
                .collect(),
        }
    }
}

impl<S: LedgerStore + Send> RequestHandler for InferenceHandler<S> {
    fn handle(
        &self,
        peer: &WorkloadIdentity,
        authorization: &Authorization,
        request: &[u8],
    ) -> Vec<u8> {
        let now = self.clock.now();

        // Decode first so a declared purpose can flow into the audit; a malformed
        // request is still an authenticated call and is still audited.
        let decoded = InferenceRequest::decode(request);
        let (purpose, action) = match &decoded {
            Ok(req) => (req.purpose.clone(), "Infer"),
            Err(_) => ("inference".to_string(), "Infer:MALFORMED"),
        };
        // The per-call context: the peer's certified roles and the request's declared
        // purpose, at the lower of its certified clearance and the model's
        // accreditation. [VLD-ANB-MIDGARD-0063] [VLD-ANB-MIDGARD-0034]
        let mut ctx = mesh_context(peer, authorization, &Purpose(purpose));
        ctx.clearance = self.call_clearance(&ctx.clearance);
        // The data in the prompt carries its marking into the decision.
        let resource = match &decoded {
            Ok(req) => self.resource_for(&req.marking),
            Err(_) => self.resource.clone(),
        };

        // Decide AND audit — one PQ-signed lineage entry per call, permit or deny.
        let decision = {
            let mut gov = self.governance();
            gov.authorize(&ctx, &resource, action, now)
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
        if req.prompt.len().saturating_add(req.system_prompt.len()) > self.limits.max_prompt_bytes {
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
                // Record what the model was given and produced, linked to the
                // decision; a completion that cannot be recorded is never served.
                // [VLD-ANB-MIDGARD-0034]
                let action = completion_action(decision.audit_seq, &resource.marking, &req, &c);
                let recorded = self.governance().record_event(
                    &ctx.subject,
                    &action,
                    &resource.id.0,
                    self.clock.now(),
                );
                if recorded.is_err() {
                    return InferenceResponse::Denied {
                        reason: "AUDIT_FAILURE".to_string(),
                        audit_seq: decision.audit_seq,
                    }
                    .encode();
                }
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
