//! AC-AD-9 — when the panel administers a subsystem, it uses a distinct
//! least-privilege service identity, not a superuser credential. `[FR-AD-1]`
//! `[NFR-SEC-1]`

use std::collections::BTreeSet;

use admin_service_identity::{service_role, AdminServiceIdentities, SUBSYSTEMS};
use iam::Idp;
use pqc_agility::Rng;

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

#[test]
fn ac_ad_9_distinct_least_privilege_identity_per_subsystem() {
    let mut rng = SeedRng(42);
    let idp = Idp::generate("midgard-idp", &mut rng).unwrap();
    let ids = AdminServiceIdentities::issue(&idp, &mut rng).unwrap();
    let provider = idp.provider();

    // One identity per subsystem, all least-privilege (single narrow role, no root).
    assert_eq!(ids.identities().len(), SUBSYSTEMS.len());
    assert!(ids.is_least_privilege());

    // Each identity is authorised for its OWN subsystem...
    for sub in SUBSYSTEMS {
        let cert = ids.cert_for(sub).expect("identity for subsystem");
        assert!(
            ids.authorized_for(cert, sub, provider),
            "{sub} must accept its own identity"
        );
    }

    // ...and for no other. A KMS identity is rejected by Bifrost and IAM, even
    // though the cert itself is validly signed by the same CA — the least-privilege
    // boundary is the role, not merely cert validity.
    let kms = ids.cert_for("kms").unwrap();
    assert!(!ids.authorized_for(kms, "bifrost", provider));
    assert!(!ids.authorized_for(kms, "iam", provider));

    // No single identity is a superuser: each authorises exactly one subsystem.
    for id in ids.identities() {
        let accepted = SUBSYSTEMS
            .iter()
            .filter(|s| ids.authorized_for(&id.cert, s, provider))
            .count();
        assert_eq!(
            accepted, 1,
            "{} must administer exactly one subsystem",
            id.subsystem
        );
    }

    // Distinct service ids, distinct key material, and exactly the expected role.
    let mut service_ids = BTreeSet::new();
    let mut keys = BTreeSet::new();
    for id in ids.identities() {
        assert_eq!(id.cert.identity.roles.len(), 1);
        assert!(id
            .cert
            .identity
            .roles
            .contains(&service_role(&id.subsystem)));
        assert!(
            service_ids.insert(id.cert.identity.id.0.clone()),
            "service ids are distinct"
        );
        assert!(
            keys.insert(id.cert.identity.verifying_key.as_bytes().to_vec()),
            "key material is distinct per subsystem"
        );
    }
}

#[test]
fn ac_ad_9_no_superuser_role_is_present() {
    let mut rng = SeedRng(7);
    let idp = Idp::generate("midgard-idp", &mut rng).unwrap();
    let ids = AdminServiceIdentities::issue(&idp, &mut rng).unwrap();

    // None of the minted roles is a root/superuser/wildcard role.
    for id in ids.identities() {
        for role in &id.cert.identity.roles {
            let n = role.0.to_lowercase();
            assert!(!n.contains("root"));
            assert!(!n.contains("superuser"));
            assert!(n != "*");
        }
    }
}
