//! AC-AD-3, AC-AD-4, AC-AD-13 — the dual-control guard.
//!
//! AC-AD-3   a high-risk action cannot execute without a second, distinct approver.
//! AC-AD-4   a requester cannot approve their own action (separation of duties).
//! AC-AD-13  a change to the panel's own privilege logic requires a security-officer
//!           approver.
//! Every decision is sealed into the real FIPS-backed PQ-signed audit chain and the
//! chain is verified. `[FR-AD-5]`
#![cfg(feature = "backend-fips")]

use admin_identity::{
    AdminAuthenticator, AdminChallenge, AdminId, AdminSession, IdentityDirectory, ResourceScope,
    RoleBinding, RoleCatalog, SoftwareAdminAuthenticator, ROLE_IAM_ADMIN, ROLE_SECURITY_OFFICER,
};
use admin_privilege::{
    ActionClass, ActionState, AdminActionRequest, Disposition, PrivError, PrivilegeService,
};
use audit_writer::{verify_chain, AuditWriter, InMemoryLedger, LedgerId};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::registry::CryptoProvider;
use pqc_agility::{Rng, SigId, SignatureScheme, SigningKey, VerifyingKey};

const ALG: SigId = SigId::MlDsa65;
const STEP_UP_MAX_AGE: u64 = 300;
const REAUTH_MAX_AGE: u64 = 120;
const NOW: u64 = 1_000;

// --- deterministic FIPS keying (mirrors the heimdall/workflow harness) --------

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

fn keypair(seed: u64) -> (VerifyingKey, SigningKey) {
    FipsSig::new(ALG).keypair(&mut SeedRng(seed)).unwrap()
}

fn provider() -> CryptoProvider {
    let mut p = CryptoProvider::new();
    p.register_sig(Box::new(FipsSig::new(ALG)));
    p
}

fn service() -> (PrivilegeService<InMemoryLedger>, VerifyingKey) {
    let (vk, sk) = keypair(7);
    let writer = AuditWriter::new(
        InMemoryLedger::new(),
        Box::new(FipsSig::new(ALG)),
        sk,
        vk.clone(),
        LedgerId("admin-privilege".into()),
    );
    (PrivilegeService::new(writer), vk)
}

fn chain_ok(svc: &PrivilegeService<InMemoryLedger>, vk: &VerifyingKey) -> bool {
    let vk = vk.clone();
    verify_chain(svc.audit_entries(), &provider(), svc.ledger_id(), &|alg| {
        if alg == ALG {
            Some(vk.clone())
        } else {
            None
        }
    })
    .is_ok()
}

// --- identity fixtures --------------------------------------------------------

