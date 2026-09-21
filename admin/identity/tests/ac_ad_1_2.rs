//! AC-AD-1 and AC-AD-2 — the RBAC and authentication invariants of the panel.
//!
//! AC-AD-2  no single role grants all capabilities; the Auditor can mutate nothing.
//! AC-AD-1  sign-in requires MFA; entering an admin scope requires step-up re-auth.
//! Plus: bindings are scoped and expiring. `[FR-AD-2]`

use admin_identity::{
    AdminAuthenticator, AdminChallenge, AdminId, IdentityDirectory, IdentityError, ResourceScope,
    RoleBinding, RoleCatalog, SoftwareAdminAuthenticator, WebAuthnAssertion, ROLE_AUDITOR,
    ROLE_SECURITY_OFFICER,
};
use pqc_agility::backend::fips::provider as fips_provider;
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

// --- AC-AD-2: role separation -------------------------------------------------

#[test]
fn ac_ad_2_no_role_grants_all_capabilities() {
    let catalog = RoleCatalog::standard();
    // The universe is the union of every role's capabilities; no single role
    // holds all of it — there is no all-powerful role.
    assert!(!catalog.any_role_all_powerful());

    let universe = catalog.capability_universe();
    for r in catalog.roles() {
        assert_ne!(
            r.capabilities, universe,
            "role {} must not hold the whole capability universe",
            r.name
        );
        assert!(
            r.capabilities.len() < universe.len(),
            "role {} holds a strict subset of the universe",
            r.name
        );
    }
}

#[test]
fn ac_ad_2_auditor_is_strictly_read_only() {
    let catalog = RoleCatalog::standard();
    let auditor = catalog.get(ROLE_AUDITOR).expect("auditor role exists");
    assert!(auditor.read_only, "Auditor must be flagged read-only");
    assert!(
        !auditor.has_mutating_capability(),
        "Auditor must hold no mutating capability"
    );
    // A mutating capability (owned by another role) is genuinely absent.
    assert!(!catalog
        .get(ROLE_SECURITY_OFFICER)
        .unwrap()
        .capabilities
        .is_empty());
    let so = catalog.get(ROLE_SECURITY_OFFICER).unwrap();
    assert!(so.has_mutating_capability(), "Security-Officer does mutate");
}

// --- AC-AD-2: scoped, expiring bindings --------------------------------------

#[test]
fn ac_ad_2_bindings_are_scoped_and_expiring() {
    let mut dir = IdentityDirectory::new(RoleCatalog::standard());
    let kms = ResourceScope::all("kms");
    let bifrost = ResourceScope::all("bifrost");
    dir.bind(RoleBinding {
        admin: AdminId("alice".into()),
        role: ROLE_SECURITY_OFFICER.into(),
        scope: kms.clone(),
        not_before: 100,
        not_after: 200,
    });

    // In-window and in-scope: the capability resolves.
    assert!(dir.has_capability(&AdminId("alice".into()), "kms.key.rotate", &kms, 150));
    // Out of scope (different subsystem): nothing is granted.
    assert!(!dir.has_capability(&AdminId("alice".into()), "kms.key.rotate", &bifrost, 150));
    // Before the window: not yet active.
    assert!(!dir.has_capability(&AdminId("alice".into()), "kms.key.rotate", &kms, 50));
    // At/after expiry (half-open window): expired.
    assert!(!dir.has_capability(&AdminId("alice".into()), "kms.key.rotate", &kms, 200));
    assert_eq!(dir.risk_ceiling(&AdminId("alice".into()), &kms, 150), 5);
    assert_eq!(dir.risk_ceiling(&AdminId("alice".into()), &kms, 250), 0);
}

// --- AC-AD-1: WebAuthn-gated sign-in + step-up on scope entry -----------------

