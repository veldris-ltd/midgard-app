//! AC-M-4: role/marking/purpose is enforced and every decision is audited with a
//! PQ signature; the append-only ledger verifies and detects tampering.
//! [NFR-SEC-2] [NFR-PQ-3]

use std::collections::BTreeSet;

use audit_writer::{verify_chain, AuditError};
use governance::{GovContext, Governance, GOV_SIG_ALG};
use policy::{
    Action, ClassificationLevel, Clearance, Compartment, DefaultPdp, Effect, Marking, PolicySet,
    Purpose, Resource, ResourceId, Role, Rule, RuleId, Selector,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{Rng, SigId, VerifyingKey};

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

fn ctx(subject: &str, role: &str, level: ClassificationLevel, purpose: &str) -> GovContext {
    GovContext {
        subject: subject.into(),
        roles: [Role(role.into())].into_iter().collect(),
        clearance: Clearance {
            level,
            compartments: BTreeSet::new(),
        },
        purpose: Purpose(purpose.into()),
    }
}

fn marked_resource(id: &str, level: ClassificationLevel, purpose: &str) -> Resource {
    Resource {
        id: ResourceId(id.into()),
        marking: Marking {
            level,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose(purpose.into())].into_iter().collect(),
    }
}

fn verify(gov: &Governance) -> Result<(), AuditError> {
    let provider = fips_provider();
    let vk = gov.audit_verifying_key().clone();
    let ledger = gov.audit_ledger_id().clone();
    let resolver =
        move |alg: SigId| -> Option<VerifyingKey> { (alg == GOV_SIG_ALG).then(|| vk.clone()) };
    verify_chain(gov.audit_entries(), &provider, &ledger, &resolver)
}

#[test]
fn permit_on_marked_data_is_authorized_and_audited() {
    let mut gov = Governance::new(analyst_read_pdp(), &mut SeedRng(1)).unwrap();
    let alice = ctx(
        "alice",
        "analyst",
        ClassificationLevel::SECRET,
        "investigation",
    );
    let doc = marked_resource("doc:1", ClassificationLevel::CONFIDENTIAL, "investigation");

    let d = gov.authorize(&alice, &doc, "Read", 1000).unwrap();
    assert!(d.permitted);
    // The decision produced exactly one PQ-signed audit entry.
    assert_eq!(gov.audit_entries().len(), 1);
    verify(&gov).expect("audit chain verifies under the governance key");
}

#[test]
fn denials_are_also_audited() {
    let mut gov = Governance::new(analyst_read_pdp(), &mut SeedRng(2)).unwrap();

    // No read-up: a CONFIDENTIAL subject cannot read SECRET data.
    let low = ctx(
        "bob",
        "analyst",
        ClassificationLevel::CONFIDENTIAL,
        "investigation",
    );
    let secret = marked_resource("doc:2", ClassificationLevel::SECRET, "investigation");
    let d1 = gov.authorize(&low, &secret, "Read", 2000).unwrap();
    assert!(!d1.permitted);

    // Wrong purpose is denied.
    let wp = ctx("carol", "analyst", ClassificationLevel::SECRET, "audit");
    let doc = marked_resource("doc:3", ClassificationLevel::UNCLASSIFIED, "investigation");
    let d2 = gov.authorize(&wp, &doc, "Read", 2001).unwrap();
    assert!(!d2.permitted);

    // Missing role is denied (deny-by-default).
    let guest = ctx(
        "mallory",
        "guest",
        ClassificationLevel::SECRET,
        "investigation",
    );
    let doc2 = marked_resource("doc:4", ClassificationLevel::UNCLASSIFIED, "investigation");
    let d3 = gov.authorize(&guest, &doc2, "Read", 2002).unwrap();
    assert!(!d3.permitted);

    // All three denials are on the immutable ledger and it verifies.
    assert_eq!(gov.audit_entries().len(), 3);
    verify(&gov).expect("chain verifies");
}

#[test]
fn compartment_need_to_know_is_enforced() {
    let mut gov = Governance::new(analyst_read_pdp(), &mut SeedRng(3)).unwrap();
    let alice = ctx(
        "alice",
        "analyst",
        ClassificationLevel::SECRET,
        "investigation",
    );
    // Resource requires the CREST compartment, which the subject lacks.
    let mut compartments = BTreeSet::new();
    compartments.insert(Compartment("CREST".into()));
    let doc = Resource {
        id: ResourceId("doc:5".into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments,
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    };
    let d = gov.authorize(&alice, &doc, "Read", 3000).unwrap();
    assert!(!d.permitted, "missing compartment must deny");
    verify(&gov).unwrap();
}

#[test]
fn tampering_the_ledger_is_detected() {
    let mut gov = Governance::new(analyst_read_pdp(), &mut SeedRng(4)).unwrap();
    let alice = ctx(
        "alice",
        "analyst",
        ClassificationLevel::SECRET,
        "investigation",
    );
    let doc = marked_resource("doc:6", ClassificationLevel::UNCLASSIFIED, "investigation");
    gov.authorize(&alice, &doc, "Read", 4000).unwrap();
    gov.authorize(&alice, &doc, "Read", 4001).unwrap();

    // Verify with the WRONG key → signatures don't validate (tamper-evidence).
    let provider = fips_provider();
    let (wrong_vk, _sk) = provider
        .sig_for(GOV_SIG_ALG)
        .unwrap()
        .keypair(&mut SeedRng(9999))
        .unwrap();
    let ledger = gov.audit_ledger_id().clone();
    let resolver = move |alg: SigId| -> Option<VerifyingKey> {
        (alg == GOV_SIG_ALG).then(|| wrong_vk.clone())
    };
    assert!(verify_chain(gov.audit_entries(), &provider, &ledger, &resolver).is_err());
}

#[test]
fn permits_does_not_audit() {
    // The non-auditing fast path leaves the ledger untouched (used for per-object
    // filters where the query-level authorize already recorded the access).
    let mut gov = Governance::new(analyst_read_pdp(), &mut SeedRng(5)).unwrap();
    let alice = ctx(
        "alice",
        "analyst",
        ClassificationLevel::SECRET,
        "investigation",
    );
    let doc = marked_resource("doc:7", ClassificationLevel::UNCLASSIFIED, "investigation");
    assert!(gov.permits(&alice, &doc, "Read"));
    assert_eq!(gov.audit_entries().len(), 0);
    // And an audited call then records exactly one entry.
    gov.authorize(&alice, &doc, "Read", 5000).unwrap();
    assert_eq!(gov.audit_entries().len(), 1);
}
