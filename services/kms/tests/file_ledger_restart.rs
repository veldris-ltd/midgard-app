//! RC-09: the durable audit ledger survives a restart. Given a running ledger,
//! when the process restarts, every prior entry is recovered, the hash chain
//! re-verifies, entries are encrypted at rest (KMS envelope), and a tampered
//! on-disk entry is detected on load. [AC-M-4] [NFR-PQ-2] [NFR-PQ-3]

use std::path::PathBuf;

use audit_writer::{
    verify_chain, Action, Actor, AuditRecord, AuditWriter, FileLedger, LedgerId, RecordId,
    Resource, Timestamp,
};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{CryptoProvider, Rng, SigId, SignatureScheme, SigningKey, VerifyingKey};

const ALG: SigId = SigId::MlDsa65;
const ROOT_SEED: u64 = 4242; // deterministic KMS root — the SAME across "restarts"
const KEY_SEED: u64 = 77; //   deterministic audit signing key

// A small deterministic RNG (same construction the envelope test uses).
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

fn scratch(tag: &str) -> PathBuf {
    let pid = std::process::id();
    let ptr = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!("kms-ledger-{tag}-{pid}-{ptr}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A KMS sealer built from a deterministic software trust root — regenerating it
/// with the SAME seed models the same KMS identity being available after restart.
fn sealer() -> KmsSealer<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root =
        SoftwareTrustRoot::generate(&provider, "audit-root", &mut SeedRng(ROOT_SEED)).unwrap();
    KmsSealer::new(Kms::new(provider, root))
}

fn audit_keypair() -> (VerifyingKey, SigningKey) {
    FipsSig::new(ALG).keypair(&mut SeedRng(KEY_SEED)).unwrap()
}

fn record(i: usize) -> AuditRecord {
    AuditRecord {
        id: RecordId(format!("rec-{i}")),
        actor: Actor(format!("officer-{i}")),
        action: Action("PERMIT breakglass-grant".into()),
        resource: Resource(format!("vault/{i}")),
        ts: Timestamp("2026-07-24T00:00:00Z".into()),
    }
}

fn verify(entries: &[audit_writer::SealedEntry], provider: &CryptoProvider, vk: &VerifyingKey) {
    let vk = vk.clone();
    verify_chain(entries, provider, &LedgerId("gov-audit".into()), &|_alg| {
        Some(vk.clone())
    })
    .expect("chain re-verifies after restart");
}

#[test]
fn ledger_survives_restart_recovers_and_reverifies() {
    let dir = scratch("ok");
    let path = dir.join("audit.ledger");
    let (vk, sk) = audit_keypair();
    let ledger_id = LedgerId("gov-audit".into());

    // --- session 1: append N entries, then drop the process (writer) ---
    const N: usize = 8;
    {
        let ledger =
            FileLedger::open_or_create(&path, Box::new(sealer()), ledger_id.clone()).unwrap();
        let mut writer = AuditWriter::new(
            ledger,
            Box::new(FipsSig::new(ALG)),
            sk,
            vk.clone(),
            ledger_id.clone(),
        );
        for i in 0..N {
            let seq = writer.append(record(i)).unwrap();
            assert_eq!(seq, i as u64);
        }
        assert_eq!(writer.entries().len(), N);
    } // writer + FileLedger dropped => file closed. Simulated restart.

    // Entries are ENCRYPTED AT REST: no plaintext field leaks into the file.
    let on_disk = std::fs::read(&path).unwrap();
    for needle in [
        b"officer-0".as_slice(),
        b"PERMIT breakglass-grant",
        b"vault/3",
    ] {
        assert!(
            !on_disk.windows(needle.len()).any(|w| w == needle),
            "plaintext {:?} must not appear on disk",
            String::from_utf8_lossy(needle)
        );
    }

    // --- session 2: reopen; every entry is recovered and the chain re-verifies ---
    let reopened = FileLedger::open(&path, Box::new(sealer()), ledger_id.clone()).unwrap();
    use audit_writer::LedgerStore;
    assert_eq!(reopened.entries().len(), N, "all prior entries recovered");
    for (i, e) in reopened.entries().iter().enumerate() {
        assert_eq!(e.seq, i as u64);
        assert_eq!(e.record, record(i), "entry {i} recovered byte-for-byte");
    }
    verify(reopened.entries(), &fips_provider(), &vk);

    // --- session 3: appends continue from the recovered head ---
    {
        let (_vk2, sk2) = audit_keypair(); // same deterministic key
        let mut writer = AuditWriter::new(
            reopened,
            Box::new(FipsSig::new(ALG)),
            sk2,
            vk.clone(),
            ledger_id.clone(),
        );
        let seq = writer.append(record(99)).unwrap();
        assert_eq!(seq, N as u64, "sequence continues past the restart");
    }
    let final_ledger = FileLedger::open(&path, Box::new(sealer()), ledger_id).unwrap();
    assert_eq!(final_ledger.entries().len(), N + 1);
    verify(final_ledger.entries(), &fips_provider(), &vk);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tampered_on_disk_entry_is_detected_on_load() {
    let dir = scratch("tamper");
    let path = dir.join("audit.ledger");
    let (vk, sk) = audit_keypair();
    let ledger_id = LedgerId("gov-audit".into());

    {
        let ledger =
            FileLedger::open_or_create(&path, Box::new(sealer()), ledger_id.clone()).unwrap();
        let mut writer = AuditWriter::new(
            ledger,
            Box::new(FipsSig::new(ALG)),
            sk,
            vk,
            ledger_id.clone(),
        );
        for i in 0..4 {
            writer.append(record(i)).unwrap();
        }
    }

    // Flip a byte inside the sealed file — the AEAD authentication must fail, so
    // reopening the ledger fails CLOSED rather than surfacing forged content.
    let mut bytes = std::fs::read(&path).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x01;
    std::fs::write(&path, &bytes).unwrap();

    let reopened = FileLedger::open(&path, Box::new(sealer()), ledger_id);
    assert!(
        reopened.is_err(),
        "a tampered on-disk entry must be detected and fail closed on load"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
