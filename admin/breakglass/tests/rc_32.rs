//! RC-32 — Given a break-glass grant, when issued, then a real-time alert is
//! delivered AND a time-boxed capability actually elevates the grantee's access,
//! expiring automatically and audited throughout. [FR-AD-8] [RC-32]

mod common;

use admin_breakglass::{
    AlertKind, BreakGlassController, BreakGlassError, BreakGlassService, BreakGlassState,
    DurableAlerter, GrantRequest, SecurityOfficerBus,
};
use admin_identity::{AdminSession, IdentityDirectory, ROLE_IAM_ADMIN, ROLE_SECURITY_OFFICER};
use audit_writer::{verify_chain, AuditWriter, InMemoryLedger, LedgerId};
use common::{directory, Admins, APPROVAL_MAX_AGE};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::registry::CryptoProvider;
use pqc_agility::{Rng, SigId, SignatureScheme, SigningKey, VerifyingKey};

const ALG: SigId = SigId::MlDsa65;
const NOW: u64 = 1_000;
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

type Alerts = DurableAlerter<SecurityOfficerBus>;
type Controller = BreakGlassController<Alerts, InMemoryLedger, BreakGlassService<Alerts>>;

fn controller(bus: SecurityOfficerBus) -> Controller {
    let (vk, sk) = keypair(7);
    let audit = AuditWriter::new(
        InMemoryLedger::new(),
        Box::new(FipsSig::new(ALG)),
        sk,
        vk,
        LedgerId("admin-breakglass".into()),
    );
    BreakGlassController::new(BreakGlassService::new(DurableAlerter::new(bus)), audit)
}

fn request(id: &str) -> GrantRequest {
    GrantRequest {
        id: id.into(),
        principal: "responder".into(),
        scope: "kms".into(),
        justification: "prod incident: root key custody unreachable".into(),
        ttl_secs: TTL,
    }
}

/// The responder and the security officer, each signed in with their own passkey.
fn sessions() -> (AdminSession, AdminSession) {
    let mut admins = Admins::new(&["responder", "security-officer"]);
    (
        admins.sign_in("responder", NOW),
        admins.sign_in("security-officer", NOW),
    )
}

/// The responder may request break-glass; the security officer approves it.
fn roles() -> IdentityDirectory {
    directory(&[
        ("responder", ROLE_IAM_ADMIN),
        ("security-officer", ROLE_SECURITY_OFFICER),
    ])
}

fn chain_ok(c: &Controller) -> bool {
    let vk = c.verifying_key().clone();
    verify_chain(c.audit_entries(), &provider(), c.ledger_id(), &|alg| {
        (alg == ALG).then(|| vk.clone())
    })
    .is_ok()
}

#[test]
fn grant_delivers_a_real_alert_and_elevates_access_which_auto_expires() {
    let mut c = controller(SecurityOfficerBus::new());
    let (responder, officer) = sessions();
    let dir = roles();

    // Before any grant, the responder is NOT elevated for the emergency capability.
    assert!(!c.elevates("responder", "kms", "kms.key.rotate", NOW));

    // Grant break-glass elevating kms.key.rotate within the kms scope.
    let token = c
        .grant(
            &dir,
            request("bg-1"),
            &responder,
            &officer,
            APPROVAL_MAX_AGE,
            "kms.key.rotate",
            NOW,
            "t0",
        )
        .expect("grant opens");
    assert_eq!(token.capability, "kms.key.rotate");
    assert_eq!(token.expires_at, NOW + TTL);

    // A real-time alert was DELIVERED to the security-officer bus (not just recorded).
    let alerter = c.service().alerter();
    assert_eq!(alerter.delivered(), 1);
    assert_eq!(alerter.pending(), 0);
    let received = alerter.bus().received();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].kind, AlertKind::Granted);
    assert_eq!(received[0].grant_id, "bg-1");

    // The capability ACTUALLY elevates access, within scope, during the time-box.
    assert!(c.elevates("responder", "kms", "kms.key.rotate", NOW));
    assert!(c.elevates("responder", "kms", "kms.key.rotate", NOW + TTL - 1));
    // But not for a different capability, scope, or principal.
    assert!(!c.elevates("responder", "kms", "kms.key.delete", NOW));
    assert!(!c.elevates("responder", "bifrost", "kms.key.rotate", NOW));
    assert!(!c.elevates("someone-else", "kms", "kms.key.rotate", NOW));

    // At/after the time-box, the elevation AUTO-EXPIRES — access lapses on its own.
    assert!(!c.elevates("responder", "kms", "kms.key.rotate", NOW + TTL));
    assert!(!c.elevates("responder", "kms", "kms.key.rotate", NOW + TTL + 1_000));

    // Sweeping after expiry raises the ExpiredUnreviewed alert and revokes the token.
    let expired = c.sweep(NOW + TTL, "t1").unwrap();
    assert_eq!(expired, vec!["bg-1".to_string()]);
    assert_eq!(c.service().alerter().bus().received().len(), 2);
    assert_eq!(
        c.service().alerter().bus().received()[1].kind,
        AlertKind::ExpiredUnreviewed
    );

    // Audited throughout: approve → grant → expired, sealed and chain-verifiable,
    // with both people's authenticated identities. [MG-SEC-07]
    let entries = c.audit_entries();
    assert_eq!(entries[0].record.action.0, "breakglass-approve");
    assert_eq!(entries[0].record.actor.0, "security-officer");
    assert_eq!(entries[1].record.action.0, "breakglass-grant");
    assert_eq!(entries[1].record.actor.0, "responder");
    assert_eq!(entries[2].record.action.0, "breakglass-expired");
    assert!(chain_ok(&c));

    // The mandatory post-hoc review is also audited.
    c.review(
        "bg-1",
        "security-officer",
        "confirmed legitimate",
        NOW + TTL + 10,
        "t2",
    )
    .unwrap();
    assert_eq!(c.audit_entries()[3].record.action.0, "breakglass-review");
    assert!(chain_ok(&c));
}

