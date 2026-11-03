//! RC-14 (break-glass): grants + reviews survive a restart, recovered intact —
//! including the authenticated approver. [MG-SEC-07]
//!
//! So does ending a grant early, through the store or the controller, and the
//! one-time expiry alert. A journal record whose time-box is longer than the
//! four-hour cap is refused on boot rather than restored. [FR-AD-8]
//! [VLD-ANB-MIDGARD-0055]

mod common;

use std::path::PathBuf;

use admin_breakglass::{
    AlertKind, BreakGlassController, BreakGlassError, BreakGlassState, DurableBreakGlass,
    GrantRequest, RecordingAlerter, MAX_TTL_SECS,
};
use admin_identity::{AdminSession, IdentityDirectory, ROLE_IAM_ADMIN, ROLE_SECURITY_OFFICER};
use audit_writer::{AuditWriter, InMemoryLedger, LedgerId, SealedJournal};
use common::{directory, Admins, APPROVAL_MAX_AGE};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{Rng, SigId, SignatureScheme};

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
    let d = std::env::temp_dir().join(format!("admin-bg-{}-{}", std::process::id(), uniq));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sealer() -> KmsSealer<SoftwareTrustRoot> {
    let p = fips_provider();
    let root = SoftwareTrustRoot::generate(&p, "bg-root", &mut SeedRng(9)).unwrap();
    KmsSealer::new(Kms::new(p, root))
}

fn open(path: &PathBuf) -> DurableBreakGlass<RecordingAlerter> {
    DurableBreakGlass::open(path, Box::new(sealer()), RecordingAlerter::new()).unwrap()
}

fn req(id: &str, ttl: u64) -> GrantRequest {
    GrantRequest {
        id: id.into(),
        principal: "responder".into(),
        scope: "kms".into(),
        justification: "sev-1 incident".into(),
        ttl_secs: ttl,
    }
}

