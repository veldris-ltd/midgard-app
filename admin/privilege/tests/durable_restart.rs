//! RC-14 (privilege): the PQ-signed decision ledger AND in-flight dual-control
//! state survive a restart — recovered and re-verified. [FR-AD-5] [RC-14]
#![cfg(feature = "backend-fips")]

use std::path::PathBuf;

use admin_identity::{
    AdminAuthenticator, AdminChallenge, AdminId, AdminSession, IdentityDirectory, ResourceScope,
    RoleBinding, RoleCatalog, SoftwareAdminAuthenticator, ROLE_SECURITY_OFFICER,
};
use admin_privilege::{
    ActionClass, ActionState, AdminActionRequest, Disposition, DurablePrivilege,
};
use audit_writer::{verify_chain, LedgerId};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{CryptoProvider, Rng, SigId, SignatureScheme, VerifyingKey};

const ALG: SigId = SigId::MlDsa65;
const NOW: u64 = 1_000;

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
    let d = std::env::temp_dir().join(format!("admin-priv-{}-{}", std::process::id(), uniq));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sealer() -> KmsSealer<SoftwareTrustRoot> {
    let p = fips_provider();
    let root = SoftwareTrustRoot::generate(&p, "priv-root", &mut SeedRng(9)).unwrap();
    KmsSealer::new(Kms::new(p, root))
}

fn open(dir: &PathBuf) -> DurablePrivilege {
    let (vk, sk) = FipsSig::new(ALG).keypair(&mut SeedRng(500)).unwrap();
    DurablePrivilege::open(
        dir,
        Box::new(sealer()),
        Box::new(sealer()),
        Box::new(FipsSig::new(ALG)),
        sk,
        vk,
        LedgerId("admin-privilege".into()),
    )
    .unwrap()
}

fn provider() -> CryptoProvider {
    let mut p = CryptoProvider::new();
    p.register_sig(Box::new(FipsSig::new(ALG)));
    p
}

fn verify(store: &DurablePrivilege) {
    let vk: VerifyingKey = store.verifying_key().clone();
    verify_chain(
        store.audit_entries(),
        &provider(),
        store.ledger_id(),
        &|a| (a == ALG).then(|| vk.clone()),
    )
    .expect("privilege audit chain re-verifies after restart");
}

fn ready_session(admin: &str, scope: &ResourceScope) -> AdminSession {
    // A real WebAuthn-gated session (enrol → sign in → step up → re-auth). [RC-30]
    let device = SoftwareAdminAuthenticator::enrol(&fips_provider(), "k", &mut SeedRng(7)).unwrap();
    let mut authr = AdminAuthenticator::new(fips_provider());
    authr.register(AdminId(admin.into()), device.credential().clone());
    let sign = |seed: u64| {
        let ch = AdminChallenge::sample(&mut SeedRng(seed));
        let asr = device.authenticate(authr.provider(), &ch, true).unwrap();
        (ch, asr)
    };
    let (ch, asr) = sign(1);
    let mut s = authr
        .sign_in(&AdminId(admin.into()), &ch, &asr, NOW)
        .unwrap();
    let (ch2, asr2) = sign(2);
    authr
        .step_up(&mut s, scope.clone(), &ch2, &asr2, NOW)
        .unwrap();
    let (ch3, asr3) = sign(3);
    authr.reauth(&mut s, &ch3, &asr3, NOW).unwrap();
    s
}

fn directory(scope: &ResourceScope) -> IdentityDirectory {
    let mut dir = IdentityDirectory::new(RoleCatalog::standard());
    for a in ["alice", "bob"] {
        dir.bind(RoleBinding {
            admin: AdminId(a.into()),
            role: ROLE_SECURITY_OFFICER.into(),
            scope: scope.clone(),
            not_before: 0,
            not_after: 1_000_000,
        });
    }
    dir
}

#[test]
fn privilege_ledger_and_inflight_state_survive_restart() {
    let dir = scratch();
    let kms = ResourceScope::all("kms");
    let idir = directory(&kms);

    // --- session 1: submit a high-risk action; it parks awaiting a 2nd approver ---
    {
        let mut svc = open(&dir);
        let req = AdminActionRequest {
            id: "act-1".into(),
            requester: AdminId("alice".into()),
            action_class: ActionClass::KeyRotation,
            scope: kms.clone(),
            justification: "rotate the release key".into(),
        };
        let disp = svc
            .submit(&idir, &ready_session("alice", &kms), req, NOW, 300, "t0")
            .unwrap();
        assert_eq!(disp, Disposition::NeedsApproval);
        assert_eq!(
            svc.service().state_of("act-1"),
            Some(&ActionState::PendingApproval)
        );
    }

    // --- restart: pending dual-control action + audit lineage recovered ---
    let mut svc = open(&dir);
    assert_eq!(
        svc.service().state_of("act-1"),
        Some(&ActionState::PendingApproval),
        "parked high-risk action recovered after restart"
    );
    verify(&svc);

    // A distinct second officer approves post-restart; the chain continues.
    svc.approve(&idir, &ready_session("bob", &kms), "act-1", NOW, 120, "t1")
        .unwrap();
    assert!(matches!(
        svc.service().state_of("act-1"),
        Some(ActionState::Authorised { .. })
    ));
    verify(&svc);

    // --- restart again: the approval persisted too ---
    let svc = open(&dir);
    assert!(matches!(
        svc.service().state_of("act-1"),
        Some(ActionState::Authorised { .. })
    ));
    verify(&svc);

    let _ = std::fs::remove_dir_all(&dir);
}
