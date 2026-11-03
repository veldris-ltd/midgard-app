//! AC-H-4 — Given a consequential action, when proposed, then it is not executed
//! without explicit human authorisation and is fully audited. Exercises the gate and
//! the PQ-signed audit chain with the real FIPS backend, and the end-to-end lineage
//! from an advisory A.8 option → proposed action → human authorisation → audited
//! execution. [FR-H-5] [NFR-AI-1]
//!
//! WP-64: an action's class comes from the deployment's catalogue, never the
//! proposer; an unknown type is refused before anything is audited; every
//! execution, informational included, needs a recorded human decision; the
//! actuator re-checks the catalogue; and only a grounded option can become an
//! action. [AC-64.1] [AC-64.2] [AC-64.3] [AC-64.4] [AC-64.5]
//! [VLD-ANB-MIDGARD-0045] [VLD-ANB-MIDGARD-0079]
#![cfg(feature = "backend-fips")]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use audit_writer::{verify_chain, AuditWriter, InMemoryLedger, LedgerId};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::registry::CryptoProvider;
use pqc_agility::{Rng, SigId, SignatureScheme, SigningKey, VerifyingKey};

use heimdall_decision_support::{request_options, MockOptionProvider};
use heimdall_picture::{
    Classification, ClassificationSource, EntityId, LayerId, Picture, Track, TrackedEntity,
};
use heimdall_workflow::{
    ActionClass, ActionTypeCatalog, Actuator, ActuatorError, DispatchActuator, Effector,
    ProposedAction, WorkflowEngine, WorkflowError, WorkflowState,
};

const ALG: SigId = SigId::MlDsa65;

/// A minimal success effector for the tests' action types.
struct OkEffector;
impl Effector for OkEffector {
    fn effect(&mut self, action: &ProposedAction) -> Result<String, String> {
        Ok(format!("actuated {}", action.action_type))
    }
}

/// The deployment's action-type catalogue for these tests.
fn catalog() -> ActionTypeCatalog {
    ActionTypeCatalog::new()
        .with("dispatch-asset", ActionClass::Consequential)
        .with("cue-sensor", ActionClass::Consequential)
        .with("annotate-track", ActionClass::Informational)
}

/// A dispatcher wired for the action types these tests use, re-checking against
/// the same catalogue as the engine.
fn actuator() -> DispatchActuator {
    let mut a = DispatchActuator::new().with_catalog(catalog());
    a.register("dispatch-asset", Box::new(OkEffector));
    a.register("cue-sensor", Box::new(OkEffector));
    a.register("annotate-track", Box::new(OkEffector));
    a
}

/// An effector that records every action it was invoked for.
struct RecordingEffector {
    log: Rc<RefCell<Vec<String>>>,
}
impl Effector for RecordingEffector {
    fn effect(&mut self, action: &ProposedAction) -> Result<String, String> {
        self.log.borrow_mut().push(action.id.clone());
        Ok("done".into())
    }
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
    (WorkflowEngine::new(writer).with_catalog(catalog()), vk)
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
    // turns the chosen grounded option into a proposed action carrying its cited
    // evidence → human authorises → audited execution. [AC-64.5]
    let mut picture = Picture::new();
    picture.upsert_entity(tracked("A123", "radar"));

    // Rank 1 invents its evidence; rank 2 is grounded. Only rank 2 is offered, and
    // it is re-ranked to 1.
    let a8 = r#"{"options":[{"rank":1,"title":"Strike GHOST","summary":"act on a phantom",
        "citations":["entity:GHOST"],"expected_effect":"","risks":["cost"]},
        {"rank":2,"title":"Cue radar on A123",
        "summary":"Task the radar for a closer look.","citations":["entity:A123","feed:radar"],
        "expected_effect":"tighter track","risks":["cost"]}],"unsupported":null}"#;
    let advisory = request_options(&picture, "handle A123", &MockOptionProvider::new(a8)).unwrap();
    assert_eq!(advisory.grounded.len(), 1);
    assert_eq!(advisory.rejected.len(), 1);

    // Operator selects rank-1 of the grounded set and proposes an action from it —
    // the option is advisory; this proposal still requires explicit authorisation.
    // `from_course_of_action` takes only a GroundedOption, which only the adapter
    // makes. [VLD-ANB-MIDGARD-0079]
    let chosen = advisory.grounding_for(1).expect("grounded rank 1");
    assert_eq!(chosen.title(), "Cue radar on A123");
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
    // Every citation in the audited action resolves against the picture.
    for c in &wf.record("act-42").unwrap().action.citations {
        assert!(resolves(&picture, c), "unresolved citation {c} in lineage");
    }
    assert_eq!(wf.audit_entries().len(), 4);
    assert!(chain_ok(&wf, &vk));
}

