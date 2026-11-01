//! Tamper detection, including the adversarial compromised-signer case.
#![cfg(any(feature = "test-backend", feature = "backend-fips"))]

mod common;

use audit_writer::{
    entry_hash, signing_preimage, verify_chain, Action, AuditError, AuditWriter, InMemoryLedger,
    LedgerId, SealedEntry,
};
use common::SeedRng;
use common::{provider, Sig};
use pqc_agility::{SigId, SignatureScheme, VerifyingKey};

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
fn mutated_record_is_detected() {
    let (mut entries, ledger, vk) = build_chain();
    entries[1].record.action = Action("evil".into());
    let p = provider();
    let r = resolver(vk);
    let err = verify_chain(&entries, &p, &ledger, &r).unwrap_err();
    assert!(
        matches!(err, AuditError::HashMismatch { seq: 1 }),
        "{err:?}"
    );
}

#[test]
fn swapped_signature_is_detected() {
    let (mut entries, ledger, vk) = build_chain();
    let sig0 = entries[0].pq_signature.clone();
    let sig1 = entries[1].pq_signature.clone();
    entries[0].pq_signature = sig1;
    entries[1].pq_signature = sig0;
    let p = provider();
    let r = resolver(vk);
    let err = verify_chain(&entries, &p, &ledger, &r).unwrap_err();
    assert!(
        matches!(err, AuditError::SignatureInvalid { seq: 0 }),
        "{err:?}"
    );
}

#[test]
fn wrong_key_resign_is_detected() {
    let (mut entries, ledger, vk) = build_chain();
    // Re-sign entry 1 with a different key but the correct entry hash.
    let scheme = Sig::new(ALG);
    let mut rng = SeedRng::new(999);
    let (_wrong_vk, wrong_sk) = scheme.keypair(&mut rng).unwrap();
    let eh = entries[1].entry_hash;
    entries[1].pq_signature = scheme.sign(&wrong_sk, &signing_preimage(&eh)).unwrap();
    let p = provider();
    let r = resolver(vk);
    let err = verify_chain(&entries, &p, &ledger, &r).unwrap_err();
    assert!(
        matches!(err, AuditError::SignatureInvalid { seq: 1 }),
        "{err:?}"
    );
}

#[test]
fn compromised_signer_cannot_rewrite_history() {
    // The adversarial case: an attacker with the *real* signing key rewrites a
    // historical record and re-signs it correctly. The entry itself now verifies,
    // but its successor's prev_hash still points at the original hash → the chain
    // breaks. This proves the chain protects even a compromised signer for any
    // entry that already has a successor.
    let (mut entries, ledger, vk) = build_chain();
    let sk = common::signing_key(ALG, 1); // the genuine key
    let scheme = Sig::new(ALG);

    entries[1].record.action = Action("tampered".into());
    let new_eh = entry_hash(entries[1].seq, &entries[1].prev_hash, &entries[1].record);
    entries[1].entry_hash = new_eh;
    entries[1].pq_signature = scheme.sign(&sk, &signing_preimage(&new_eh)).unwrap();

    let p = provider();
    let r = resolver(vk);
    let err = verify_chain(&entries, &p, &ledger, &r).unwrap_err();
    assert!(
        matches!(err, AuditError::ChainBroken { seq: 2, .. }),
        "{err:?}"
    );
}
