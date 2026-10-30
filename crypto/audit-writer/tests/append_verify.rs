//! Happy path: append a chain and verify it end to end.
#![cfg(any(feature = "test-backend", feature = "backend-fips"))]

mod common;

use audit_writer::{verify_chain, AuditWriter, InMemoryLedger, LedgerId};
use common::provider;
use pqc_agility::SigId;

#[test]
fn append_and_verify_chain() {
    let (scheme, sk, vk) = common::signer(SigId::MlDsa65, 1);
    let mut w = AuditWriter::new(
        InMemoryLedger::new(),
        scheme,
        sk,
        vk.clone(),
        LedgerId("ledger-1".into()),
    );
    for i in 0..5 {
        let seq = w
            .append(common::record(&format!("r{i}"), "alice", "read", "obj:1"))
            .unwrap();
        assert_eq!(seq, i);
    }
    let entries = w.entries().to_vec();
    assert_eq!(entries.len(), 5);

    let p = provider();
    let vk2 = vk.clone();
    let resolver = move |alg: SigId| {
        if alg == SigId::MlDsa65 {
            Some(vk2.clone())
        } else {
            None
        }
    };
    verify_chain(&entries, &p, w.ledger_id(), &resolver).expect("chain must verify");
}