/// Whether a raw citation token names an entity in `picture`, or a feed that
/// contributed to one.
fn resolves(picture: &Picture, token: &str) -> bool {
    match token.split_once(':') {
        Some(("entity", id)) | Some(("track", id)) => picture.entities().any(|e| e.id.0 == id),
        Some(("feed", id)) => picture.entities().any(|e| e.sources.contains(id)),
        _ => false,
    }
}

#[test]
fn unknown_action_type_is_refused_at_proposal() {
    // [AC-64.2] [VLD-ANB-MIDGARD-0045]
    let (mut wf, _vk) = engine();
    assert_eq!(
        wf.propose(
            "analyst",
            ProposedAction::new("act-x", "self-destruct", "no reason"),
            "t0"
        ),
        Err(WorkflowError::UnknownActionType("self-destruct".into()))
    );
    // Nothing was audited and no workflow exists.
    assert!(wf.audit_entries().is_empty());
    assert!(wf.record("act-x").is_none());

    // An engine with no catalogue at all refuses everything (fail closed).
    let (vk, sk) = keypair(2);
    let writer = AuditWriter::new(
        InMemoryLedger::new(),
        Box::new(FipsSig::new(ALG)),
        sk,
        vk,
        LedgerId("heimdall-workflow".into()),
    );
    let mut bare = WorkflowEngine::new(writer);
    assert_eq!(
        bare.propose(
            "analyst",
            ProposedAction::new("act-y", "cue-sensor", "look"),
            "t0"
        ),
        Err(WorkflowError::UnknownActionType("cue-sensor".into()))
    );
    assert!(bare.audit_entries().is_empty());
}

#[test]
fn informational_action_still_needs_a_recorded_human_decision() {
    // [AC-64.3] [VLD-ANB-MIDGARD-0045]
    let (mut wf, vk) = engine();
    wf.propose(
        "analyst",
        ProposedAction::new("note-1", "annotate-track", "mark A123 as of interest"),
        "t0",
    )
    .unwrap();
    assert_eq!(
        wf.record("note-1").unwrap().action.class(),
        ActionClass::Informational
    );

    // Informational is not a shortcut round the gate.
    assert_eq!(
        wf.execute("system", "note-1", &mut actuator(), "t1"),
        Err(WorkflowError::NotAuthorised("note-1".into()))
    );
    assert_eq!(wf.state_of("note-1"), Some(&WorkflowState::Proposed));
    assert_eq!(
        wf.audit_entries().len(),
        1,
        "the refused attempt ran nothing"
    );

    // A human acknowledges it; only then does it execute.
    wf.authorise("note-1", "cdr-jones", true, "t2").unwrap();
    wf.execute("system", "note-1", &mut actuator(), "t3")
        .unwrap();
    let actions: Vec<&str> = wf
        .audit_entries()
        .iter()
        .map(|e| e.record.action.0.as_str())
        .collect();
    assert_eq!(
        actions,
        vec![
            "propose-action",
            "authorise-action",
            "execute-action",
            "actuation-succeeded"
        ]
    );
    assert_eq!(wf.audit_entries()[1].record.actor.0, "cdr-jones");
    assert!(chain_ok(&wf, &vk));
}

