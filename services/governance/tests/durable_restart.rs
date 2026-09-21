//! RC-09 (governance repoint): governance lineage persists across a restart when
//! wired to a durable, KMS-sealed `FileLedger`. Every `authorize` decision is
//! written to disk, encrypted at rest; after a restart the trail is recovered and
//! the PQ-signed hash chain re-verifies. [AC-M-4] [RC-09]

use std::collections::BTreeSet;
use std::path::PathBuf;

use audit_writer::{verify_chain, AuditWriter, FileLedger, LedgerId, LedgerStore};
use governance::{GovContext, Governance, GOV_SIG_ALG};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use policy::{
    Action, ClassificationLevel, Clearance, DefaultPdp, Effect, Marking, PolicySet, Purpose,
    Resource, ResourceId, Role, Rule, RuleId, Selector,
};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{Rng, SigId, SignatureScheme, VerifyingKey};

const LEDGER: &str = "gov-durable";

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

fn scratch() -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!("gov-durable-{}-{}", std::process::id(), uniq));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn analyst_read_pdp() -> DefaultPdp {
    DefaultPdp::new(
        PolicySet::new(vec![Rule {
            id: RuleId("read".into()),
            role: Role("analyst".into()),
            action: Action("Read".into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        }])
        .unwrap(),
    )
}

fn ctx() -> GovContext {
    GovContext {
        subject: "alice".into(),
        roles: [Role("analyst".into())].into_iter().collect(),
        clearance: Clearance {
            level: ClassificationLevel::SECRET,
            compartments: BTreeSet::new(),
        },
        purpose: Purpose("investigation".into()),
    }
}

fn doc(i: usize) -> Resource {
    Resource {
        id: ResourceId(format!("doc:{i}")),
        marking: Marking {
            level: ClassificationLevel::CONFIDENTIAL,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

/// A KMS sealer from a deterministic software root — regenerating with the same
/// seed models the same KMS identity being present after a restart.
fn sealer() -> KmsSealer<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, "gov-root", &mut SeedRng(9)).unwrap();
    KmsSealer::new(Kms::new(provider, root))
}

fn open_ledger(path: &PathBuf) -> FileLedger {
    FileLedger::open(path, Box::new(sealer()), LedgerId(LEDGER.into())).unwrap()
}

#[test]
fn governance_lineage_persists_across_restart() {
    let dir = scratch();
    let path = dir.join("governance.ledger");
    // Deterministic governance audit signing key (stable across the restart).
    let (vk, sk) = FipsSig::new(GOV_SIG_ALG)
        .keypair(&mut SeedRng(500))
        .unwrap();

    // --- session 1: run governance against a durable ledger, audit 5 decisions ---
    const N: usize = 5;
    {
        let writer = AuditWriter::new(
            open_ledger(&path),
            Box::new(FipsSig::new(GOV_SIG_ALG)),
            sk,
            vk.clone(),
            LedgerId(LEDGER.into()),
        );
        let mut gov = Governance::with_audit(analyst_read_pdp(), writer);
        for i in 0..N {
            let d = gov
                .authorize(&ctx(), &doc(i), "Read", 1000 + i as u64)
                .unwrap();
            assert!(
                d.permitted,
                "analyst may read CONFIDENTIAL for the declared purpose"
            );
        }
        assert_eq!(gov.audit_entries().len(), N);
    } // governance + writer + FileLedger dropped: simulated process exit.

    // Encrypted at rest: the subject/action never appear as plaintext on disk.
    let on_disk = std::fs::read(&path).unwrap();
    for needle in [b"alice".as_slice(), b"PERMIT Read", b"doc:2"] {
        assert!(
            !on_disk.windows(needle.len()).any(|w| w == needle),
            "plaintext {:?} must not be on disk",
            String::from_utf8_lossy(needle)
        );
    }

    // --- restart: reopen the durable ledger, recover + re-verify every entry ---
    let reopened = open_ledger(&path);
    assert_eq!(
        reopened.entries().len(),
        N,
        "governance lineage recovered after restart"
    );
    let vk_for = {
        let vk = vk.clone();
        move |alg: SigId| -> Option<VerifyingKey> { (alg == GOV_SIG_ALG).then(|| vk.clone()) }
    };
    verify_chain(
        reopened.entries(),
        &fips_provider(),
        &LedgerId(LEDGER.into()),
        &vk_for,
    )
    .expect("recovered governance chain re-verifies under the audit key");

    let _ = std::fs::remove_dir_all(&dir);
}
