//! Admission control for inference: a permitted call's `max_tokens` is clamped to the
//! ceiling before the engine sees it, a prompt over the byte bound is refused, and calls
//! in flight are bounded per identity and globally. Every refusal is a `Denied` with a
//! reason code and an audit entry of its own, after the call's decision entry.
//! [VLD-ANB-MIDGARD-0016] [AC-62.1] [AC-62.2] [AC-62.3] [AC-MI-8]
//!
//! While the handler's `ServingState` is stopped, a permitted call is refused with the
//! stop's code on the same path, audited and counted, while calls already admitted
//! finish and are what the state reports in flight. [VLD-ANB-MIDGARD-0057] [AC-67.1]
//! [AC-67.2] [AC-67.4]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;

use gateway::RequestHandler;
use governance::Governance;
use iam::IDP_SIG_ALG;
use model_server::{
    BackendDescriptor, HwProfile, InferError, InferenceHandler, InferenceLimits, InferenceRequest,
    InferenceResponse, Locality, LogicalClock, ModelBackend, NodeProfile, PreparedRequest, Quant,
    RawCompletion, Server, ServingState, WorkloadClass, MODEL_REVOKED, SERVING_STOPPED,
};
use policy::{
    Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicySet, Purpose, Resource,
    ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{Authorization, Posture, ServiceId, WorkloadIdentity};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::Rng;

struct SeedRng(u64);
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            let b = z.to_le_bytes();
            let take = (buf.len() - i).min(8);
            buf[i..i + take].copy_from_slice(&b[..take]);
            i += take;
        }
    }
}

fn descriptor() -> BackendDescriptor {
    BackendDescriptor {
        id: "test-double".into(),
        locality: Locality::InEnclosure,
        hw: HwProfile::none(),
        quants: vec![Quant::None],
        rank: 50,
        families: vec!["reference".into()],
    }
}

fn completion() -> RawCompletion {
    RawCompletion {
        text: "ok".into(),
        first_token_ms: 1,
        cache_hit: false,
        prompt_tokens: 1,
        completion_tokens: 1,
    }
}

/// Records every request it is asked to serve.
struct Recording {
    descriptor: BackendDescriptor,
    seen: Mutex<Vec<PreparedRequest>>,
}

impl ModelBackend for Recording {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.descriptor
    }
    fn infer_batch(&self, batch: &[PreparedRequest]) -> Result<Vec<RawCompletion>, InferError> {
        self.seen.lock().unwrap().extend(batch.iter().cloned());
        Ok(batch.iter().map(|_| completion()).collect())
    }
}

/// While `block` is set, each call meets the test on `entered` and then waits on
/// `release`: the calls are held in flight until the test lets them go.
struct Blocking {
    descriptor: BackendDescriptor,
    block: AtomicBool,
    entered: Barrier,
    release: Barrier,
}

impl Blocking {
    /// Holds `calls` calls in flight at once.
    fn new(calls: usize) -> Arc<Self> {
        Arc::new(Self {
            descriptor: descriptor(),
            block: AtomicBool::new(true),
            entered: Barrier::new(calls + 1),
            release: Barrier::new(calls + 1),
        })
    }
}

impl ModelBackend for Blocking {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.descriptor
    }
    fn infer_batch(&self, batch: &[PreparedRequest]) -> Result<Vec<RawCompletion>, InferError> {
        if self.block.load(Ordering::SeqCst) {
            self.entered.wait();
            self.release.wait();
        }
        Ok(batch.iter().map(|_| completion()).collect())
    }
}

