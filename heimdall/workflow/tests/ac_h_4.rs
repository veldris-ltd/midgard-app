//! AC-H-4 — Given a consequential action, when proposed, then it is not executed
//! without explicit human authorisation and is fully audited. Exercises the gate and
//! the PQ-signed audit chain with the real FIPS backend, and the end-to-end lineage
//! from an advisory A.8 option → proposed action → human authorisation → audited
//! execution. [FR-H-5] [NFR-AI-1]
#![cfg(feature = "backend-fips")]

use std::collections::{BTreeMap, BTreeSet};

use audit_writer::{verify_chain, AuditWriter, InMemoryLedger, LedgerId};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::registry::CryptoProvider;
use pqc_agility::{Rng, SigId, SignatureScheme, SigningKey, VerifyingKey};

use heimdall_decision_support::{request_options, MockOptionProvider};
use heimdall_picture::{
    Classification, ClassificationSource, EntityId, LayerId, Picture, Track, TrackedEntity,
};
use heimdall_workflow::{
    DispatchActuator, Effector, ProposedAction, WorkflowEngine, WorkflowError, WorkflowState,
};

const ALG: SigId = SigId::MlDsa65;

/// A minimal success effector for the tests' action types.
struct OkEffector;
impl Effector for OkEffector {
    fn effect(&mut self, action: &ProposedAction) -> Result<String, String> {
        Ok(format!("actuated {}", action.action_type))
    }
}

/// A dispatcher wired for the action types these tests use.
fn actuator() -> DispatchActuator {
    let mut a = DispatchActuator::new();
    a.register("dispatch-asset", Box::new(OkEffector));
    a.register("cue-sensor", Box::new(OkEffector));
    a
}

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

fn keypair(seed: u64) -> (VerifyingKey, SigningKey) {
    FipsSig::new(ALG).keypair(&mut SeedRng(seed)).unwrap()
}

fn provider() -> CryptoProvider {
    let mut p = CryptoProvider::new();
    p.register_sig(Box::new(FipsSig::new(ALG)));
    p
}

/// A workflow engine over an in-memory PQ-signed audit ledger, plus its verifying
/// key (for chain verification).
fn engine() -> (WorkflowEngine<InMemoryLedger>, VerifyingKey) {
    let (vk, sk) = keypair(1);
    let writer = AuditWriter::new(
        InMemoryLedger::new(),
        Box::new(FipsSig::new(ALG)),
        sk,
        vk.clone(),
        LedgerId("heimdall-workflow".into()),
    );
    (WorkflowEngine::new(writer), vk)
}

/// The audit chain verifies against the engine's key.
fn chain_ok(engine: &WorkflowEngine<InMemoryLedger>, vk: &VerifyingKey) -> bool {
    let vk = vk.clone();
    verify_chain(
        engine.audit_entries(),
        &provider(),
        engine.ledger_id(),
        &|alg| if alg == ALG { Some(vk.clone()) } else { None },
    )
    .is_ok()
}

#[test]
fn ac_h_4_consequential_action_needs_human_authorisation_and_is_audited() {
    let (mut wf, vk) = engine();
    let action = ProposedAction::new("act-1", "dispatch-asset", "intercept the fast mover")
        .with_citations(vec!["entity:A123".into(), "feed:radar".into()]);

    wf.propose("analyst", action, "t0").unwrap();

    // Attempting to execute WITHOUT authorisation fails closed — the actuator is
    // never invoked and nothing runs.
    assert_eq!(
        wf.execute("system", "act-1", &mut actuator(), "t1"),
        Err(WorkflowError::NotAuthorised("act-1".into()))
    );
    assert_eq!(wf.state_of("act-1"), Some(&WorkflowState::Proposed));

    // A human authorises; only now can it execute — invoking the real actuator.
    wf.authorise("act-1", "cdr-jones", true, "t2").unwrap();
    assert_eq!(
        wf.state_of("act-1"),
        Some(&WorkflowState::Authorised {
            actor: "cdr-jones".into()
        })
    );
    wf.execute("system", "act-1", &mut actuator(), "t3")
        .unwrap();
    assert!(matches!(
        wf.state_of("act-1"),
        Some(WorkflowState::Executed { .. })
    ));

    // Full lineage: propose → authorise → execute-intent → actuation-outcome, all
    // sealed and chain-verifiable (execution is now two audited phases).
    let record = wf.record("act-1").unwrap();
    assert_eq!(record.audit_seqs, vec![0, 1, 2, 3]);
    assert_eq!(wf.audit_entries().len(), 4);
    assert_eq!(wf.audit_entries()[1].record.actor.0, "cdr-jones");
    assert_eq!(wf.audit_entries()[1].record.action.0, "authorise-action");
    assert_eq!(wf.audit_entries()[2].record.action.0, "execute-action");
    assert_eq!(wf.audit_entries()[3].record.action.0, "actuation-succeeded");
    assert!(chain_ok(&wf, &vk));
}

