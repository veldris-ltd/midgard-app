//! RC-31 — Given a high-risk config change, when committed, then it is refused unless
//! simulated, dual-approved with a distinct approver, and audited as one linked
//! transaction. Every mutating admin action goes through the guarded orchestrator.
//! [FR-AD-5] [FR-AD-6] [FR-AD-7] [RC-31]
#![cfg(feature = "backend-fips")]

use admin_config::{ConfigState, ConfigStore};
use admin_identity::{
    AdminAuthenticator, AdminChallenge, AdminId, AdminSession, IdentityDirectory, ResourceScope,
    RoleBinding, RoleCatalog, SoftwareAdminAuthenticator, ROLE_IAM_ADMIN, ROLE_SECURITY_OFFICER,
};
use admin_orchestrator::{GuardedActionOrchestrator, OrchestratorError};
use admin_privilege::{
    ActionClass, ActionState, AdminActionRequest, Disposition, PrivError, PrivilegeService,
};
use audit_writer::{verify_chain, AuditWriter, InMemoryLedger, LedgerId};
use policy::PolicySet;
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::registry::CryptoProvider;
use pqc_agility::{Rng, SigId, SignatureScheme, SigningKey, VerifyingKey};

const ALG: SigId = SigId::MlDsa65;
const NOW: u64 = 1_000;
const STEP_UP_MAX_AGE: u64 = 300;
const REAUTH_MAX_AGE: u64 = 120;
const TTL: u64 = 300;

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

/// A privilege service over an in-memory PQ-signed decision ledger.
fn privilege() -> PrivilegeService<InMemoryLedger> {
    let (vk, sk) = keypair(7);
    let writer = AuditWriter::new(
        InMemoryLedger::new(),
        Box::new(FipsSig::new(ALG)),
        sk,
        vk,
        LedgerId("admin-privilege".into()),
    );
    PrivilegeService::new(writer)
}

fn config_store() -> ConfigStore {
    let (vk, sk) = keypair(11);
    ConfigStore::new(Box::new(FipsSig::new(ALG)), sk, vk)
}

fn orchestrator() -> GuardedActionOrchestrator<InMemoryLedger> {
    GuardedActionOrchestrator::new(privilege(), config_store(), TTL)
}

/// A real WebAuthn-gated session for `admin`, stepped up + freshly re-authed at NOW.
fn ready_session(admin: &str, scope: &ResourceScope) -> AdminSession {
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
        justification: "rotate the enclosure root key".into(),
    }
}

fn empty_policy() -> PolicySet {
    PolicySet::new(vec![]).unwrap()
}

fn new_state(key: &str, value: &str) -> ConfigState {
    let mut s = ConfigState::new();
    s.set(key, value);
    s
}

fn chain_ok(orc: &GuardedActionOrchestrator<InMemoryLedger>) -> bool {
    let vk = orc.privilege().verifying_key().clone();
    verify_chain(
        orc.privilege().audit_entries(),
        &provider(),
        orc.privilege().ledger_id(),
        &|alg| if alg == ALG { Some(vk.clone()) } else { None },
    )
    .is_ok()
}

// --- RC-31 -------------------------------------------------------------------

#[test]
fn single_author_high_risk_commit_is_denied() {
    let kms = ResourceScope::all("kms");
    let mut dir = IdentityDirectory::new(RoleCatalog::standard());
    bind(&mut dir, "alice", ROLE_SECURITY_OFFICER, &kms);

    let mut orc = orchestrator();
    let alice = ready_session("alice", &kms);

    // Submit a high-risk (KeyRotation, risk 4) change → parked, NO commit token.
    let sub = orc
        .submit(
            &dir,
            &alice,
            request("act-1", "alice", ActionClass::KeyRotation, &kms),
            &empty_policy(),
            &empty_policy(),
            &[],
            NOW,
            STEP_UP_MAX_AGE,
            "t0",
        )
        .unwrap();
    assert_eq!(sub.disposition, Disposition::NeedsApproval);
    assert!(
        sub.token.is_none(),
        "no commit token for an unapproved high-risk action"
    );
    // It was simulated as part of the transaction.
    assert!(orc.simulation("act-1").is_some());

    // The requester cannot approve their own action (separation of duties) — so they
    // can never obtain a commit token single-handedly.
    let err = orc
        .approve(&dir, &alice, "act-1", NOW, REAUTH_MAX_AGE, "t1")
        .unwrap_err();
    assert_eq!(
        err,
        OrchestratorError::Privilege(PrivError::SelfApprovalDenied)
    );

    // The action is not authorised, and nothing was committed.
    assert!(!orc.privilege().is_authorised("act-1"));
    assert!(orc.config().head().is_none(), "no config commit occurred");
}

