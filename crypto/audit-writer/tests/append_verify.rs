//! Happy path: append a chain and verify it end to end; and a ledger written before
//! records carried a context still opens, verifies and grows.
#![cfg(any(feature = "test-backend", feature = "backend-fips"))]

mod common;

use audit_writer::{
    entry_hash, verify_chain, verify_ledger, AuditContext, AuditError, AuditWriter, FileLedger,
    InMemoryLedger, LedgerId, LedgerStore, Outcome, PinnedKey, SealedEntry, Sealer,
};
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

/// A stand-in for the KMS sealer that binds the AAD, as in `truncation.rs`.
struct AadSealer;
impl Sealer for AadSealer {
    fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
        let mut o = (aad.len() as u32).to_le_bytes().to_vec();
        o.extend_from_slice(aad);
        o.extend_from_slice(plaintext);
        Ok(o)
    }
    fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
        let n = sealed
            .get(..4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| AuditError::Store("short".into()))?;
        if sealed.get(4..4 + n) != Some(aad) {
            return Err(AuditError::Store("aad mismatch".into()));
        }
        Ok(sealed[4 + n..].to_vec())
    }
}

/// The file ledger's frame AAD: its domain tag, the ledger id and the position.
fn frame_aad(ledger: &LedgerId, seq: u64) -> Vec<u8> {
    let mut a = b"midgard-audit-file-v1".to_vec();
    a.extend_from_slice(&(ledger.0.len() as u32).to_le_bytes());
    a.extend_from_slice(ledger.0.as_bytes());
    a.extend_from_slice(&seq.to_le_bytes());
    a
}

/// An entry in the schema 1 layout, the only one there was before the context.
fn schema_one(e: &SealedEntry) -> Vec<u8> {
    let mut o = vec![1u8];
    o.extend_from_slice(&e.seq.to_le_bytes());
    o.extend_from_slice(&e.prev_hash);
    o.extend_from_slice(&e.entry_hash);
    o.push(e.sig_alg.as_u8());
    let r = &e.record;
    for field in [
        e.pq_signature.as_bytes(),
        r.id.0.as_bytes(),
        r.actor.0.as_bytes(),
        r.action.0.as_bytes(),
        r.resource.0.as_bytes(),
        r.ts.0.as_bytes(),
    ] {
        o.extend_from_slice(&(field.len() as u32).to_le_bytes());
        o.extend_from_slice(field);
    }
    o
}

/// A ledger of schema 1 frames, sealed and checkpointed as the previous release left
/// it, opens with this code: every entry decodes, hashes to its stored entry hash and
/// verifies under the pinned key with its checkpoint; a new entry with a populated
/// context is appended and survives a reopen, and the whole chain still verifies.
/// [VLD-ANB-MIDGARD-0052] [AC-26.1] [MG-SEC-05]
#[test]
fn a_ledger_written_before_the_context_existed_still_verifies() {
    let dir = std::env::temp_dir().join(format!(
        "audit-v1-ledger-{}-{}",
        std::process::id(),
        Box::into_raw(Box::new(0u8)) as usize
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("gov.ledger");
    let id = LedgerId("gov".into());
    let writer = |store: FileLedger| {
        let (scheme, sk, vk) = common::signer(SigId::MlDsa65, 1);
        AuditWriter::new(store, scheme, sk, vk, id.clone())
    };
    let pin = PinnedKey::new(common::signer(SigId::MlDsa65, 1).2);

    // Three rows and their signed checkpoint, then the ledger rewritten frame by
    // frame in the schema 1 layout.
    let old: Vec<SealedEntry> = {
        let mut w = writer(FileLedger::create(&path, Box::new(AadSealer), id.clone()).unwrap());
        w.seal_checkpoint().unwrap();
        for i in 0..3 {
            w.append(common::record(
                &format!("r{i}"),
                "alice",
                "PERMIT Read",
                "obj:1",
            ))
            .unwrap();
        }
        w.entries().to_vec()
    };
    let mut frames = Vec::new();
    for e in &old {
        let sealed = AadSealer
            .seal(&frame_aad(&id, e.seq), &schema_one(e))
            .unwrap();
        frames.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
        frames.extend_from_slice(&sealed);
    }
    std::fs::write(&path, frames).unwrap();

    // Opened by this code: the same entries, each re-hashing to its stored hash.
    let ledger = FileLedger::open(&path, Box::new(AadSealer), id.clone()).unwrap();
    assert_eq!(ledger.entries(), &old[..]);
    for e in ledger.entries() {
        assert!(e.record.context.is_empty());
        assert_eq!(entry_hash(e.seq, &e.prev_hash, &e.record), e.entry_hash);
    }
    verify_ledger(
        ledger.entries(),
        ledger.checkpoint(),
        &provider(),
        &id,
        &pin,
    )
    .expect("the schema 1 ledger verifies with its checkpoint");

    // A new row with a context, then a reopen.
    let context = AuditContext {
        source: Some("127.0.0.1".into()),
        session: Some("alice:1800000000".into()),
        outcome: Outcome::Refused,
        reason: Some("human-gate-missing".into()),
        target: Some("aircraft/1".into()),
        ..AuditContext::default()
    };
    let mut w = writer(ledger);
    let mut record = common::record("AUD-3", "alice", "REFUSED Apply", "aircraft/1");
    record.context = context.clone();
    assert_eq!(w.append(record).unwrap(), 3);
    drop(w);

    let ledger = FileLedger::open(&path, Box::new(AadSealer), id.clone()).unwrap();
    let entries = ledger.entries();
    assert_eq!(entries.len(), 4);
    assert_eq!(&entries[..3], &old[..]);
    assert_eq!(entries[3].record.context, context);
    verify_ledger(entries, ledger.checkpoint(), &provider(), &id, &pin)
        .expect("old and new rows verify together");
    let _ = std::fs::remove_dir_all(&dir);
}
