//! MG-SEC-07 — break-glass needs a real second person. The approver is an
//! administrator who signed in with their own passkey, is not the requester, and
//! re-authenticated recently; a typed name, the requester again, or an approver who
//! cannot authenticate is refused, and a valid grant seals both authenticated
//! identities into the audit ledger. [MG-SEC-07] [FR-AD-8]
//!
//! There is no "approver" field to type into any more: `GrantRequest` names only the
//! principal, and the approver recorded is read from the approving session.
//!
//! Authenticated is not entitled: the requester must hold `breakglass.request` and
//! the approver `breakglass.approve` (the security officer's) through role bindings
//! in the identity directory that are in force and cover the grant's scope.

mod common;

use admin_breakglass::{
    AlertKind, BreakGlassController, BreakGlassError, BreakGlassService, GrantRequest,
    RecordingAlerter,
};
use admin_identity::{
    IdentityDirectory, IdentityError, ResourceScope, RoleCatalog, Subsystem, ROLE_AUDITOR,
    ROLE_IAM_ADMIN, ROLE_SECURITY_OFFICER,
};
use audit_writer::{verify_chain, AuditWriter, InMemoryLedger, LedgerId};
use common::{bind, directory, Admins, SeedRng, APPROVAL_MAX_AGE};
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

/// `mallory` may request break-glass and `secoff` is the security officer.
fn roles() -> IdentityDirectory {
    directory(&[
        ("mallory", ROLE_IAM_ADMIN),
        ("secoff", ROLE_SECURITY_OFFICER),
    ])
}

/// Grant through the controller against `roles()`; `Err` leaves no trace (asserted
/// by callers).
fn grant(
    c: &mut Controller,
    req: GrantRequest,
    requester: &admin_identity::AdminSession,
    approver: &admin_identity::AdminSession,
) -> Result<(), BreakGlassError> {
    grant_in(&roles(), c, req, requester, approver)
}