#[test]
fn dual_approved_commit_is_audited_as_one_linked_transaction() {
    let kms = ResourceScope::all("kms");
    let mut dir = IdentityDirectory::new(RoleCatalog::standard());
    bind(&mut dir, "alice", ROLE_SECURITY_OFFICER, &kms);
    bind(&mut dir, "bob", ROLE_SECURITY_OFFICER, &kms);

    let mut orc = orchestrator();
    let alice = ready_session("alice", &kms);
    let bob = ready_session("bob", &kms);

    // 1. authenticate + submit + simulate.
    let sub = orc
        .submit(
            &dir,
            &alice,
            request("act-1", "alice", ActionClass::KeyRotation, &kms),
            &empty_policy(),
            &empty_policy(),
            &[],
            NOW,
            STEP_UP_MAX_AGE,
            "t0",
        )
        .unwrap();
    assert_eq!(sub.disposition, Disposition::NeedsApproval);

    // 2. a DISTINCT, qualified approver approves → single-use commit token.
    let token = orc
        .approve(&dir, &bob, "act-1", NOW, REAUTH_MAX_AGE, "t1")
        .expect("distinct approver approves");
    assert!(orc.privilege().is_authorised("act-1"));

    // 3. commit with the token → the config change lands, linked + audited.
    let commit = orc
        .commit(
            &token,
            new_state("kms.root", "v2"),
            "rotate root",
            NOW,
            "t2",
        )
        .expect("authorised + simulated + tokened commit");
    assert_eq!(commit.version, 0);
    assert_eq!(commit.author, "alice");
    assert_eq!(commit.state.get("kms.root"), Some(&"v2".to_string()));
    assert_eq!(
        orc.config().head().unwrap().content_hash,
        commit.content_hash
    );

    // ONE linked transaction: request → approve → commit, all in the same chain.
    let rec = orc.privilege().record("act-1").unwrap();
    assert_eq!(rec.audit_seqs, vec![0, 1, 2]);
    let entries = orc.privilege().audit_entries();
    assert_eq!(entries[0].record.action.0, "request-action");
    assert_eq!(entries[1].record.action.0, "approve-action");
    assert_eq!(entries[1].record.actor.0, "bob");
    assert_eq!(entries[2].record.action.0, "commit-config:v0");
    assert!(chain_ok(&orc), "the linked audit chain re-verifies");
}

#[test]
fn commit_token_is_single_use() {
    let kms = ResourceScope::all("kms");
    let mut dir = IdentityDirectory::new(RoleCatalog::standard());
    bind(&mut dir, "alice", ROLE_SECURITY_OFFICER, &kms);
    bind(&mut dir, "bob", ROLE_SECURITY_OFFICER, &kms);

    let mut orc = orchestrator();
    orc.submit(
        &dir,
        &ready_session("alice", &kms),
        request("act-1", "alice", ActionClass::KeyRotation, &kms),
        &empty_policy(),
        &empty_policy(),
        &[],
        NOW,
        STEP_UP_MAX_AGE,
        "t0",
    )
    .unwrap();
    let token = orc
        .approve(
            &dir,
            &ready_session("bob", &kms),
            "act-1",
            NOW,
            REAUTH_MAX_AGE,
            "t1",
        )
        .unwrap();

    // First commit succeeds; a replay of the same token is refused (single-use).
    orc.commit(&token, new_state("k", "1"), "m", NOW, "t2")
        .unwrap();
    assert_eq!(
        orc.commit(&token, new_state("k", "2"), "m", NOW, "t3")
            .unwrap_err(),
        OrchestratorError::TokenReused
    );
}

#[test]
fn commit_token_is_time_boxed() {
    let kms = ResourceScope::all("kms");
    let mut dir = IdentityDirectory::new(RoleCatalog::standard());
    bind(&mut dir, "alice", ROLE_SECURITY_OFFICER, &kms);
    bind(&mut dir, "bob", ROLE_SECURITY_OFFICER, &kms);

    let mut orc = orchestrator();
    orc.submit(
        &dir,
        &ready_session("alice", &kms),
        request("act-1", "alice", ActionClass::KeyRotation, &kms),
        &empty_policy(),
        &empty_policy(),
        &[],
        NOW,
        STEP_UP_MAX_AGE,
        "t0",
    )
    .unwrap();
    let token = orc
        .approve(
            &dir,
            &ready_session("bob", &kms),
            "act-1",
            NOW,
            REAUTH_MAX_AGE,
            "t1",
        )
        .unwrap();

    // Committing past the token's time-box is refused.
    assert_eq!(
        orc.commit(&token, new_state("k", "1"), "m", NOW + TTL + 1, "t2")
            .unwrap_err(),
        OrchestratorError::TokenExpired
    );
    assert!(
        orc.config().head().is_none(),
        "expired token committed nothing"
    );
}

#[test]
fn low_risk_action_flows_through_single_actor_and_is_audited() {
    let cfg = ResourceScope::all("config");
    let mut dir = IdentityDirectory::new(RoleCatalog::standard());
    bind(&mut dir, "alice", ROLE_IAM_ADMIN, &cfg);

    let mut orc = orchestrator();
    // ConfigLabelEdit is risk 2 (single actor) → authorised on submit, token issued.
    let sub = orc
        .submit(
            &dir,
            &ready_session("alice", &cfg),
            request("act-1", "alice", ActionClass::ConfigLabelEdit, &cfg),
            &empty_policy(),
            &empty_policy(),
            &[],
            NOW,
            STEP_UP_MAX_AGE,
            "t0",
        )
        .unwrap();
    assert_eq!(sub.disposition, Disposition::Authorised);
    let token = sub
        .token
        .expect("single-actor action is tokened immediately");

    // It commits and is audited (request → commit), no approver needed.
    let commit = orc
        .commit(&token, new_state("label", "SECRET"), "relabel", NOW, "t1")
        .unwrap();
    assert_eq!(commit.version, 0);
    let rec = orc.privilege().record("act-1").unwrap();
    assert!(matches!(
        rec.state,
        ActionState::Authorised { approver: None }
    ));
    // A single-actor action audits request → authorise on submit, then commit here.
    assert_eq!(rec.audit_seqs, vec![0, 1, 2]);
    let entries = orc.privilege().audit_entries();
    assert_eq!(entries[0].record.action.0, "request-action");
    assert_eq!(entries[2].record.action.0, "commit-config:v0");
    assert!(chain_ok(&orc));
}
