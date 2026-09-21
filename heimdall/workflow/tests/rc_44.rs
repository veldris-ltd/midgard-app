//! RC-44: an approved workflow action **invokes a real actuator and is audited**.
//!
//! Proves the RT-44 seam end-to-end: after the human gate, execution drives a real
//! [`Effector`] (here one that records the command it carried out, standing in for a
//! live sensor/asset effector), the effector's outcome is sealed into the PQ-signed
//! audit chain, and an action the actuator cannot invoke fails closed — audited,
//! not executed. [RT-44]
#![cfg(feature = "backend-fips")]

use std::cell::RefCell;
use std::rc::Rc;

use audit_writer::{verify_chain, AuditWriter, InMemoryLedger, LedgerId};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::registry::CryptoProvider;
use pqc_agility::{Rng, SigId, SignatureScheme, SigningKey, VerifyingKey};

use heimdall_workflow::{
    ActuationStatus, DispatchActuator, Effector, ProposedAction, WorkflowEngine, WorkflowError,
    WorkflowState,
};

const ALG: SigId = SigId::MlDsa65;

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

fn provider() -> CryptoProvider {
    let mut p = CryptoProvider::new();
    p.register_sig(Box::new(FipsSig::new(ALG)));
    p
}

fn engine() -> (WorkflowEngine<InMemoryLedger>, VerifyingKey) {
    let (vk, sk): (VerifyingKey, SigningKey) = FipsSig::new(ALG).keypair(&mut SeedRng(7)).unwrap();
    let writer = AuditWriter::new(
        InMemoryLedger::new(),
        Box::new(FipsSig::new(ALG)),
        sk,
        vk.clone(),
        LedgerId("heimdall-workflow".into()),
    );
    (WorkflowEngine::new(writer), vk)
}

fn chain_ok(engine: &WorkflowEngine<InMemoryLedger>, vk: &VerifyingKey) -> bool {
    let vk = vk.clone();
    verify_chain(
        engine.audit_entries(),
        &provider(),
        engine.ledger_id(),
        &|alg| {
            if alg == ALG {
                Some(vk.clone())
            } else {
                None
            }
        },
    )
    .is_ok()
}

/// An effector that records every command it actually carried out — observable
/// proof the actuator invoked it (a live deployment would drive a real effector).
struct RecordingEffector {
    log: Rc<RefCell<Vec<String>>>,
}
impl Effector for RecordingEffector {
    fn effect(&mut self, action: &ProposedAction) -> Result<String, String> {
        let target = action.params.get("target").cloned().unwrap_or_default();
        self.log
            .borrow_mut()
            .push(format!("{}:{target}", action.action_type));
        Ok(format!("dispatched to {target}"))
    }
}

#[test]
fn approved_action_invokes_a_real_actuator_and_is_audited() {
    let (mut wf, vk) = engine();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut actuator = DispatchActuator::new();
    actuator.register(
        "dispatch-asset",
        Box::new(RecordingEffector { log: log.clone() }),
    );

    let action =
        ProposedAction::new("t-1", "dispatch-asset", "intercept").with_param("target", "asset-7");
    wf.propose("analyst", action, "t0").unwrap();
    wf.authorise("t-1", "cdr-jones", true, "t1").unwrap();

    // The actuator has NOT run yet — proposing/authorising never actuates.
    assert!(log.borrow().is_empty(), "no side effect before execution");

    // Execute: the real effector is invoked and returns its outcome.
    let outcome = wf.execute("operator", "t-1", &mut actuator, "t2").unwrap();
    assert_eq!(outcome.status, ActuationStatus::Succeeded);
    assert_eq!(outcome.detail, "dispatched to asset-7");

    // The effector actually carried out the command (observable side effect).
    assert_eq!(*log.borrow(), vec!["dispatch-asset:asset-7"]);

    // ...and the actuation is audited: propose, authorise, execute-intent,
    // actuation-succeeded — the outcome sealed into the chain.
    let entries = wf.audit_entries();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[2].record.action.0, "execute-action");
    assert_eq!(entries[3].record.action.0, "actuation-succeeded");
    assert!(matches!(
        wf.state_of("t-1"),
        Some(WorkflowState::Executed { .. })
    ));
    assert!(chain_ok(&wf, &vk));
}

#[test]
fn unactuatable_action_fails_closed_and_is_audited() {
    let (mut wf, vk) = engine();
    // The dispatcher has NO effector for this action type.
    let mut actuator = DispatchActuator::new();

    wf.propose(
        "analyst",
        ProposedAction::new("t-2", "launch-drone", "recon"),
        "t0",
    )
    .unwrap();
    wf.authorise("t-2", "cdr-jones", true, "t1").unwrap();

    // Execution is attempted but the actuator cannot invoke it → fails closed.
    let err = wf
        .execute("operator", "t-2", &mut actuator, "t2")
        .unwrap_err();
    assert!(
        matches!(err, WorkflowError::Actuation(_)),
        "unexpected: {err:?}"
    );

    // The action did NOT execute — it stays authorised (retryable).
    assert_eq!(
        wf.state_of("t-2"),
        Some(&WorkflowState::Authorised {
            actor: "cdr-jones".into()
        })
    );
    // But the failed attempt IS audited: execute-intent + actuation-error.
    let entries = wf.audit_entries();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[2].record.action.0, "execute-action");
    assert_eq!(entries[3].record.action.0, "actuation-error");
    assert!(chain_ok(&wf, &vk));
}