fn handler(backend: Arc<dyn ModelBackend>, limits: InferenceLimits) -> Arc<InferenceHandler> {
    let server = Server::bootstrap(NodeProfile::cpu_only(4), &[backend], None).unwrap();
    let pdp = DefaultPdp::new(
        PolicySet::new(vec![Rule {
            id: RuleId("infer".into()),
            role: Role("inference-consumer".into()),
            action: Action("Infer".into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        }])
        .unwrap(),
    );
    let resource = Resource {
        id: ResourceId("mimir-inference".into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    };
    Arc::new(
        InferenceHandler::new(
            Arc::new(server),
            Governance::new(pdp, &mut SeedRng(77)).unwrap(),
            resource,
            Arc::new(LogicalClock::starting_at(1000)),
        )
        .with_limits(limits),
    )
}

/// The handler out of its `Arc`, to configure further.
fn unshared(handler: Arc<InferenceHandler>) -> InferenceHandler {
    Arc::try_unwrap(handler).ok().expect("not shared yet")
}

/// A certified consumer named `id`, as the gateway hands it to the handler.
fn consumer(id: &str) -> WorkloadIdentity {
    let (verifying_key, _) = fips_provider()
        .sig_for(IDP_SIG_ALG)
        .unwrap()
        .keypair(&mut SeedRng(7))
        .unwrap();
    WorkloadIdentity {
        id: ServiceId(id.into()),
        roles: [Role("inference-consumer".into())].into_iter().collect(),
        sig_alg: IDP_SIG_ALG,
        verifying_key,
    }
}

fn request(prompt: &str, max_tokens: u32) -> InferenceRequest {
    InferenceRequest {
        purpose: "investigation".into(),
        workload_class: WorkloadClass::Balanced,
        quant_pref: None,
        backend_override: None,
        max_tokens,
        prompt: prompt.into(),
    }
}

fn call(handler: &InferenceHandler, peer: &WorkloadIdentity) -> InferenceResponse {
    InferenceResponse::decode(&handler.handle(peer, &channel(), &request("hello", 16).encode()))
        .unwrap()
}

fn denied(response: &InferenceResponse) -> Option<&str> {
    match response {
        InferenceResponse::Denied { reason, .. } => Some(reason),
        InferenceResponse::Ok(_) => None,
    }
}

/// What the channel authorised a caller with a certificate issued before attributes
/// for: unclassified, binding no purposes. [VLD-ANB-MIDGARD-0063]
fn channel() -> Authorization {
    Authorization {
        clearance: Posture::unclassified(),
        purpose: Purpose("investigation".into()),
        purposes: None,
    }
}

#[test]
fn max_tokens_is_clamped_to_the_ceiling() {
    // [VLD-ANB-MIDGARD-0016] [AC-62.1]
    let backend = Arc::new(Recording {
        descriptor: descriptor(),
        seen: Mutex::new(Vec::new()),
    });
    let limits = InferenceLimits {
        max_tokens: 8,
        ..InferenceLimits::default()
    };
    let h = handler(backend.clone(), limits);
    let peer = consumer("forge-worker");

    let wire = request("hello", u32::MAX).encode();
    let response = InferenceResponse::decode(&h.handle(&peer, &channel(), &wire)).unwrap();
    assert!(matches!(response, InferenceResponse::Ok(_)), "{response:?}");
    let wire = request("hello", 4).encode();
    let response = InferenceResponse::decode(&h.handle(&peer, &channel(), &wire)).unwrap();
    assert!(matches!(response, InferenceResponse::Ok(_)), "{response:?}");

    let seen = backend.seen.lock().unwrap();
    let budgets: Vec<u32> = seen.iter().map(|p| p.max_tokens).collect();
    assert_eq!(
        budgets,
        [8, 4],
        "clamped to the ceiling, a smaller budget kept"
    );
}

#[test]
fn oversized_prompt_is_refused_and_audited() {
    // [VLD-ANB-MIDGARD-0016] [AC-62.2]
    let backend = Arc::new(Recording {
        descriptor: descriptor(),
        seen: Mutex::new(Vec::new()),
    });
    let limits = InferenceLimits {
        max_prompt_bytes: 64,
        ..InferenceLimits::default()
    };
    let h = handler(backend.clone(), limits);
    let peer = consumer("forge-worker");

    let at_bound = request(&"a".repeat(64), 16).encode();
    let response = InferenceResponse::decode(&h.handle(&peer, &channel(), &at_bound)).unwrap();
    assert!(matches!(response, InferenceResponse::Ok(_)), "{response:?}");

    let before = h.audit_len();
    let over = request(&"a".repeat(65), 16).encode();
    let response = InferenceResponse::decode(&h.handle(&peer, &channel(), &over)).unwrap();
    assert_eq!(denied(&response), Some("PROMPT_TOO_LARGE"));
    assert_eq!(h.audit_len(), before + 2, "the decision and the refusal");
    assert_eq!(backend.seen.lock().unwrap().len(), 1, "never served");

    // The default bound is 256 KiB.
    assert_eq!(InferenceLimits::default().max_prompt_bytes, 256 * 1024);
}

#[test]
fn third_concurrent_call_for_one_identity_is_busy() {
    // [VLD-ANB-MIDGARD-0016] [AC-62.3]
    let backend = Blocking::new(2);
    let h = handler(backend.clone(), InferenceLimits::default());
    let peer = consumer("forge-worker");

    let holders: Vec<_> = (0..2)
        .map(|_| {
            let (h, peer) = (h.clone(), peer.clone());
            thread::spawn(move || call(&h, &peer))
        })
        .collect();
    backend.entered.wait(); // both calls are in the engine

    let before = h.audit_len();
    assert_eq!(denied(&call(&h, &peer)), Some("BUSY"));
    assert_eq!(h.audit_len(), before + 2, "the decision and the refusal");
    // Another identity still has room under the global bound of 4.
    backend.block.store(false, Ordering::SeqCst);
    assert_eq!(denied(&call(&h, &consumer("heimdall-feed"))), None);

    backend.release.wait();
    for holder in holders {
        assert_eq!(denied(&holder.join().unwrap()), None);
    }
    assert_eq!(
        denied(&call(&h, &peer)),
        None,
        "served once a slot is released"
    );
}

#[test]
fn global_limit_applies_across_identities() {
    // [VLD-ANB-MIDGARD-0016] [AC-62.3]
    let backend = Blocking::new(2);
    let limits = InferenceLimits {
        max_concurrent: 2,
        ..InferenceLimits::default()
    };
    let h = handler(backend.clone(), limits);

    let holders: Vec<_> = ["forge-worker", "bifrost-fleet"]
        .into_iter()
        .map(|id| {
            let (h, peer) = (h.clone(), consumer(id));
            thread::spawn(move || call(&h, &peer))
        })
        .collect();
    backend.entered.wait();

    let third = consumer("heimdall-feed");
    let before = h.audit_len();
    assert_eq!(denied(&call(&h, &third)), Some("BUSY"));
    assert_eq!(h.audit_len(), before + 2, "the decision and the refusal");

    backend.block.store(false, Ordering::SeqCst);
    backend.release.wait();
    for holder in holders {
        assert_eq!(denied(&holder.join().unwrap()), None);
    }
    assert_eq!(
        denied(&call(&h, &third)),
        None,
        "served once a slot is released"
    );
}

#[test]
fn a_stopped_serving_state_refuses_with_its_code_and_audits_it() {
    // [VLD-ANB-MIDGARD-0057] [AC-67.1] [AC-67.2]
    let backend = Arc::new(Recording {
        descriptor: descriptor(),
        seen: Mutex::new(Vec::new()),
    });
    let state = Arc::new(ServingState::new());
    let refusals = Arc::new(Mutex::new(Vec::new()));
    let counted = refusals.clone();
    let h = unshared(handler(backend.clone(), InferenceLimits::default()))
        .with_serving_state(state.clone())
        .on_refusal(Arc::new(move |reason| {
            counted.lock().unwrap().push(reason.to_string());
        }));
    let peer = consumer("forge-worker");
    assert!(state.is_serving());
    assert_eq!(denied(&call(&h, &peer)), None, "serving by default");

    for code in [MODEL_REVOKED, SERVING_STOPPED] {
        state.stop(code);
        assert!(!state.is_serving());
        assert_eq!(state.reason().as_deref(), Some(code));
        let before = h.audit_len();
        assert_eq!(denied(&call(&h, &peer)), Some(code));
        assert_eq!(h.audit_len(), before + 2, "the decision and the refusal");
    }
    assert_eq!(*refusals.lock().unwrap(), [MODEL_REVOKED, SERVING_STOPPED]);
    assert_eq!(
        backend.seen.lock().unwrap().len(),
        1,
        "never served while stopped"
    );

    state.resume();
    assert_eq!(state.reason(), None);
    assert_eq!(denied(&call(&h, &peer)), None, "served once resumed");
    assert_eq!(backend.seen.lock().unwrap().len(), 2);
}

#[test]
fn a_stop_lets_calls_in_flight_finish_and_counts_them() {
    // [VLD-ANB-MIDGARD-0057] [AC-67.4]
    let backend = Blocking::new(2);
    let state = Arc::new(ServingState::new());
    assert_eq!(state.in_flight(), 0, "nothing attached yet");
    let h = Arc::new(
        unshared(handler(backend.clone(), InferenceLimits::default()))
            .with_serving_state(state.clone()),
    );
    let holders: Vec<_> = ["forge-worker", "bifrost-fleet"]
        .into_iter()
        .map(|id| {
            let (h, peer) = (h.clone(), consumer(id));
            thread::spawn(move || call(&h, &peer))
        })
        .collect();
    backend.entered.wait();
    assert_eq!(state.in_flight(), 2);

    state.stop(SERVING_STOPPED);
    assert_eq!(
        denied(&call(&h, &consumer("heimdall-feed"))),
        Some(SERVING_STOPPED),
        "a new call is refused while the admitted ones run"
    );
    backend.block.store(false, Ordering::SeqCst);
    backend.release.wait();
    for holder in holders {
        assert_eq!(
            denied(&holder.join().unwrap()),
            None,
            "admitted calls finish"
        );
    }
    assert_eq!(state.in_flight(), 0, "drained");
}