/// A fully-authenticated session for `admin`, stepped up into `scope` and freshly
/// re-authenticated at `NOW`.
fn ready_session(admin: &str, scope: &ResourceScope) -> AdminSession {
    // A real WebAuthn-gated session: enrol a passkey, sign in with a user-verified
    // assertion, step up into the scope, and re-auth — all verified. [RC-30]
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

fn bind(dir: &mut IdentityDirectory, admin: &str, role: &str, scope: &ResourceScope) {
    dir.bind(RoleBinding {
        admin: AdminId(admin.into()),
        role: role.into(),
        scope: scope.clone(),
        not_before: 0,
        not_after: 1_000_000,
    });
}

fn request(
    id: &str,
    requester: &str,
    class: ActionClass,
    scope: &ResourceScope,
) -> AdminActionRequest {
    AdminActionRequest {
        id: id.into(),
        requester: AdminId(requester.into()),
        action_class: class,
        scope: scope.clone(),
        justification: "test".into(),
    }
}

// --- AC-AD-3 ------------------------------------------------------------------

#[test]
fn ac_ad_3_high_risk_action_needs_a_second_distinct_approver() {
    let kms = ResourceScope::all("kms");
    let mut dir = IdentityDirectory::new(RoleCatalog::standard());
    // Two distinct security officers, both authorised for KMS.
    bind(&mut dir, "alice", ROLE_SECURITY_OFFICER, &kms);
    bind(&mut dir, "bob", ROLE_SECURITY_OFFICER, &kms);

    let (mut svc, vk) = service();
    let req = request("act-1", "alice", ActionClass::KeyRotation, &kms);
    let alice = ready_session("alice", &kms);

    // Submitting a high-risk action parks it — it is NOT yet authorised.
    let disp = svc
        .submit(&dir, &alice, req, NOW, STEP_UP_MAX_AGE, "t0")
        .unwrap();
    assert_eq!(disp, Disposition::NeedsApproval);
    assert_eq!(svc.state_of("act-1"), Some(&ActionState::PendingApproval));
    assert!(!svc.is_authorised("act-1"));

    // A distinct second security officer approves; only now is it authorised.
    let bob = ready_session("bob", &kms);
    svc.approve(&dir, &bob, "act-1", NOW, REAUTH_MAX_AGE, "t1")
        .unwrap();
    assert!(svc.is_authorised("act-1"));
    assert_eq!(
        svc.state_of("act-1"),
        Some(&ActionState::Authorised {
            approver: Some(AdminId("bob".into()))
        })
    );

    // Lineage: request → approve, both sealed and chain-verifiable.
    let rec = svc.record("act-1").unwrap();
    assert_eq!(rec.audit_seqs, vec![0, 1]);
    assert_eq!(svc.audit_entries()[0].record.action.0, "request-action");
    assert_eq!(svc.audit_entries()[1].record.action.0, "approve-action");
    assert_eq!(svc.audit_entries()[1].record.actor.0, "bob");
    assert!(chain_ok(&svc, &vk));
}

#[test]
fn ac_ad_3_low_risk_action_is_authorised_without_a_second_approver() {
    let cfg = ResourceScope::all("config");
    let mut dir = IdentityDirectory::new(RoleCatalog::standard());
    bind(&mut dir, "alice", ROLE_IAM_ADMIN, &cfg);

    let (mut svc, vk) = service();
    let req = request("cfg-1", "alice", ActionClass::ConfigLabelEdit, &cfg);
    let alice = ready_session("alice", &cfg);
    let disp = svc
        .submit(&dir, &alice, req, NOW, STEP_UP_MAX_AGE, "t0")
        .unwrap();
    assert_eq!(disp, Disposition::Authorised);
    assert_eq!(
        svc.state_of("cfg-1"),
        Some(&ActionState::Authorised { approver: None })
    );
    assert!(chain_ok(&svc, &vk));
}

// --- AC-AD-4 ------------------------------------------------------------------

#[test]
fn ac_ad_4_self_approval_is_denied_by_separation_of_duties() {
    let kms = ResourceScope::all("kms");
    let mut dir = IdentityDirectory::new(RoleCatalog::standard());
    bind(&mut dir, "alice", ROLE_SECURITY_OFFICER, &kms);

    let (mut svc, vk) = service();
    let req = request("act-2", "alice", ActionClass::KeyRotation, &kms);
    let alice = ready_session("alice", &kms);
    svc.submit(&dir, &alice, req, NOW, STEP_UP_MAX_AGE, "t0")
        .unwrap();

    // The requester tries to approve their own action → denied, still pending.
    let alice_again = ready_session("alice", &kms);
    assert_eq!(
        svc.approve(&dir, &alice_again, "act-2", NOW, REAUTH_MAX_AGE, "t1"),
        Err(PrivError::SelfApprovalDenied)
    );
    assert_eq!(svc.state_of("act-2"), Some(&ActionState::PendingApproval));
    assert!(!svc.is_authorised("act-2"));
    // Only the request was audited; the self-approval never sealed an entry.
    assert_eq!(svc.audit_entries().len(), 1);
    assert!(chain_ok(&svc, &vk));
}

// --- AC-AD-13 -----------------------------------------------------------------

#[test]
fn ac_ad_13_panel_privilege_change_requires_a_security_officer_approver() {
    let panel = ResourceScope::all("panel");
    let mut dir = IdentityDirectory::new(RoleCatalog::standard());
    // Requester and one qualified approver are security officers on the panel;
    // dave is an IAM-Admin — an admin, but NOT a security officer.
    bind(&mut dir, "alice", ROLE_SECURITY_OFFICER, &panel);
    bind(&mut dir, "bob", ROLE_SECURITY_OFFICER, &panel);
    bind(&mut dir, "dave", ROLE_IAM_ADMIN, &panel);

    let (mut svc, vk) = service();
    let req = request("priv-1", "alice", ActionClass::PanelPrivilegeChange, &panel);
    let alice = ready_session("alice", &panel);
    let disp = svc
        .submit(&dir, &alice, req, NOW, STEP_UP_MAX_AGE, "t0")
        .unwrap();
    assert_eq!(disp, Disposition::NeedsApproval);

    // A non-security-officer approver is refused, even distinct and fully authed.
    let dave = ready_session("dave", &panel);
    assert_eq!(
        svc.approve(&dir, &dave, "priv-1", NOW, REAUTH_MAX_AGE, "t1"),
        Err(PrivError::SecurityOfficerRequired)
    );
    assert!(!svc.is_authorised("priv-1"));

    // A distinct security officer can approve it.
    let bob = ready_session("bob", &panel);
    svc.approve(&dir, &bob, "priv-1", NOW, REAUTH_MAX_AGE, "t2")
        .unwrap();
    assert!(svc.is_authorised("priv-1"));
    assert!(chain_ok(&svc, &vk));
}

#[test]
fn ac_ad_13_panel_privilege_change_still_blocks_self_approval() {
    let panel = ResourceScope::all("panel");
    let mut dir = IdentityDirectory::new(RoleCatalog::standard());
    bind(&mut dir, "alice", ROLE_SECURITY_OFFICER, &panel);

    let (mut svc, _vk) = service();
    let req = request("priv-2", "alice", ActionClass::PanelPrivilegeChange, &panel);
    let alice = ready_session("alice", &panel);
    svc.submit(&dir, &alice, req, NOW, STEP_UP_MAX_AGE, "t0")
        .unwrap();

    // Even a security officer cannot approve their OWN panel privilege change.
    let alice_again = ready_session("alice", &panel);
    assert_eq!(
        svc.approve(&dir, &alice_again, "priv-2", NOW, REAUTH_MAX_AGE, "t1"),
        Err(PrivError::SelfApprovalDenied)
    );
}
