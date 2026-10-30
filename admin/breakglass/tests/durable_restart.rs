//! RC-14 (break-glass): grants + reviews survive a restart, recovered intact —
//! including the authenticated approver. [MG-SEC-07]

mod common;

use std::path::PathBuf;

use admin_breakglass::{BreakGlassState, DurableBreakGlass, GrantRequest, RecordingAlerter};
use admin_identity::{ROLE_IAM_ADMIN, ROLE_SECURITY_OFFICER};
use common::{directory, Admins, APPROVAL_MAX_AGE};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
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
