//! A durable enclosure: the edge's object fabric over a KMS-sealed
//! `DurableObjectStore`, and its governance spine over a KMS-sealed `FileLedger`.
//! What an action changed and the audit lineage that authorised it both survive a
//! restart, the recovered ledger chain re-verifies across both sessions, nothing
//! readable is left on disk, and the gateway holding a durable enclosure can be
//! shared across connection threads. [AC-M-4] [RC-09] [RC-10]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use audit_writer::{verify_chain, AuditWriter, FileLedger, LedgerId};
use fabric::{DurableObjectStore, PolicyContext};
use governance::{Governance, GOV_SIG_ALG};
use http_gateway::enclosure::Enclosure;
use http_gateway::http::Request;
use http_gateway::json::Json;
use http_gateway::{demo_approver, demo_operator, dev_login_policy, SecuredGateway};
use kms::{Kms, KmsSealer, SoftwareTrustRoot};
use policy::{DefaultPdp, Purpose};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{Rng, SigId, SignatureScheme, SigningKey, VerifyingKey};

const PURPOSE: &str = "operations";
const LEDGER: &str = "enclosure-durable";
const NOW: u64 = 1_800_000_000;
const ROOT_SEED: u64 = 13_579;
const AUDIT_KEY_SEED: u64 = 97_531;

type DurableEnclosure = Enclosure<DurableObjectStore, FileLedger>;

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

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "enclosure-durable-{tag}-{}-{}",
        std::process::id(),
        uniq
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Regenerating the software root from the same seed models the same KMS identity
/// being present after a restart.
fn sealer() -> KmsSealer<SoftwareTrustRoot> {
    let provider = fips_provider();
    let root =
        SoftwareTrustRoot::generate(&provider, "enclosure-root", &mut SeedRng(ROOT_SEED)).unwrap();
    KmsSealer::new(Kms::new(provider, root))
}

/// The governance audit key, stable across the restart.
fn audit_keys() -> (VerifyingKey, SigningKey) {
    FipsSig::new(GOV_SIG_ALG)
        .keypair(&mut SeedRng(AUDIT_KEY_SEED))
        .unwrap()
}

/// Boot an enclosure over the durable store and ledger in `dir`. The schema is
/// registered on every boot; data is not seeded here.
fn boot(dir: &Path) -> DurableEnclosure {
    let (vk, sk) = audit_keys();
    let ledger = FileLedger::open_or_create(
        dir.join("governance.ledger"),
        Box::new(sealer()),
        LedgerId(LEDGER.into()),
    )
    .expect("ledger opens");
    let writer = AuditWriter::new(
        ledger,
        Box::new(FipsSig::new(GOV_SIG_ALG)),
        sk,
        vk,
        LedgerId(LEDGER.into()),
    );
    let governance = Governance::with_audit(DefaultPdp::new(dev_login_policy()), writer);
    let store = DurableObjectStore::open(
        dir.join("fabric.store"),
        Box::new(sealer()),
        "object-fabric",
    )
    .expect("store opens");
    let mut enclosure = Enclosure::with_store(store, governance);
    enclosure
        .register_dev_schema(PURPOSE, NOW)
        .expect("schema registers");
    enclosure
}

fn operator() -> PolicyContext {
    let user = demo_operator();
    PolicyContext {
        subject: "operator".into(),
        roles: user.roles,
        clearance: user.clearance,
        purpose: Purpose(PURPOSE.into()),
    }
}

fn approver() -> PolicyContext {
    let user = demo_approver();
    PolicyContext {
        subject: "approver".into(),
        roles: user.roles,
        clearance: user.clearance,
        purpose: Purpose(PURPOSE.into()),
    }
}

/// A human gate on `action` against `target`, requested by the operator and approved
/// by someone else; its id is the action's `humanGateToken`.
fn approved_gate(enclosure: &mut DurableEnclosure, action: &str, target: &str) -> String {
    let requested = enclosure
        .request_approval(&operator(), action, target, NOW)
        .unwrap_or_else(|e| panic!("approval request refused: {}", e.body.encode()));
    let id = requested.str_field("id").expect("approval id").to_string();
    enclosure
        .decide_approval(&approver(), &id, true, NOW)
        .unwrap_or_else(|e| panic!("approval refused: {}", e.body.encode()));
    id
}

