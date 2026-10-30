//! MG-SEC-07 — break-glass needs a real second person. The approver is an
//! administrator who signed in with their own passkey, is not the requester, and
//! re-authenticated recently; a typed name, the requester again, or an approver who
//! cannot authenticate is refused, and a valid grant seals both authenticated
//! identities into the audit ledger. [MG-SEC-07] [FR-AD-8]
//!
//! There is no "approver" field to type into any more: `GrantRequest` names only the
//! principal, and the approver recorded is read from the approving session.

mod common;

use admin_breakglass::{
    AlertKind, BreakGlassController, BreakGlassError, BreakGlassService, GrantRequest,
    RecordingAlerter,
};
use admin_identity::IdentityError;
use audit_writer::{verify_chain, AuditWriter, InMemoryLedger, LedgerId};
use common::{Admins, SeedRng, APPROVAL_MAX_AGE};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{SigId, SignatureScheme, VerifyingKey};

const ALG: SigId = SigId::MlDsa65;
const NOW: u64 = 10_000;

type Controller = BreakGlassController<RecordingAlerter, InMemoryLedger>;

fn controller() -> (Controller, VerifyingKey) {
    let (vk, sk) = FipsSig::new(ALG).keypair(&mut SeedRng(5)).unwrap();
    let audit = AuditWriter::new(
        InMemoryLedger::new(),
        Box::new(FipsSig::new(ALG)),
        sk,
        vk.clone(),
        LedgerId("admin-breakglass".into()),
    );
    (
        BreakGlassController::new(BreakGlassService::new(RecordingAlerter::new()), audit),
        vk,
    )
}

fn request(principal: &str) -> GrantRequest {
    GrantRequest {
        id: "bg-1".into(),
        principal: principal.into(),
        scope: "kms".into(),
        justification: "sev-1: root key custody unreachable".into(),
        ttl_secs: 600,
    }
}

/// Grant through the controller; `Err` leaves no trace (asserted by callers).
fn grant(
    c: &mut Controller,
    req: GrantRequest,
    requester: &admin_identity::AdminSession,
    approver: &admin_identity::AdminSession,
) -> Result<(), BreakGlassError> {
    c.grant(
        req,
        requester,
        approver,
        APPROVAL_MAX_AGE,
        "kms.key.rotate",
        NOW,
        "t0",
    )
    .map(|_| ())
}

fn nothing_happened(c: &Controller) {
    assert!(c.audit_entries().is_empty(), "nothing sealed");
    assert_eq!(c.service().alerter().count(AlertKind::Granted), 0);
    assert!(!c.elevates("mallory", "kms", "kms.key.rotate", NOW));
}

#[test]
fn a_typed_approver_name_is_not_an_approval() {
    let mut admins = Admins::new(&["mallory", "secoff"]);
    // Knowing the security officer's name buys nothing: without their passkey there
    // is no session in their name to approve with.
    assert_eq!(
        admins
            .try_sign_in("secoff", "mallory", true, NOW)
            .unwrap_err(),
        IdentityError::WebAuthnRejected
    );

    // Nor can the requester ask for elevation in someone else's name.
    let mallory = admins.sign_in("mallory", NOW);
    let secoff = admins.sign_in("secoff", NOW);
    let (mut c, _) = controller();
    assert_eq!(
        grant(&mut c, request("secoff"), &mallory, &secoff).unwrap_err(),
        BreakGlassError::SessionMismatch
    );
    nothing_happened(&c);
}

#[test]
fn the_requester_cannot_approve_their_own_break_glass() {
    let mut admins = Admins::new(&["mallory", "secoff"]);
    // Two separately authenticated sessions, one person.
    let first = admins.sign_in("mallory", NOW);
    let second = admins.sign_in("mallory", NOW);
    let (mut c, _) = controller();
    assert_eq!(
        grant(&mut c, request("mallory"), &first, &second).unwrap_err(),
        BreakGlassError::SelfApprovalDenied
    );
    nothing_happened(&c);
}

#[test]
fn an_unauthenticated_approver_is_refused() {
    let mut admins = Admins::new(&["mallory", "secoff"]);
    // Not an enrolled administrator.
    assert_eq!(
        admins
            .try_sign_in("ghost", "secoff", true, NOW)
            .unwrap_err(),
        IdentityError::UnknownAdmin
    );
    // The right passkey without user verification is one factor, not a sign-in.
    assert_eq!(
        admins
            .try_sign_in("secoff", "secoff", false, NOW)
            .unwrap_err(),
        IdentityError::MfaRequired
    );

    // A session the approver opened long ago is not an approval now: they must
    // re-authenticate with their passkey at the moment of approving.
    let mallory = admins.sign_in("mallory", NOW);
    let stale = admins.sign_in("secoff", NOW - APPROVAL_MAX_AGE);
    let (mut c, _) = controller();
    assert_eq!(
        grant(&mut c, request("mallory"), &mallory, &stale).unwrap_err(),
        BreakGlassError::StaleApproval
    );
    nothing_happened(&c);
}

#[test]
fn a_valid_two_person_grant_records_both_identities_in_the_ledger() {
    let mut admins = Admins::new(&["mallory", "secoff"]);
    let mallory = admins.sign_in("mallory", NOW);
    let secoff = admins.sign_in("secoff", NOW - 10);
    let (mut c, vk) = controller();
    grant(&mut c, request("mallory"), &mallory, &secoff).expect("two people, both signed in");

    let g = c.service().get("bg-1").unwrap();
    assert_eq!(g.principal, "mallory");
    assert_eq!(
        g.approver, "secoff",
        "the approver is the authenticated session"
    );
    assert!(c.elevates("mallory", "kms", "kms.key.rotate", NOW));

    // Both authenticated identities are sealed into the PQ-signed ledger.
    let e = c.audit_entries();
    assert_eq!(e.len(), 2);
    assert_eq!(
        (e[0].record.action.0.as_str(), e[0].record.actor.0.as_str()),
        ("breakglass-approve", "secoff")
    );
    assert_eq!(
        (e[1].record.action.0.as_str(), e[1].record.actor.0.as_str()),
        ("breakglass-grant", "mallory")
    );
    assert!(e.iter().all(|e| e.record.resource.0 == "bg-1"));
    verify_chain(e, &fips_provider(), c.ledger_id(), &|alg| {
        (alg == ALG).then(|| vk.clone())
    })
    .expect("the grant's entries verify");
}