#[test]
fn proposer_cannot_assert_the_class() {
    // [AC-64.1] [VLD-ANB-MIDGARD-0045]
    // By API shape: `ProposedAction` has no public class setter and its class field
    // is private (the compile_fail doctest on `ProposedAction` proves the field
    // cannot be written). The only way a class is set is the engine's catalogue
    // lookup at proposal.
    let (mut wf, _vk) = engine();

    // Built through the public constructor, a dispatch-asset action is recorded with
    // the catalogue's class.
    wf.propose(
        "analyst",
        ProposedAction::new("act-5", "dispatch-asset", "intercept"),
        "t0",
    )
    .unwrap();
    let rec = wf.record("act-5").unwrap();
    assert_eq!(
        rec.action.class(),
        catalog().class_of("dispatch-asset").unwrap()
    );
    assert!(rec.action.is_consequential());

    // And the class really is derived, not defaulted: a fresh action starts
    // consequential, and the engine records annotate-track as the catalogue says.
    let fresh = ProposedAction::new("note-5", "annotate-track", "note");
    assert_eq!(fresh.class(), ActionClass::Consequential);
    wf.propose("analyst", fresh, "t1").unwrap();
    assert_eq!(
        wf.record("note-5").unwrap().action.class(),
        ActionClass::Informational
    );
}

#[test]
fn actuator_refuses_a_type_outside_the_catalogue() {
    // [AC-64.4] [VLD-ANB-MIDGARD-0045]
    let (mut wf, _vk) = engine();
    wf.propose(
        "analyst",
        ProposedAction::new("act-6", "cue-sensor", "look closer"),
        "t0",
    )
    .unwrap();
    wf.authorise("act-6", "cdr-jones", true, "t1").unwrap();

    // The actuator's catalogue does not list cue-sensor, although an effector is
    // registered for it.
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut narrow = DispatchActuator::new()
        .with_catalog(ActionTypeCatalog::new().with("dispatch-asset", ActionClass::Consequential));
    narrow.register(
        "cue-sensor",
        Box::new(RecordingEffector { log: log.clone() }),
    );

    let action = wf.record("act-6").unwrap().action.clone();
    assert_eq!(
        narrow.actuate(&action),
        Err(ActuatorError::Unsupported("cue-sensor".into()))
    );
    let err = wf
        .execute("operator", "act-6", &mut narrow, "t2")
        .unwrap_err();
    assert!(
        matches!(&err, WorkflowError::Actuation(m) if m.contains("not in the actuator's catalogue")),
        "{err:?}"
    );
    assert!(log.borrow().is_empty(), "the effector was never invoked");
    // Fails closed: still authorised, with the attempt audited.
    assert!(matches!(
        wf.state_of("act-6"),
        Some(WorkflowState::Authorised { .. })
    ));
    assert_eq!(
        wf.audit_entries().last().unwrap().record.action.0,
        "actuation-error"
    );
}

#[test]
fn actuator_refuses_a_class_mismatch() {
    // [AC-64.4] [VLD-ANB-MIDGARD-0045]
    // The engine classified annotate-track as informational; an actuator holding a
    // second catalogue that calls it consequential refuses to run it.
    let (mut wf, _vk) = engine();
    wf.propose(
        "analyst",
        ProposedAction::new("note-7", "annotate-track", "note"),
        "t0",
    )
    .unwrap();
    wf.authorise("note-7", "cdr-jones", true, "t1").unwrap();

    let log = Rc::new(RefCell::new(Vec::new()));
    let mut other = DispatchActuator::new()
        .with_catalog(ActionTypeCatalog::new().with("annotate-track", ActionClass::Consequential));
    other.register(
        "annotate-track",
        Box::new(RecordingEffector { log: log.clone() }),
    );

    let action = wf.record("note-7").unwrap().action.clone();
    assert_eq!(
        other.actuate(&action),
        Err(ActuatorError::ClassMismatch {
            action_type: "annotate-track".into(),
            catalogued: ActionClass::Consequential,
            recorded: ActionClass::Informational,
        })
    );
    let err = wf
        .execute("operator", "note-7", &mut other, "t2")
        .unwrap_err();
    assert!(
        matches!(&err, WorkflowError::Actuation(m) if m.contains("recorded as informational")),
        "{err:?}"
    );
    assert!(log.borrow().is_empty(), "the effector was never invoked");
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
            last_received: 105,
        },
        classification: Some(Classification {
            label: "fast-mover".into(),
            source: ClassificationSource::Rules,
            rationale: "speed".into(),
        }),
        latest_attrs: BTreeMap::new(),
    }
}
