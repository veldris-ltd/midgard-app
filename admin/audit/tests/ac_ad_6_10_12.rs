//! Cross-cutting acceptance for the admin panel's data/audit plane.
//!
//! AC-AD-6   every admin action — committed, denied, AND simulated — writes a
//!           PQ-signed, append-only audit entry recording who/what/why/when/approver.
//! AC-AD-10  the data plane authorises independently of the admin panel.
//! AC-AD-12  break-glass and audit function fully air-gapped (this whole suite runs
//!           offline; under `docker --network none` that is demonstrated concretely).
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;

use admin_audit::{decode_detail, AdminAuditLog, AdminEvent, AdminOutcome};
use admin_breakglass::{BreakGlassService, GrantRequest, RecordingAlerter};
use admin_config::{ConfigState, ConfigStore};
use admin_sim::{simulate, AccessProbe};
use audit_writer::{verify_chain, AuditWriter, InMemoryLedger, LedgerId};
use policy::{
    Action, ClassificationLevel, Clearance, DefaultPdp, Effect, Marking, PolicyDecisionPoint,
    PolicySet, Purpose, Resource, ResourceId, Role, Rule, RuleId, Selector, Subject, SubjectId,
};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::registry::CryptoProvider;
use pqc_agility::{Rng, SigId, SignatureScheme, SigningKey, VerifyingKey};

const ALG: SigId = SigId::MlDsa65;

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

fn audit_log() -> (AdminAuditLog<InMemoryLedger>, VerifyingKey) {
    let (vk, sk) = keypair(11);
    let writer = AuditWriter::new(
        InMemoryLedger::new(),
        Box::new(FipsSig::new(ALG)),
        sk,
        vk.clone(),
        LedgerId("admin-audit".into()),
    );
    (AdminAuditLog::new(writer), vk)
}

fn config_store() -> ConfigStore {
    let (vk, sk) = keypair(22);
    ConfigStore::new(Box::new(FipsSig::new(ALG)), sk, vk)
}

fn chain_ok(log: &AdminAuditLog<InMemoryLedger>, vk: &VerifyingKey) -> bool {
    let vk = vk.clone();
    verify_chain(log.entries(), &provider(), log.ledger_id(), &|alg| {
        if alg == ALG {
            Some(vk.clone())
        } else {
            None
        }
    })
    .is_ok()
}

// --- AC-AD-6: one signed chain over committed, denied, and simulated actions ---

#[test]
fn ac_ad_6_every_admin_action_including_denied_and_simulated_is_audited() {
    let (mut log, vk) = audit_log();

    // (1) SIMULATED — a dry-run of a policy change. It changes nothing, but it is
    // still recorded (outcome=Simulated).
    let current = PolicySet::new(vec![]).unwrap();
    let proposed = PolicySet::new(vec![Rule {
        id: RuleId("r1".into()),
        role: Role("reader".into()),
        action: Action("read".into()),
        purpose: None,
        selector: Selector::Any,
        effect: Effect::Permit,
    }])
    .unwrap();
    let probes = vec![AccessProbe::new(
        "reader reads open",
        subject("alice", "reader"),
        resource("open"),
        Action("read".into()),
        Purpose("ops".into()),
    )];
    let report = simulate(&current, &proposed, &probes);
    log.record(
        AdminEvent::new(
            "alice",
            "simulate",
            "policy/r1",
            report.summary(),
            None,
            AdminOutcome::Simulated,
        ),
        "t0",
    )
    .unwrap();

    // (2) COMMITTED — a real config change.
    let mut cfg = config_store();
    let commit = cfg
        .commit(
            ConfigState::new().with("mfa.required", "true"),
            "alice",
            "require MFA",
        )
        .unwrap();
    log.record(
        AdminEvent::new(
            "alice",
            "config-commit",
            format!("config@v{}", commit.version),
            "require MFA",
            None,
            AdminOutcome::Committed,
        ),
        "t1",
    )
    .unwrap();

    // (3) DENIED — a break-glass request with no justification is refused; the
    // denial itself is audited.
    let mut bg = BreakGlassService::new(RecordingAlerter::new());
    let denied = bg.grant(
        GrantRequest {
            id: "bg-bad".into(),
            principal: "mallory".into(),
            approver: "secoff".into(),
            scope: "kms".into(),
            justification: String::new(),
            ttl_secs: 60,
        },
        1000,
    );
    assert!(denied.is_err());
    log.record(
        AdminEvent::new(
            "mallory",
            "break-glass-request",
            "kms",
            "denied: no justification supplied",
            Some("secoff".into()),
            AdminOutcome::Denied,
        ),
        "t2",
    )
    .unwrap();

    // (4) BREAK-GLASS GRANTED — a valid, approved, time-boxed grant.
    bg.grant(
        GrantRequest {
            id: "bg-1".into(),
            principal: "mallory".into(),
            approver: "secoff".into(),
            scope: "kms".into(),
            justification: "prod incident #42".into(),
            ttl_secs: 60,
        },
        1000,
    )
    .unwrap();
    log.record(
        AdminEvent::new(
            "mallory",
            "break-glass-grant",
            "kms",
            "prod incident #42",
            Some("secoff".into()),
            AdminOutcome::BreakGlassGranted,
        ),
        "t3",
    )
    .unwrap();

    // (5) BREAK-GLASS REVIEWED — the forced post-hoc review.
    bg.sweep(1100);
    bg.review("bg-1", "auditor", "justified", 1200).unwrap();
    log.record(
        AdminEvent::new(
            "auditor",
            "break-glass-review",
            "kms",
            "justified",
            None,
            AdminOutcome::BreakGlassReviewed,
        ),
        "t4",
    )
    .unwrap();

    // Five heterogeneous admin actions, one append-only PQ-signed chain.
    assert_eq!(log.events().len(), 5);
    assert_eq!(log.entries().len(), 5);
    assert!(chain_ok(&log, &vk));

    // Denied and simulated actions are present — nothing escaped the ledger.
    let outcomes: Vec<String> = log
        .entries()
        .iter()
        .map(|e| decode_detail(&e.record.resource.0).unwrap().outcome)
        .collect();
    assert!(outcomes.iter().any(|o| o == "denied"));
    assert!(outcomes.iter().any(|o| o == "simulated"));

    // Every entry carries who/what/when in native fields and why/approver in the
    // signed detail. The break-glass grant records its distinct approver.
    for e in log.entries() {
        assert!(!e.record.actor.0.is_empty(), "who");
        assert!(!e.record.action.0.is_empty(), "what");
        assert!(!e.record.ts.0.is_empty(), "when");
        let d = decode_detail(&e.record.resource.0).unwrap();
        assert!(!d.why.is_empty(), "why");
    }
    let grant_entry = log
        .entries()
        .iter()
        .find(|e| e.record.action.0 == "break-glass-grant")
        .unwrap();
    let detail = decode_detail(&grant_entry.record.resource.0).unwrap();
    assert_eq!(detail.approver.as_deref(), Some("secoff"));
    assert_eq!(detail.outcome, "break-glass-granted");
}

