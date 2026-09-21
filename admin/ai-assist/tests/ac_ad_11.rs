//! AC-AD-11 — the admin copilot suggests an action but cannot execute it; the
//! action still traverses the guarded (dual-control) path. `[FR-AD-10]` `[NFR-AI-1]`
//!
//! The copilot's output is advisory: a [`CopilotResponse`] can only express a
//! PROPOSED change — there is no execute method on it. To act, the proposal is
//! bridged into an [`AdminActionRequest`] and submitted to the real
//! [`PrivilegeService`] (Prompt 7.b), which parks the high-risk action for a second,
//! distinct approver. The AI never acts; the guarded path does.

use admin_ai_assist::{ev_ad1_advisory, request_copilot, CopilotContext, MockCopilot};
use admin_identity::{
    AdminAuthenticator, AdminChallenge, AdminId, AdminSession, IdentityDirectory, ResourceScope,
    RoleBinding, RoleCatalog, SoftwareAdminAuthenticator, ROLE_SECURITY_OFFICER,
};
use admin_privilege::{
    ActionClass, ActionState, AdminActionRequest, Disposition, PrivilegeService,
};
use audit_writer::{AuditWriter, InMemoryLedger, LedgerId};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::{Rng, SigId, SignatureScheme, SigningKey, VerifyingKey};

const ALG: SigId = SigId::MlDsa65;
const NOW: u64 = 1_000;

// The copilot, asked to 'rotate the root key now', returns a PROPOSED change that is
// high-risk and requires dual-control — never an execution.
const COPILOT_PROPOSAL: &str = r#"{
  "explanation": "Rotating the root key is a high-risk action requiring dual-control.",
  "citations": ["resource:kms/root-key"],
  "proposed_changes": [
    {"subsystem": "kms", "change": "rotate root key", "risk_level": 4,
     "requires_dual_control": true, "least_privilege_note": "scoped to the kms root key only",
     "suggested_expiry": null}
  ],
  "refusal": {"code": null, "detail": null}
}"#;

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

fn service() -> PrivilegeService<InMemoryLedger> {
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

#[test]
fn ac_ad_11_copilot_proposal_cannot_execute_and_traverses_the_guarded_path() {
    // 1. The copilot drafts an advisory proposal. It has no execute path — the type
    //    can only carry proposals — and its high-risk change requires dual-control.
    let ctx = CopilotContext::new(
        "Security-Officer",
        "STATE",
        "POLICY",
        "rotate the root key now",
    );
    let response = request_copilot(&ctx, &MockCopilot::new(COPILOT_PROPOSAL)).unwrap();
    assert!(ev_ad1_advisory(&response).passed());
    let proposal = &response.proposed_changes[0];
    assert!(proposal.requires_dual_control);
    assert_eq!(proposal.subsystem, "kms");

    // 2. To actually act, the proposal is bridged into the guarded path. It is NOT
    //    executed by the copilot; it is submitted to the dual-control service.
    let kms = ResourceScope::all("kms");
    let mut dir = IdentityDirectory::new(RoleCatalog::standard());
    dir.bind(RoleBinding {
        admin: AdminId("alice".into()),
        role: ROLE_SECURITY_OFFICER.into(),
        scope: kms.clone(),
        not_before: 0,
        not_after: 1_000_000,
    });
    dir.bind(RoleBinding {
        admin: AdminId("bob".into()),
        role: ROLE_SECURITY_OFFICER.into(),
        scope: kms.clone(),
        not_before: 0,
        not_after: 1_000_000,
    });

    let mut svc = service();
    let req = AdminActionRequest {
        id: "from-copilot-1".into(),
        requester: AdminId("alice".into()),
        action_class: ActionClass::KeyRotation, // the orchestrator maps the proposal → class
        scope: kms.clone(),
        justification: proposal.change.clone(),
    };
    let alice = ready_session("alice", &kms);
    let disp = svc.submit(&dir, &alice, req, NOW, 300, "t0").unwrap();

    // The copilot-suggested action is parked for a second approver — it did not
    // execute on the strength of the AI's suggestion.
    assert_eq!(disp, Disposition::NeedsApproval);
    assert_eq!(
        svc.state_of("from-copilot-1"),
        Some(&ActionState::PendingApproval)
    );
    assert!(!svc.is_authorised("from-copilot-1"));

    // Only a distinct, human second approver can authorise it.
    let bob = ready_session("bob", &kms);
    svc.approve(&dir, &bob, "from-copilot-1", NOW, 120, "t1")
        .unwrap();
    assert!(svc.is_authorised("from-copilot-1"));
}