#[test]
fn alert_delivery_is_durable_across_a_transient_bus_outage() {
    // The bus fails the first delivery attempt (transient outage).
    let mut bus = SecurityOfficerBus::new();
    bus.fail_next(1);
    let mut c = controller(bus);
    let (responder, officer) = sessions();
    let dir = roles();

    // Granting still succeeds (the grant is not lost), but the alert is pending.
    c.grant(
        &dir,
        request("bg-1"),
        &responder,
        &officer,
        APPROVAL_MAX_AGE,
        "kms.key.rotate",
        NOW,
        "t0",
    )
    .unwrap();
    assert_eq!(
        c.service().alerter().delivered(),
        0,
        "first delivery failed"
    );
    assert_eq!(
        c.service().alerter().pending(),
        1,
        "alert retained, not dropped"
    );
    assert!(c.service().alerter().bus().received().is_empty());

    // The elevation is live regardless — access is not blocked on alert delivery.
    assert!(c.elevates("responder", "kms", "kms.key.rotate", NOW));

    // A flush retries the pending alert; the bus is healthy now → delivered.
    let delivered = c.alerter_mut().flush();
    assert_eq!(delivered, 1);
    assert_eq!(c.service().alerter().pending(), 0);
    assert_eq!(c.service().alerter().bus().received().len(), 1);
}

/// Open `bg-1` for the responder, elevating kms.key.rotate.
fn open_grant(c: &mut Controller) {
    let (responder, officer) = sessions();
    c.grant(
        &roles(),
        request("bg-1"),
        &responder,
        &officer,
        APPROVAL_MAX_AGE,
        "kms.key.rotate",
        NOW,
        "t0",
    )
    .expect("grant opens");
    assert!(c.elevates("responder", "kms", "kms.key.rotate", NOW));
}

/// A grant reviewed inside its time-box stops elevating at once, not at expiry, and
/// the review is sealed. [FR-AD-8] [RC-32] [VLD-ANB-MIDGARD-0055]
#[test]
fn review_revokes_the_capability_immediately() {
    let mut c = controller(SecurityOfficerBus::new());
    open_grant(&mut c);

    c.review("bg-1", "security-officer", "stand down", NOW + 10, "t1")
        .unwrap();

    // Well before NOW + TTL, the reviewed grant no longer elevates.
    assert!(!c.elevates("responder", "kms", "kms.key.rotate", NOW + 10));
    assert!(!c.elevates("responder", "kms", "kms.key.rotate", NOW + 11));
    assert!(c.authority().token("bg-1").is_none());
    assert_eq!(c.authority().active_count(NOW + 10), 0);

    let last = c.audit_entries().last().unwrap();
    assert_eq!(last.record.action.0, "breakglass-review");
    assert_eq!(last.record.actor.0, "security-officer");
    assert!(chain_ok(&c));
}

/// Closing a grant early revokes its capability, seals a `breakglass-closed` entry
/// naming who closed it, and leaves the grant awaiting its post-hoc review.
/// [FR-AD-8] [AC-AD-8] [RC-32] [VLD-ANB-MIDGARD-0055]
#[test]
fn close_revokes_and_audits() {
    let mut c = controller(SecurityOfficerBus::new());
    open_grant(&mut c);

    c.close("bg-1", "responder", NOW + 20, "t1").unwrap();

    assert!(!c.elevates("responder", "kms", "kms.key.rotate", NOW + 20));
    assert!(c.authority().token("bg-1").is_none());
    assert!(!c.service().is_active("bg-1", NOW + 20));
    assert_eq!(
        c.service().state("bg-1", NOW + 20),
        Some(BreakGlassState::Expired)
    );
    assert_eq!(c.service().pending_reviews(NOW + 20).len(), 1);

    let last = c.audit_entries().last().unwrap();
    assert_eq!(last.record.action.0, "breakglass-closed");
    assert_eq!(last.record.actor.0, "responder");
    assert_eq!(last.record.resource.0, "bg-1");
    assert!(chain_ok(&c));

    // The review still has to happen, and it clears the backlog.
    c.review("bg-1", "security-officer", "closed early", NOW + 30, "t2")
        .unwrap();
    assert!(c.service().all_reviewed(NOW + 30));

    // A reviewed grant cannot be closed, and an unknown one is refused.
    assert_eq!(
        c.close("bg-1", "responder", NOW + 40, "t3").unwrap_err(),
        BreakGlassError::AlreadyReviewed("bg-1".into())
    );
    assert_eq!(
        c.close("nope", "responder", NOW + 40, "t3").unwrap_err(),
        BreakGlassError::UnknownGrant("nope".into())
    );
}
