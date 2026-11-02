//! VLD-ANB-MIDGARD-0034: the marking of the data in a prompt reaches the PDP, with
//! the model's accreditation as the clearance, and a served completion leaves the
//! SHA-256 of its prompt and of its text in the signed ledger, linked to the
//! decision. These drive the audited handler directly with an authenticated peer,
//! as the gateway does once PQ-mTLS has admitted it (the channel itself is proven
//! in `inference_pqmtls.rs`). [AC-65.1] [AC-65.2] [AC-65.3] [AC-65.4]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::sync::Arc;

use audit_writer::{
    AuditError, AuditWriter, InMemoryLedger, LedgerCheckpoint, LedgerId, LedgerStore, SealedEntry,
};
use gateway::RequestHandler;
use governance::{Governance, GOV_SIG_ALG};
use model_server::marking::{accreditation, marking_label, parse_marking, unclassified};
use model_server::{
    reference_registry, InferenceHandler, InferenceRequest, InferenceResponse, LogicalClock,
    NodeProfile, Server, WorkloadClass,
};
use policy::{
    Action, ClassificationLevel, Clearance, Compartment, DefaultPdp, Effect, Marking, PolicySet,
    Purpose, Resource, ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{Authorization, Posture, ServiceId, WorkloadIdentity};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::hash::sha256;
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

/// An in-memory ledger that refuses every append after the first: the decision
/// entry is written, the completion entry is not.
#[derive(Default)]
struct FailsOnSecondWrite(InMemoryLedger);

impl LedgerStore for FailsOnSecondWrite {
    fn append(&mut self, entry: SealedEntry) -> Result<(), AuditError> {
        if self.0.entries().is_empty() {
            self.0.append(entry)
        } else {
            Err(AuditError::Store("disk full".into()))
        }
    }
    fn last(&self) -> Option<&SealedEntry> {
        self.0.last()
    }
    fn entries(&self) -> &[SealedEntry] {
        self.0.entries()
    }
    fn record_checkpoint(&mut self, checkpoint: LedgerCheckpoint) -> Result<(), AuditError> {
        self.0.record_checkpoint(checkpoint)
    }
    fn checkpoint(&self) -> Option<&LedgerCheckpoint> {
        self.0.checkpoint()
    }
}

/// Permit `inference-consumer` to `Infer`, so every refusal below is the marking's.
fn pdp() -> DefaultPdp {
    DefaultPdp::new(
        PolicySet::new(vec![Rule {
            id: RuleId("consumer-infer".into()),
            role: Role("inference-consumer".into()),
            action: Action("Infer".into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        }])
        .unwrap(),
    )
}

fn inference_resource() -> Resource {
    Resource {
        id: ResourceId("mimir-inference".into()),
        marking: unclassified(),
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

fn server() -> Arc<Server> {
    Arc::new(Server::bootstrap(NodeProfile::cpu_only(4), &reference_registry(), None).unwrap())
}

fn handler(clearance: Clearance) -> InferenceHandler {
    let governance = Governance::new(pdp(), &mut SeedRng(77)).unwrap();
    InferenceHandler::new(
        server(),
        governance,
        inference_resource(),
        Arc::new(LogicalClock::starting_at(1000)),
    )
    .with_model_clearance(clearance)
}

fn clearance(text: &str) -> Clearance {
    accreditation(&parse_marking(text).unwrap())
}

fn peer() -> WorkloadIdentity {
    let (vk, _sk) = fips_provider()
        .sig_for(GOV_SIG_ALG)
        .unwrap()
        .keypair(&mut SeedRng(3))
        .unwrap();
    WorkloadIdentity {
        id: ServiceId("svc-forge".into()),
        roles: [Role("inference-consumer".into())].into_iter().collect(),
        sig_alg: GOV_SIG_ALG,
        verifying_key: vk,
    }
}

fn request(marking: Marking) -> InferenceRequest {
    InferenceRequest {
        purpose: "investigation".into(),
        system_prompt: "You are Mimir.".into(),
        workload_class: WorkloadClass::Balanced,
        quant_pref: None,
        backend_override: None,
        max_tokens: 16,
        marking,
        prompt: "who is assigned to case 42?".into(),
    }
}

/// What the channel authorised the caller at: cleared above every prompt these tests
/// send, so the model's accreditation is what decides. [VLD-ANB-MIDGARD-0063]
fn channel() -> Authorization {
    channel_at(Posture {
        level: ClassificationLevel::TOP_SECRET,
        compartments: [Compartment("ALPHA".into())].into_iter().collect(),
    })
}

fn channel_at(clearance: Posture) -> Authorization {
    Authorization {
        clearance,
        purpose: Purpose("investigation".into()),
        purposes: None,
    }
}

fn call<S: LedgerStore + Send>(
    h: &InferenceHandler<S>,
    req: &InferenceRequest,
) -> InferenceResponse {
    call_as(h, &channel(), req)
}

fn call_as<S: LedgerStore + Send>(
    h: &InferenceHandler<S>,
    channel: &Authorization,
    req: &InferenceRequest,
) -> InferenceResponse {
    InferenceResponse::decode(&h.handle(&peer(), channel, &req.encode())).expect("decodes")
}

fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write;
    sha256(bytes).iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// The value of `key` in a ledger entry's space-separated `key=value` fields.
fn field<'a>(action: &'a str, key: &str) -> Option<&'a str> {
    action
        .split(' ')
        .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
}

// [VLD-ANB-MIDGARD-0034] [AC-65.1]
#[test]
fn marked_prompt_against_unaccredited_model_is_denied_and_audited() {
    // The level the book calls OFFICIAL-SENSITIVE; the policy lattice names it by
    // ordinal, and anything above UNCLASSIFIED is refused by an unaccredited model.
    let h = handler(accreditation(&unclassified()));
    let marked = Marking {
        level: ClassificationLevel::CONFIDENTIAL,
        compartments: BTreeSet::new(),
    };
    match call(&h, &request(marked)) {
        InferenceResponse::Denied { reason, audit_seq } => {
            assert_eq!(reason, "POLICY_DENIED");
            assert_eq!(audit_seq, 0);
        }
        InferenceResponse::Ok(_) => panic!("a marked prompt reached an unaccredited model"),
    }
    // The refusal is the decision entry, and nothing was served to record.
    assert_eq!(h.audit_actions(), vec!["DENY Infer".to_string()]);

    // The same identity with an unmarked prompt is served: the marking decided.
    assert!(matches!(
        call(&h, &request(unclassified())),
        InferenceResponse::Ok(_)
    ));
}

// [VLD-ANB-MIDGARD-0034] [AC-65.2]
#[test]
fn compartmented_prompt_needs_the_compartment() {
    let marked = parse_marking("CONFIDENTIAL:ALPHA").unwrap();

    let without = handler(clearance("CONFIDENTIAL"));
    assert!(matches!(
        call(&without, &request(marked.clone())),
        InferenceResponse::Denied { ref reason, .. } if reason == "POLICY_DENIED"
    ));
    assert_eq!(without.audit_actions(), vec!["DENY Infer".to_string()]);

    let with = handler(clearance("CONFIDENTIAL:ALPHA"));
    assert!(matches!(
        call(&with, &request(marked)),
        InferenceResponse::Ok(_)
    ));
    assert_eq!(with.audit_actions()[0], "PERMIT Infer");
}

// [VLD-ANB-MIDGARD-0034] [AC-65.3]
#[test]
fn completion_digests_are_recorded() {
    let h = handler(clearance("SECRET:ALPHA"));
    // Spend one decision first, so decision_seq is not trivially zero.
    let _ = call(&h, &request(unclassified()));
    let before = h.audit_len();

    let marking = parse_marking("SECRET:ALPHA").unwrap();
    let req = request(marking.clone());
    let c = match call(&h, &req) {
        InferenceResponse::Ok(c) => c,
        InferenceResponse::Denied { reason, .. } => panic!("unexpected denial: {reason}"),
    };

    // One decision entry and one completion entry for the call.
    let actions = h.audit_actions();
    assert_eq!(actions.len(), before + 2, "{actions:?}");
    assert_eq!(actions[before], "PERMIT Infer");
    let complete = &actions[before + 1];
    assert!(complete.starts_with("Infer:COMPLETE "), "{complete}");

    // Linked to the decision whose seq the response carries.
    assert_eq!(
        field(complete, "decision_seq"),
        Some(&*c.audit_seq.to_string())
    );
    assert_eq!(c.audit_seq, before as u64);
    assert_eq!(field(complete, "marking"), Some("SECRET:ALPHA"));
    assert_eq!(marking_label(&marking), "SECRET:ALPHA");
    assert_eq!(field(complete, "model"), Some(c.model_id.as_str()));

    // The digests are recomputed here from the request and the served text alone.
    let prompt = format!("{}\n\n{}", req.system_prompt, req.prompt);
    assert_eq!(
        field(complete, "prompt_sha256"),
        Some(hex_sha256(prompt.as_bytes()).as_str())
    );
    assert_eq!(
        field(complete, "completion_sha256"),
        Some(hex_sha256(c.text.as_bytes()).as_str())
    );
    assert_eq!(
        field(complete, "completion_tokens"),
        Some(&*c.completion_tokens.to_string())
    );
}

// [VLD-ANB-MIDGARD-0034] [AC-65.4]
#[test]
fn audit_failure_after_completion_is_a_refusal() {
    let (vk, sk) = fips_provider()
        .sig_for(GOV_SIG_ALG)
        .unwrap()
        .keypair(&mut SeedRng(77))
        .unwrap();
    let writer = AuditWriter::new(
        FailsOnSecondWrite::default(),
        Box::new(FipsSig::new(GOV_SIG_ALG)),
        sk,
        vk,
        LedgerId("mimir-inference".into()),
    );
    let h = InferenceHandler::new(
        server(),
        Governance::with_audit(pdp(), writer),
        inference_resource(),
        Arc::new(LogicalClock::starting_at(1000)),
    );

    // The decision is recorded and the model answers, but the completion entry
    // cannot be written: the completion is withheld.
    match call(&h, &request(unclassified())) {
        InferenceResponse::Denied { reason, audit_seq } => {
            assert_eq!(reason, "AUDIT_FAILURE");
            assert_eq!(audit_seq, 0, "the decision entry that was written");
        }
        InferenceResponse::Ok(_) => panic!("a completion was served unaudited"),
    }
    assert_eq!(h.audit_actions(), vec!["PERMIT Infer".to_string()]);
}

/// The call is authorised at the lower of the caller's certified clearance and the
/// model's accreditation: a model accredited for CONFIDENTIAL still refuses a
/// CONFIDENTIAL prompt from a caller certified only for UNCLASSIFIED data.
/// [VLD-ANB-MIDGARD-0034] [VLD-ANB-MIDGARD-0063]
#[test]
fn the_callers_certified_clearance_also_bounds_the_prompt() {
    let marked = parse_marking("CONFIDENTIAL").unwrap();
    let h = handler(clearance("CONFIDENTIAL"));
    assert!(matches!(
        call_as(&h, &channel_at(Posture::unclassified()), &request(marked.clone())),
        InferenceResponse::Denied { ref reason, .. } if reason == "POLICY_DENIED"
    ));
    assert!(matches!(
        call(&h, &request(marked)),
        InferenceResponse::Ok(_)
    ));
}