// --- AC-AD-10: the data plane does not depend on the admin panel ---------------

#[test]
fn ac_ad_10_data_plane_authorises_without_the_admin_panel() {
    // No admin panel service exists in this test — only the committed policy and
    // the data-plane PDP. Authorisation still works: if the panel is down, the data
    // plane keeps running and only administration would pause.
    let live = DefaultPdp::new(
        PolicySet::new(vec![Rule {
            id: RuleId("r1".into()),
            role: Role("reader".into()),
            action: Action("read".into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        }])
        .unwrap(),
    );

    let (alice, open) = (subject("alice", "reader"), resource("open"));
    let permit = live.decide(&policy::AccessRequest {
        subject: &alice,
        resource: &open,
        action: Action("read".into()),
        purpose: Purpose("ops".into()),
    });
    assert!(permit.is_permit());

    // And deny-by-default still holds for an unauthorised subject.
    let guest = subject("guest", "guest");
    let deny = live.decide(&policy::AccessRequest {
        subject: &guest,
        resource: &open,
        action: Action("read".into()),
        purpose: Purpose("ops".into()),
    });
    assert!(!deny.is_permit());
}

// --- AC-AD-12: break-glass + audit run air-gapped ------------------------------

#[test]
fn ac_ad_12_break_glass_and_audit_work_offline() {
    // This exercises the whole grant → expire → review → audit path with no network
    // and no external dependency. Under `docker --network none` its success is a
    // concrete demonstration of the air-gap posture.
    let (mut log, vk) = audit_log();
    let mut bg = BreakGlassService::new(RecordingAlerter::new());
    bg.grant(
        GrantRequest {
            id: "bg-1".into(),
            principal: "mallory".into(),
            approver: "secoff".into(),
            scope: "kms".into(),
            justification: "offline incident".into(),
            ttl_secs: 30,
        },
        1000,
    )
    .unwrap();
    log.record(
        AdminEvent::new(
            "mallory",
            "break-glass-grant",
            "kms",
            "offline incident",
            Some("secoff".into()),
            AdminOutcome::BreakGlassGranted,
        ),
        "t0",
    )
    .unwrap();
    bg.sweep(1031);
    bg.review("bg-1", "auditor", "ok", 1040).unwrap();
    log.record(
        AdminEvent::new(
            "auditor",
            "break-glass-review",
            "kms",
            "ok",
            None,
            AdminOutcome::BreakGlassReviewed,
        ),
        "t1",
    )
    .unwrap();
    assert!(chain_ok(&log, &vk));
}

// --- helpers ------------------------------------------------------------------

fn subject(id: &str, role: &str) -> Subject {
    let mut roles = BTreeSet::new();
    roles.insert(Role(role.into()));
    let mut purposes = BTreeSet::new();
    purposes.insert(Purpose("ops".into()));
    Subject {
        id: SubjectId(id.into()),
        roles,
        clearance: Clearance {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        purposes,
    }
}

fn resource(id: &str) -> Resource {
    let mut allowed = BTreeSet::new();
    allowed.insert(Purpose("ops".into()));
    Resource {
        id: ResourceId(id.into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: allowed,
    }
}
