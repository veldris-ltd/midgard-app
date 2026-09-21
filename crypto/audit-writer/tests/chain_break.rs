//! Reorder / gap / cross-ledger splice detection.
#![cfg(feature = "test-backend")]

mod common;

use audit_writer::{verify_chain, AuditError, AuditWriter, InMemoryLedger, LedgerId, SealedEntry};
use pqc_agility::backend::test_backend::provider;
use pqc_agility::{SigId, VerifyingKey};

const ALG: SigId = SigId::MlDsa65;

fn build_chain() -> (Vec<SealedEntry>, LedgerId, VerifyingKey) {
    let (scheme, sk, vk) = common::signer(ALG, 1);
    let mut w = AuditWriter::new(
        InMemoryLedger::new(),
        scheme,
        sk,
        vk.clone(),
        LedgerId("L".into()),
    );
    for i in 0..3 {
        w.append(common::record(&format!("r{i}"), "alice", "read", "obj:1"))
            .unwrap();
    }
    (w.entries().to_vec(), w.ledger_id().clone(), vk)
}

fn resolver(vk: VerifyingKey) -> impl Fn(SigId) -> Option<VerifyingKey> {
    move |alg: SigId| if alg == ALG { Some(vk.clone()) } else { None }
}

#[test]
fn reorder_is_detected() {
    let (mut entries, ledger, vk) = build_chain();
    entries.swap(1, 2);
    let p = provider();
    let r = resolver(vk);
    let err = verify_chain(&entries, &p, &ledger, &r).unwrap_err();
    assert!(matches!(err, AuditError::SequenceGap { .. }), "{err:?}");
}

#[test]
fn truncation_gap_is_detected() {
    let (mut entries, ledger, vk) = build_chain();
    entries.remove(1); // leaves seqs [0, 2]
    let p = provider();
    let r = resolver(vk);
    let err = verify_chain(&entries, &p, &ledger, &r).unwrap_err();
    assert!(
        matches!(
            err,
            AuditError::SequenceGap {
                expected: 1,
                got: 2
            }
        ),
        "{err:?}"
    );
}

#[test]
fn cross_ledger_splice_is_detected() {
    let (entries, _ledger, vk) = build_chain();
    let p = provider();
    let r = resolver(vk);
    // Verifying against a different ledger id changes the genesis hash.
    let wrong_ledger = LedgerId("OTHER".into());
    let err = verify_chain(&entries, &p, &wrong_ledger, &r).unwrap_err();
    assert!(
        matches!(err, AuditError::ChainBroken { seq: 0, .. }),
        "{err:?}"
    );
}
