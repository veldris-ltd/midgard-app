//! Enrolling a principal is a high-risk action: an IAM administrator can request it,
//! but it is parked until a second, distinct IAM administrator approves; the requester
//! cannot approve their own, an administrator without the IAM user capability cannot
//! approve it, the parked request survives a restart, and every decision is in the
//! PQ-signed chain. [RC-29] [FR-AD-5] [AC-AD-3] [AC-AD-4]
#![cfg(feature = "backend-fips")]

use std::path::PathBuf;

use admin_identity::{
    AdminAuthenticator, AdminChallenge, AdminId, AdminSession, IdentityDirectory, ResourceScope,
    RoleBinding, RoleCatalog, SoftwareAdminAuthenticator, ROLE_AUDITOR, ROLE_IAM_ADMIN,
};
use admin_privilege::{
    ActionClass, ActionState, AdminActionRequest, Disposition, DurablePrivilege, PrivError,
};
use audit_writer::{verify_chain, LedgerId};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{Rng, SigId, SignatureScheme, VerifyingKey};

const ALG: SigId = SigId::MlDsa65;
const STEP_UP_MAX_AGE: u64 = 300;
const REAUTH_MAX_AGE: u64 = 120;
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
    let d = std::env::temp_dir().join(format!("principal-enroll-{}-{}", std::process::id(), uniq));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sealer(seed: u64) -> Box<KmsSealer<SoftwareTrustRoot>> {
    let p = fips_provider();
    let root = SoftwareTrustRoot::generate(&p, "privilege-root", &mut SeedRng(seed)).unwrap();
    Box::new(KmsSealer::new(Kms::new(p, root)))
}

fn open(dir: &PathBuf) -> DurablePrivilege {
    let (vk, sk) = FipsSig::new(ALG).keypair(&mut SeedRng(7)).unwrap();
    DurablePrivilege::open(
        dir,
        sealer(9),
        sealer(10),
        Box::new(FipsSig::new(ALG)),
        sk,
        vk,
        LedgerId("admin-privilege".into()),
    )
    .unwrap()
}

fn audit_key() -> VerifyingKey {
    FipsSig::new(ALG).keypair(&mut SeedRng(7)).unwrap().0
}

/// A WebAuthn-gated session stepped up into `scope` and freshly re-authenticated.
fn ready_session(admin: &str, scope: &ResourceScope) -> AdminSession {
    let device = SoftwareAdminAuthenticator::enrol(&fips_provider(), "k", &mut SeedRng(3)).unwrap();
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
    for (admin, role) in [
        ("alice", ROLE_IAM_ADMIN),
        ("bob", ROLE_IAM_ADMIN),
        ("audra", ROLE_AUDITOR),
    ] {
        dir.bind(RoleBinding {
            admin: AdminId(admin.into()),
            role: role.into(),
            scope: scope.clone(),
            not_before: 0,
            not_after: 1_000_000,
        });
    }
    dir
}

#[test]
fn enrolling_a_principal_needs_a_second_iam_administrator() {
    assert!(ActionClass::PrincipalEnroll.is_high_risk());
    let iam = ResourceScope::all("iam");
    let dir = directory(&iam);
    let path = scratch();

    {
        let mut svc = open(&path);
        let alice = ready_session("alice", &iam);
        let disposition = svc
            .submit(
                &dir,
                &alice,
                AdminActionRequest {
                    id: "enrol-olivia".into(),
                    requester: AdminId("alice".into()),
                    action_class: ActionClass::PrincipalEnroll,
                    scope: iam.clone(),
                    justification: "new operator".into(),
                },
                NOW,
                STEP_UP_MAX_AGE,
                "t0",
            )
            .unwrap();
        assert_eq!(disposition, Disposition::NeedsApproval);

        // The requester cannot approve their own enrolment.
        assert_eq!(
            svc.approve(
                &dir,
                &ready_session("alice", &iam),
                "enrol-olivia",
                NOW,
                REAUTH_MAX_AGE,
                "t1"
            ),
            Err(PrivError::SelfApprovalDenied)
        );
        // An auditor, however authenticated, cannot approve it either.
        assert!(matches!(
            svc.approve(
                &dir,
                &ready_session("audra", &iam),
                "enrol-olivia",
                NOW,
                REAUTH_MAX_AGE,
                "t2"
            ),
            Err(PrivError::ApproverNotQualified { .. })
        ));
    } // restart

    let mut svc = open(&path);
    assert_eq!(
        svc.service().state_of("enrol-olivia"),
        Some(&ActionState::PendingApproval),
        "the parked request survives a restart"
    );
    assert_eq!(
        svc.service()
            .record("enrol-olivia")
            .map(|e| e.request.action_class),
        Some(ActionClass::PrincipalEnroll)
    );

    svc.approve(
        &dir,
        &ready_session("bob", &iam),
        "enrol-olivia",
        NOW,
        REAUTH_MAX_AGE,
        "t3",
    )
    .expect("a distinct IAM administrator approves");
    assert_eq!(
        svc.service().state_of("enrol-olivia"),
        Some(&ActionState::Authorised {
            approver: Some(AdminId("bob".into()))
        })
    );

    let vk = audit_key();
    verify_chain(
        svc.audit_entries(),
        &fips_provider(),
        svc.ledger_id(),
        &|alg| (alg == ALG).then(|| vk.clone()),
    )
    .expect("request and approval chain across the restart");

    let _ = std::fs::remove_dir_all(&path);
}