#[test]
fn breakglass_grants_survive_restart() {
    let dir = scratch();
    let path = dir.join("breakglass.journal");

    let mut admins = Admins::new(&["responder", "duty-officer"]);
    let responder = admins.sign_in("responder", 1_000);
    let officer = admins.sign_in("duty-officer", 1_000);
    let roles = directory(&[
        ("responder", ROLE_IAM_ADMIN),
        ("duty-officer", ROLE_SECURITY_OFFICER),
    ]);
    {
        let mut store = open(&path);
        store
            .grant(
                &roles,
                req("bg-1", 3600),
                &responder,
                &officer,
                1_000,
                APPROVAL_MAX_AGE,
            )
            .unwrap();
        store
            .grant(
                &roles,
                req("bg-2", 60),
                &responder,
                &officer,
                1_000,
                APPROVAL_MAX_AGE,
            )
            .unwrap();
        // bg-2 gets its mandatory post-hoc review.
        store
            .review("bg-2", "security-officer", "justified", 2_000)
            .unwrap();
        assert!(store.service().is_active("bg-1", 1_500));
    }

    let on_disk = std::fs::read(&path).unwrap();
    for n in [b"sev-1 incident".as_slice(), b"responder"] {
        assert!(
            !on_disk.windows(n.len()).any(|w| w == n),
            "plaintext leak: {:?}",
            String::from_utf8_lossy(n)
        );
    }

    // Restart: grants + review recovered, states recomputed identically.
    let store = open(&path);
    let s = store.service();
    assert!(s.is_active("bg-1", 1_500), "active grant recovered");
    assert_eq!(
        s.state("bg-1", 5_000),
        Some(BreakGlassState::Expired),
        "time-box recovered"
    );
    assert_eq!(
        s.state("bg-2", 5_000),
        Some(BreakGlassState::Reviewed),
        "review recovered"
    );
    assert_eq!(
        s.get("bg-2").unwrap().review.as_ref().unwrap().reviewer,
        "security-officer"
    );
    assert_eq!(
        s.get("bg-1").unwrap().approver,
        "duty-officer",
        "the authenticated approver is recovered"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A responder and a security officer, signed in at 1 000, with their roles.
fn people() -> (AdminSession, AdminSession, IdentityDirectory) {
    let mut admins = Admins::new(&["responder", "duty-officer"]);
    let responder = admins.sign_in("responder", 1_000);
    let officer = admins.sign_in("duty-officer", 1_000);
    let roles = directory(&[
        ("responder", ROLE_IAM_ADMIN),
        ("duty-officer", ROLE_SECURITY_OFFICER),
    ]);
    (responder, officer, roles)
}

#[test]
fn a_closed_grant_is_still_closed_after_restart() {
    let dir = scratch();
    let path = dir.join("breakglass.journal");
    let (responder, officer, roles) = people();
    {
        let mut store = open(&path);
        store
            .grant(
                &roles,
                req("bg-1", 3600),
                &responder,
                &officer,
                1_000,
                APPROVAL_MAX_AGE,
            )
            .unwrap();
        store.close("bg-1", 1_200).unwrap();
        assert!(!store.service().is_active("bg-1", 1_500));
    }

    // Restart: the cut time-box comes back, not the original hour.
    let store = open(&path);
    let s = store.service();
    assert!(!s.is_active("bg-1", 1_500), "the close is recovered");
    assert_eq!(s.get("bg-1").unwrap().expires_at, 1_200);
    assert_eq!(
        s.state("bg-1", 1_500),
        Some(BreakGlassState::Expired),
        "a closed grant still owes its review"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_expiry_alert_is_not_raised_again_after_restart() {
    let dir = scratch();
    let path = dir.join("breakglass.journal");
    let (responder, officer, roles) = people();
    {
        let mut store = open(&path);
        store
            .grant(
                &roles,
                req("bg-1", 60),
                &responder,
                &officer,
                1_000,
                APPROVAL_MAX_AGE,
            )
            .unwrap();
        assert_eq!(store.sweep(2_000).unwrap(), vec!["bg-1".to_string()]);
    }

    let mut store = open(&path);
    assert!(
        store.sweep(3_000).unwrap().is_empty(),
        "the one-time alert was recorded"
    );
    assert_eq!(
        store
            .service()
            .alerter()
            .count(AlertKind::ExpiredUnreviewed),
        0
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Write one grant record, granted at 1 000 with a time-box `ttl` long, in the
/// store's own format and sealed as the store seals it: what an old or tampered
/// journal would hold.
fn write_record(path: &PathBuf, ttl: u64) {
    fn put(o: &mut Vec<u8>, s: &str) {
        o.extend_from_slice(&(s.len() as u32).to_le_bytes());
        o.extend_from_slice(s.as_bytes());
    }
    let mut o = Vec::new();
    for s in ["bg-1", "responder", "duty-officer", "kms", "sev-1 incident"] {
        put(&mut o, s);
    }
    o.extend_from_slice(&1_000u64.to_le_bytes());
    o.extend_from_slice(&(1_000 + ttl).to_le_bytes());
    o.push(0); // not reviewed
    o.push(0); // no expiry alert yet
    let (mut journal, _) =
        SealedJournal::open(path, Box::new(sealer()), b"midgard-admin-breakglass-v1").unwrap();
    journal.append(&o).unwrap();
}

#[test]
fn a_journal_record_past_the_four_hour_cap_is_refused() {
    let dir = scratch();

    // At the cap exactly, the record is restored as written.
    let at_cap = dir.join("at-cap.journal");
    write_record(&at_cap, MAX_TTL_SECS);
    let store = open(&at_cap);
    assert_eq!(
        store.service().get("bg-1").unwrap().expires_at,
        1_000 + MAX_TTL_SECS
    );

    // One second past it, the store will not open, as for any other invalid
    // record, rather than restore a grant the service could never have issued.
    let past_cap = dir.join("past-cap.journal");
    write_record(&past_cap, MAX_TTL_SECS + 1);
    match DurableBreakGlass::open(&past_cap, Box::new(sealer()), RecordingAlerter::new()) {
        Err(e) => assert_eq!(e, BreakGlassError::TtlTooLong { max: MAX_TTL_SECS }),
        Ok(_) => panic!("a grant longer than the cap was restored"),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_controllers_close_survives_a_restart() {
    let dir = scratch();
    let path = dir.join("breakglass.journal");
    let (responder, officer, roles) = people();
    let controller = |store| {
        let alg = SigId::MlDsa65;
        let (vk, sk) = FipsSig::new(alg).keypair(&mut SeedRng(5)).unwrap();
        let audit = AuditWriter::new(
            InMemoryLedger::new(),
            Box::new(FipsSig::new(alg)),
            sk,
            vk,
            LedgerId("admin-breakglass".into()),
        );
        BreakGlassController::new(store, audit)
    };
    {
        let mut c = controller(open(&path));
        for id in ["bg-1", "bg-2"] {
            c.grant(
                &roles,
                req(id, 3600),
                &responder,
                &officer,
                APPROVAL_MAX_AGE,
                "kms.key.rotate",
                1_000,
                "t0",
            )
            .unwrap();
        }
        c.close("bg-1", "duty-officer", 1_200, "t1").unwrap();
        assert!(c.elevates("responder", "kms", "kms.key.rotate", 1_100));
    }

    // Restart: the controller's close was journalled, so bg-1 stays closed.
    let c = controller(open(&path));
    assert!(
        !c.service().is_active("bg-1", 1_500),
        "the close is recovered"
    );
    assert_eq!(
        c.service().state("bg-1", 1_500),
        Some(BreakGlassState::Expired)
    );
    // bg-2 is still within its time-box, but elevation tokens live in memory, so
    // after a restart no one is elevated until a grant is issued again.
    assert!(c.service().is_active("bg-2", 1_500));
    assert!(!c.elevates("responder", "kms", "kms.key.rotate", 1_500));

    let _ = std::fs::remove_dir_all(&dir);
}