#[test]
fn ac_h_4_denied_action_cannot_execute() {
    let (mut wf, vk) = engine();
    wf.propose(
        "analyst",
        ProposedAction::new("act-2", "dispatch-asset", "engage"),
        "t0",
    )
    .unwrap();

    // A human denies it → it can never execute.
    wf.authorise("act-2", "cdr-jones", false, "t1").unwrap();
    assert_eq!(
        wf.execute("system", "act-2", &mut actuator(), "t2"),
        Err(WorkflowError::Denied("act-2".into()))
    );
    assert!(matches!(
        wf.state_of("act-2"),
        Some(WorkflowState::Denied { .. })
    ));
    // The denial itself is audited (propose + deny).
    assert_eq!(wf.audit_entries().len(), 2);
    assert_eq!(wf.audit_entries()[1].record.action.0, "deny-action");
    assert!(chain_ok(&wf, &vk));
}

#[test]
fn ac_h_4_tampering_with_the_audit_chain_is_detected() {
    let (mut wf, vk) = engine();
    wf.propose(
        "analyst",
        ProposedAction::new("act-3", "cue-sensor", "look closer"),
        "t0",
    )
    .unwrap();
    wf.authorise("act-3", "cdr-jones", true, "t1").unwrap();
    assert!(chain_ok(&wf, &vk));

    // A tampered record no longer matches its sealed hash → verification fails.
    let mut tampered = wf.audit_entries().to_vec();
    tampered[1].record.actor.0 = "impostor".into();
    let vk2 = vk.clone();
    assert!(verify_chain(
        &tampered,
        &provider(),
        wf.ledger_id(),
        &|alg| if alg == ALG { Some(vk2.clone()) } else { None },
    )
    .is_err());
}

#[test]
fn ac_h_3_and_4_option_to_audited_action_lineage() {
    // The full "AI proposes, human disposes" path: advisory options → the operator
    // turns the chosen option into a proposed action carrying its cited evidence →
    // human authorises → audited execution.
    let mut picture = Picture::new();
    picture.upsert_entity(tracked("A123", "radar"));

    let a8 = r#"{"options":[{"rank":1,"title":"Cue radar on A123",
        "summary":"Task the radar for a closer look.","citations":["entity:A123","feed:radar"],
        "expected_effect":"tighter track","risks":["cost"]}],"unsupported":null}"#;
    let advisory = request_options(&picture, "handle A123", &MockOptionProvider::new(a8)).unwrap();
    assert!(advisory.all_grounded());

    // Operator selects rank-1 and proposes an action from it — the option is
    // advisory; this proposal still requires explicit authorisation.
    let chosen = &advisory.option_set.options[0];
    let action = ProposedAction::from_course_of_action("act-42", "cue-sensor", chosen);
    assert_eq!(action.citations, vec!["entity:A123", "feed:radar"]);

    let (mut wf, vk) = engine();
    wf.propose("analyst", action, "t0").unwrap();
    // Still gated — the AI's option does not authorise anything.
    assert_eq!(
        wf.execute("system", "act-42", &mut actuator(), "t1"),
        Err(WorkflowError::NotAuthorised("act-42".into()))
    );
    wf.authorise("act-42", "cdr-jones", true, "t2").unwrap();
    wf.execute("system", "act-42", &mut actuator(), "t3")
        .unwrap();

    // The cited evidence rode into the workflow, and the whole lineage is audited
    // (propose + authorise + execute-intent + actuation-outcome).
    assert_eq!(
        wf.record("act-42").unwrap().action.citations,
        vec!["entity:A123", "feed:radar"]
    );
    assert_eq!(wf.audit_entries().len(), 4);
    assert!(chain_ok(&wf, &vk));
}

fn tracked(id: &str, feed: &str) -> TrackedEntity {
    let mut sources = BTreeSet::new();
    sources.insert(feed.to_string());
    TrackedEntity {
        id: EntityId(id.into()),
        entity_key: id.into(),
        kind: "air-track".into(),
        layer: LayerId("air".into()),
        sources,
        track: Track {
            first_seen: 100,
            last_seen: 105,
            observation_count: 2,
        },
        classification: Some(Classification {
            label: "fast-mover".into(),
            source: ClassificationSource::Rules,
            rationale: "speed".into(),
        }),
        latest_attrs: BTreeMap::new(),
    }
}