/// An aircraft's `status`, read through the governed query path.
fn status_of(enclosure: &mut DurableEnclosure, id: &str) -> String {
    let result = enclosure
        .query(
            &operator(),
            fabric::Search {
                object_type: "aircraft".into(),
                conditions: Vec::new(),
                order_by: None,
                descending: false,
                offset: 0,
                limit: 50,
            },
            NOW,
        )
        .unwrap_or_else(|e| panic!("query refused: {}", e.body.encode()));
    let Some(Json::Arr(objects)) = result.get("results") else {
        panic!("no results: {}", result.encode());
    };
    objects
        .iter()
        .find(|o| o.str_field("id") == Some(id))
        .and_then(|o| o.get("propertyValues"))
        .and_then(|p| p.str_field("status"))
        .unwrap_or_else(|| panic!("{id} not visible: {}", result.encode()))
        .to_string()
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn an_action_and_its_lineage_survive_a_restart() {
    let dir = scratch("restart");

    // --- session 1: seed once, ground an aircraft through the governed action path ---
    let audited_before_exit = {
        let mut enclosure = boot(&dir);
        enclosure.seed_dev_objects(PURPOSE);
        assert_eq!(status_of(&mut enclosure, "aircraft/AC-2214"), "active");

        let params: BTreeMap<String, String> =
            [("target".to_string(), "aircraft/AC-2214".to_string())].into();
        let gate = approved_gate(&mut enclosure, "Ground aircraft", "aircraft/AC-2214");
        let applied = enclosure
            .apply_action(
                &operator(),
                "Ground aircraft",
                params,
                Some(gate.as_str()),
                NOW,
            )
            .unwrap_or_else(|e| panic!("action refused: {}", e.body.encode()));
        assert!(
            applied.encode().contains(r#""applied":true"#),
            "{}",
            applied.encode()
        );
        assert_eq!(status_of(&mut enclosure, "aircraft/AC-2214"), "grounded");
        enclosure.audit_len()
    }; // enclosure, store and ledger dropped: a simulated process exit.

    // Encrypted at rest: neither the object state nor the decision is on disk in the
    // clear.
    let store_bytes = std::fs::read(dir.join("fabric.store")).unwrap();
    let ledger_bytes = std::fs::read(dir.join("governance.ledger")).unwrap();
    for needle in ["AC-2214", "grounded", "Heavy transport"] {
        assert!(
            !contains(&store_bytes, needle),
            "{needle} readable in store"
        );
    }
    for needle in ["operator", "groundAircraft"] {
        assert!(
            !contains(&ledger_bytes, needle),
            "{needle} readable in ledger"
        );
    }

    // --- session 2: boot again (schema only, no seed) ---
    let mut enclosure = boot(&dir);
    assert_eq!(
        enclosure.audit_len(),
        audited_before_exit,
        "every audit entry from the first session is recovered"
    );
    assert_eq!(
        status_of(&mut enclosure, "aircraft/AC-2214"),
        "grounded",
        "the action's effect was journalled"
    );
    assert_eq!(
        status_of(&mut enclosure, "aircraft/AC-2261"),
        "active",
        "only the targeted aircraft changed"
    );
    assert_eq!(
        status_of(&mut enclosure, "aircraft/AC-2198"),
        "maintenance",
        "seeded objects are not written twice or reset"
    );
    assert!(enclosure.audit_len() > audited_before_exit);

    // One chain across both sessions: new entries extend the recovered one.
    let (vk, _) = audit_keys();
    let vk_for =
        move |alg: SigId| -> Option<VerifyingKey> { (alg == GOV_SIG_ALG).then(|| vk.clone()) };
    verify_chain(
        enclosure.fabric().audit_entries(),
        &fips_provider(),
        &LedgerId(LEDGER.into()),
        &vk_for,
    )
    .expect("the ledger chain re-verifies across the restart");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_requester_cannot_decide_their_own_approval() {
    let dir = scratch("two-person");
    let mut enclosure = boot(&dir);
    enclosure.seed_dev_objects(PURPOSE);
    let requested = enclosure
        .request_approval(&operator(), "Ground aircraft", "aircraft/AC-2214", NOW)
        .unwrap_or_else(|e| panic!("approval request refused: {}", e.body.encode()));
    let id = requested.str_field("id").expect("approval id").to_string();

    // Whatever roles the requester holds, the two-person rule is the enclosure's own.
    let own = enclosure.decide_approval(&operator(), &id, true, NOW);
    assert_eq!(own.err().map(|e| e.status), Some(403));
    assert!(
        enclosure
            .approvals(&operator(), NOW)
            .encode()
            .contains(r#""status":"pending""#),
        "a refused decision leaves the approval pending"
    );
    drop(enclosure);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_gateway_over_a_durable_enclosure_serves_connection_threads() {
    fn shared_across_threads<T: Send + Sync>(_: &T) {}

    let dir = scratch("threads");
    let (issuer_vk, _) = FipsSig::new(GOV_SIG_ALG)
        .keypair(&mut SeedRng(AUDIT_KEY_SEED + 1))
        .unwrap();
    let gateway = Arc::new(SecuredGateway::with_enclosure(
        fips_provider(),
        issuer_vk,
        boot(&dir),
        vec!["http://localhost:5173".to_string()],
    ));
    shared_across_threads(&gateway);

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let gateway = Arc::clone(&gateway);
            thread::spawn(move || {
                gateway
                    .handle(
                        &Request {
                            method: "GET".into(),
                            path: "/api/v1/health".into(),
                            query: String::new(),
                            body: String::new(),
                            authorization: None,
                            origin: None,
                        },
                        NOW,
                    )
                    .status
            })
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.join().expect("connection thread"), 200);
    }

    let _ = std::fs::remove_dir_all(&dir);
}