/// Grant through the controller against the given directory.
fn grant_in(
    dir: &IdentityDirectory,
    c: &mut Controller,
    req: GrantRequest,
    requester: &admin_identity::AdminSession,
    approver: &admin_identity::AdminSession,
) -> Result<(), BreakGlassError> {
    c.grant(
        dir,
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

// --- Authenticated is not entitled: the role bindings decide. [MG-SEC-07] --------

#[test]
fn an_approver_without_breakglass_approve_is_refused() {
    let mut admins = Admins::new(&["mallory", "secoff"]);
    let mallory = admins.sign_in("mallory", NOW);
    let secoff = admins.sign_in("secoff", NOW);

    // A fellow requester-grade admin (IAM-Admin holds `breakglass.request`, not
    // `breakglass.approve`) cannot be the second person.
    let peer = directory(&[("mallory", ROLE_IAM_ADMIN), ("secoff", ROLE_IAM_ADMIN)]);
    let (mut c, _) = controller();
    assert_eq!(
        grant_in(&peer, &mut c, request("mallory"), &mallory, &secoff).unwrap_err(),
        BreakGlassError::ApproverNotAuthorised
    );
    nothing_happened(&c);

    // Nor can an enrolled admin with no role binding at all.
    let unbound = directory(&[("mallory", ROLE_IAM_ADMIN)]);
    assert_eq!(
        grant_in(&unbound, &mut c, request("mallory"), &mallory, &secoff).unwrap_err(),
        BreakGlassError::ApproverNotAuthorised
    );
    nothing_happened(&c);
}

#[test]
fn a_requester_without_breakglass_request_is_refused() {
    let mut admins = Admins::new(&["mallory", "secoff", "officer2"]);
    let mallory = admins.sign_in("mallory", NOW);
    let secoff = admins.sign_in("secoff", NOW);
    let officer2 = admins.sign_in("officer2", NOW);
    let (mut c, _) = controller();

    // The read-only auditor holds no `breakglass.request`.
    let auditor = directory(&[("mallory", ROLE_AUDITOR), ("secoff", ROLE_SECURITY_OFFICER)]);
    assert_eq!(
        grant_in(&auditor, &mut c, request("mallory"), &mallory, &secoff).unwrap_err(),
        BreakGlassError::RequesterNotAuthorised
    );
    nothing_happened(&c);

    // Nor does a security officer: the approving role cannot elevate itself, even
    // with another officer's approval.
    let officers = directory(&[
        ("secoff", ROLE_SECURITY_OFFICER),
        ("officer2", ROLE_SECURITY_OFFICER),
    ]);
    assert_eq!(
        grant_in(&officers, &mut c, request("secoff"), &secoff, &officer2).unwrap_err(),
        BreakGlassError::RequesterNotAuthorised
    );
    assert!(c.audit_entries().is_empty(), "nothing sealed");
    assert_eq!(c.service().alerter().count(AlertKind::Granted), 0);
    assert!(!c.elevates("secoff", "kms", "kms.key.rotate", NOW));
}

#[test]
fn a_security_officer_approving_another_admins_request_succeeds() {
    let mut admins = Admins::new(&["responder", "secoff"]);
    let responder = admins.sign_in("responder", NOW);
    let secoff = admins.sign_in("secoff", NOW);
    let dir = directory(&[
        ("responder", ROLE_IAM_ADMIN),
        ("secoff", ROLE_SECURITY_OFFICER),
    ]);
    let (mut c, _) = controller();
    grant_in(&dir, &mut c, request("responder"), &responder, &secoff)
        .expect("an entitled requester and a security-officer approver");

    assert_eq!(c.service().get("bg-1").unwrap().approver, "secoff");
    assert!(c.elevates("responder", "kms", "kms.key.rotate", NOW));
    assert_eq!(c.service().alerter().count(AlertKind::Granted), 1);
    let e = c.audit_entries();
    assert_eq!(
        (e[0].record.action.0.as_str(), e[0].record.actor.0.as_str()),
        ("breakglass-approve", "secoff")
    );
    assert_eq!(
        (e[1].record.action.0.as_str(), e[1].record.actor.0.as_str()),
        ("breakglass-grant", "responder")
    );
}

#[test]
fn bindings_count_only_in_their_scope_and_validity_window() {
    let mut admins = Admins::new(&["mallory", "secoff"]);
    let mallory = admins.sign_in("mallory", NOW);
    let secoff = admins.sign_in("secoff", NOW);

    // `mallory` may request over all of kms; `secoff` approves only as far as the
    // binding under test allows.
    let with_officer = |scope: ResourceScope, not_before: u64, not_after: u64| {
        let mut dir = IdentityDirectory::new(RoleCatalog::standard());
        bind(
            &mut dir,
            "mallory",
            ROLE_IAM_ADMIN,
            ResourceScope::all("kms"),
            0,
            u64::MAX,
        );
        bind(
            &mut dir,
            "secoff",
            ROLE_SECURITY_OFFICER,
            scope,
            not_before,
            not_after,
        );
        dir
    };
    let refused = |dir: &IdentityDirectory, req: GrantRequest| {
        let (mut c, _) = controller();
        let err = grant_in(dir, &mut c, req, &mallory, &secoff).unwrap_err();
        assert!(c.audit_entries().is_empty(), "nothing sealed");
        assert_eq!(c.service().alerter().count(AlertKind::Granted), 0);
        err
    };

    // Another subsystem's officer cannot approve kms break-glass.
    let elsewhere = with_officer(ResourceScope::all("bifrost"), 0, u64::MAX);
    assert_eq!(
        refused(&elsewhere, request("mallory")),
        BreakGlassError::ApproverNotAuthorised
    );

    // An expired binding, or one not yet in force, grants nothing.
    let expired = with_officer(ResourceScope::all("kms"), 0, NOW);
    assert_eq!(
        refused(&expired, request("mallory")),
        BreakGlassError::ApproverNotAuthorised
    );
    let future = with_officer(ResourceScope::all("kms"), NOW + 1, u64::MAX);
    assert_eq!(
        refused(&future, request("mallory")),
        BreakGlassError::ApproverNotAuthorised
    );

    // A binding confined to one key cannot approve elevation over the whole
    // subsystem, but can over that key.
    let one_key = with_officer(
        ResourceScope {
            subsystem: Subsystem("kms".into()),
            selector: "key-a".into(),
        },
        0,
        u64::MAX,
    );
    assert_eq!(
        refused(&one_key, request("mallory")),
        BreakGlassError::ApproverNotAuthorised
    );
    let key_a = GrantRequest {
        scope: "kms/key-a".into(),
        ..request("mallory")
    };
    let (mut c, _) = controller();
    grant_in(&one_key, &mut c, key_a, &mallory, &secoff).expect("in the officer's scope");
    assert!(c.elevates("mallory", "kms/key-a", "kms.key.rotate", NOW));

    // The requester's binding is held to the same window.
    let mut lapsed = IdentityDirectory::new(RoleCatalog::standard());
    bind(
        &mut lapsed,
        "mallory",
        ROLE_IAM_ADMIN,
        ResourceScope::all("kms"),
        0,
        NOW,
    );
    bind(
        &mut lapsed,
        "secoff",
        ROLE_SECURITY_OFFICER,
        ResourceScope::all("kms"),
        0,
        u64::MAX,
    );
    assert_eq!(
        refused(&lapsed, request("mallory")),
        BreakGlassError::RequesterNotAuthorised
    );
}