/// An authenticator with `alice` registered, plus her passkey device.
fn setup() -> (AdminAuthenticator, SoftwareAdminAuthenticator) {
    let device = SoftwareAdminAuthenticator::enrol(&fips_provider(), "alice-key", &mut SeedRng(1))
        .expect("enrol passkey");
    let mut authr = AdminAuthenticator::new(fips_provider());
    authr.register(AdminId("alice".into()), device.credential().clone());
    (authr, device)
}

fn challenge(seed: u64) -> AdminChallenge {
    AdminChallenge::sample(&mut SeedRng(seed))
}

fn assert_for(
    authr: &AdminAuthenticator,
    device: &SoftwareAdminAuthenticator,
    ch: &AdminChallenge,
    user_verified: bool,
) -> WebAuthnAssertion {
    device
        .authenticate(authr.provider(), ch, user_verified)
        .expect("device signs")
}

#[test]
fn ac_ad_1_a_non_user_verified_or_unknown_admin_gets_no_session() {
    let (authr, device) = setup();
    let ch = challenge(10);

    // A verified assertion WITHOUT user verification is single-factor → no session.
    let no_uv = assert_for(&authr, &device, &ch, false);
    assert_eq!(
        authr
            .sign_in(&AdminId("alice".into()), &ch, &no_uv, 1000)
            .unwrap_err(),
        IdentityError::MfaRequired
    );

    // An unregistered admin cannot sign in at all.
    let uv = assert_for(&authr, &device, &ch, true);
    assert_eq!(
        authr
            .sign_in(&AdminId("mallory".into()), &ch, &uv, 1000)
            .unwrap_err(),
        IdentityError::UnknownAdmin
    );
}

#[test]
fn ac_ad_1_mfa_sign_in_then_step_up_required_to_enter_scope() {
    let (authr, device) = setup();
    let kms = ResourceScope::all("kms");

    // A user-verified WebAuthn assertion signs in (MFA satisfied).
    let ch = challenge(1);
    let asr = assert_for(&authr, &device, &ch, true);
    let mut s = authr
        .sign_in(&AdminId("alice".into()), &ch, &asr, 1000)
        .expect("webauthn sign-in ok");
    assert!(s.has_mfa());

    // Before entering the scope, it is not authorised — step-up is required.
    assert_eq!(
        s.authorize_scope(&kms, 1000, 300),
        Err(IdentityError::StepUpRequired {
            scope: "kms/*".into()
        })
    );

    // A step-up whose assertion lacks user verification is refused.
    let ch2 = challenge(2);
    let weak = assert_for(&authr, &device, &ch2, false);
    assert_eq!(
        authr.step_up(&mut s, kms.clone(), &ch2, &weak, 1000),
        Err(IdentityError::WeakStepUpFactor)
    );

    // A fresh user-verified step-up succeeds; the scope is now authorised.
    let ch3 = challenge(3);
    let strong = assert_for(&authr, &device, &ch3, true);
    authr
        .step_up(&mut s, kms.clone(), &ch3, &strong, 1000)
        .expect("step-up ok");
    assert!(s.authorize_scope(&kms, 1000, 300).is_ok());

    // The step-up goes stale after its freshness window.
    assert_eq!(
        s.authorize_scope(&kms, 1400, 300),
        Err(IdentityError::StepUpStale {
            scope: "kms/*".into()
        })
    );
}

#[test]
fn ac_ad_1_fresh_reauth_tracks_the_latest_re_authentication() {
    let (authr, device) = setup();
    let ch = challenge(1);
    let asr = assert_for(&authr, &device, &ch, true);
    let mut s = authr
        .sign_in(&AdminId("alice".into()), &ch, &asr, 1000)
        .unwrap();

    // Sign-in is itself a fresh re-auth.
    assert!(s.has_fresh_reauth(1000, 120));
    // It ages out.
    assert!(!s.has_fresh_reauth(1200, 120));
    // A fresh re-auth (verified assertion) restores it.
    let ch2 = challenge(2);
    let asr2 = assert_for(&authr, &device, &ch2, true);
    authr.reauth(&mut s, &ch2, &asr2, 1200).unwrap();
    assert!(s.has_fresh_reauth(1200, 120));
}
