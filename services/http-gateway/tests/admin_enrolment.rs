//! Online enrolment under dual control, through a real gateway serving a state
//! directory. An IAM administrator signs a challenge with their post-quantum passkey
//! and requests an enrolment; it is parked, and the new principal cannot log in. The
//! requester cannot approve their own, an auditor cannot approve it, a spent challenge
//! cannot be replayed and a weak password hash is refused. The parked enrolment
//! survives a restart, a second IAM administrator approves it, the principal logs in,
//! and every step is in the audit ledger. [FR-AD-5] [AC-AD-3] [AC-AD-4] [RC-29]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};

use admin_identity::{
    AdminChallenge, AdminId, DurableAdminPasskeys, DurableIdentityDirectory, ResourceScope,
    RoleBinding, RoleCatalog, SoftwareAdminAuthenticator, ROLE_AUDITOR, ROLE_IAM_ADMIN,
};
use attest::Keystore;
use http_gateway::admin_api::MAX_CHALLENGES_PER_ADMIN;
use http_gateway::boot::{open_state_dir, DurableGateway};
use http_gateway::http::{Request, Response};
use http_gateway::json::parse;
use http_gateway::policy_file::render_policy;
use http_gateway::state_dir::{StateDir, AUDIT_KEY_ID, IDP_KEY_ID};
use http_gateway::SecuredGateway;
use iam::{PasswordHasher, Pbkdf2Hasher};
use kms::{Kms, KmsSealer};
use policy::{Action, Effect, PolicySet, ResourceId, Role, Rule, RuleId, Selector};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::OsRng;

const NOW: u64 = 1_800_000_000;
const PASSWORD: &str = "correct horse battery staple";

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    std::env::temp_dir().join(format!(
        "admin-enrolment-{tag}-{}-{}",
        std::process::id(),
        uniq
    ))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

/// A state directory with a policy letting operators read and see the ledger view, and
/// three administrators: two IAM administrators and an auditor, each with a passkey the
/// test holds.
fn provision(dir: &Path) -> (StateDir, Vec<(&'static str, SoftwareAdminAuthenticator)>) {
    let state = StateDir::new(dir);
    std::fs::create_dir_all(state.keys()).unwrap();
    std::fs::create_dir_all(state.trust()).unwrap();
    std::fs::create_dir_all(state.admin()).unwrap();
    let idp = Keystore::generate(IDP_KEY_ID, iam::IDP_SIG_ALG, &mut OsRng).unwrap();
    let audit = Keystore::generate(AUDIT_KEY_ID, governance::GOV_SIG_ALG, &mut OsRng).unwrap();
    idp.write(&state.idp_keystore()).unwrap();
    audit.write(&state.audit_keystore()).unwrap();
    // What `mg-admin init` also creates: the ledger and its signed checkpoint.
    http_gateway::boot::init_audit_ledger(&state).unwrap();

    let read = Rule {
        id: RuleId("operator-read".into()),
        role: Role("operator".into()),
        action: Action("Read".into()),
        purpose: None,
        selector: Selector::Any,
        effect: Effect::Permit,
    };
    // Reading the ledger view is its own permission. [VLD-ANB-MIDGARD-0031]
    let audit = Rule {
        id: RuleId("operator-audit".into()),
        action: Action("ReadAudit".into()),
        selector: Selector::Resource(ResourceId("audit-ledger".into())),
        ..read.clone()
    };
    let policy = PolicySet::new(vec![read, audit]).unwrap();
    std::fs::write(state.policy(), render_policy(&policy).unwrap()).unwrap();

    let sealer = || Box::new(KmsSealer::new(Kms::new(fips_provider(), idp.trust_root())));
    let mut passkeys =
        DurableAdminPasskeys::open(state.admin_passkeys(), sealer(), fips_provider()).unwrap();
    let mut roles =
        DurableIdentityDirectory::open(state.admin_roles(), sealer(), RoleCatalog::standard())
            .unwrap();
    let mut devices = Vec::new();
    for (admin, role) in [
        ("alice", ROLE_IAM_ADMIN),
        ("bob", ROLE_IAM_ADMIN),
        ("audra", ROLE_AUDITOR),
    ] {
        let device = SoftwareAdminAuthenticator::enrol(
            &fips_provider(),
            &format!("{admin}-key"),
            &mut OsRng,
        )
        .unwrap();
        passkeys
            .register(AdminId(admin.into()), device.credential().clone())
            .unwrap();
        roles
            .bind(RoleBinding {
                admin: AdminId(admin.into()),
                role: role.into(),
                scope: ResourceScope::all("iam"),
                not_before: 0,
                not_after: u64::MAX,
            })
            .unwrap();
        devices.push((admin, device));
    }
    (state, devices)
}

fn post(gw: &DurableGateway, path: &str, body: &str, token: Option<&str>) -> Response {
    gw.handle(
        &Request {
            method: if token.is_some() && path == "/api/v1/audit" {
                "GET".into()
            } else {
                "POST".into()
            },
            path: path.into(),
            query: if path == "/api/v1/audit" {
                "limit=200".into()
            } else {
                String::new()
            },
            body: body.into(),
            authorization: token.map(|t| format!("Bearer {t}")),
            origin: None,
        },
        NOW,
    )
}

/// Fetch a challenge for `admin`: the nonce, as hex.
fn fetch_challenge(gw: &DurableGateway, admin: &str) -> String {
    let r = post(
        gw,
        "/api/v1/admin/challenge",
        &format!(r#"{{"admin":"{admin}"}}"#),
        None,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    parse(&r.body)
        .unwrap()
        .str_field("challenge")
        .unwrap()
        .to_string()
}

/// Answer `challenge` with `device`: the `challenge` and `assertion` fields an
/// administration request carries.
fn answer(challenge: String, device: &SoftwareAdminAuthenticator) -> (String, String) {
    let nonce: [u8; 32] = unhex(&challenge).try_into().unwrap();
    let assertion = device
        .authenticate(&fips_provider(), &AdminChallenge::new(nonce), true)
        .unwrap();
    (
        challenge,
        format!(
            r#"{{"credentialId":"{}","userVerified":true,"signature":"{}"}}"#,
            assertion.credential_id,
            hex(assertion.signature.as_bytes())
        ),
    )
}

/// Fetch a challenge for `admin` and answer it with `device`.
fn proof(
    gw: &DurableGateway,
    admin: &str,
    device: &SoftwareAdminAuthenticator,
) -> (String, String) {
    answer(fetch_challenge(gw, admin), device)
}

fn request_body(admin: &str, proof: &(String, String), password_hash: &str) -> String {
    format!(
        r#"{{"admin":"{admin}","challenge":"{}","assertion":{},"justification":"new operator","principal":{{"subject":"olivia","roles":["operator"],"clearance":"internal","purposes":["operations"],"passwordHash":"{password_hash}"}}}}"#,
        proof.0, proof.1
    )
}

fn approve_body(admin: &str, proof: &(String, String), id: &str) -> String {
    format!(
        r#"{{"admin":"{admin}","challenge":"{}","assertion":{},"id":"{id}"}}"#,
        proof.0, proof.1
    )
}

fn login(gw: &DurableGateway) -> Response {
    post(
        gw,
        "/api/v1/login",
        &format!(r#"{{"username":"olivia","password":"{PASSWORD}"}}"#),
        None,
    )
}

#[test]
fn an_enrolment_needs_a_second_administrator_and_survives_a_restart() {
    let dir = scratch("dual");
    let (state, devices) = provision(&dir);
    let device = |name: &str| &devices.iter().find(|(n, _)| *n == name).unwrap().1;
    let strong_hash = Pbkdf2Hasher::production().hash(PASSWORD.as_bytes());

    let id = {
        let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
        assert_eq!(served.administrators, 3);
        let gw = &served.gateway;

        // A hash below the production work factor is refused outright.
        let weak = Pbkdf2Hasher::new(1_000).hash(PASSWORD.as_bytes());
        let r = post(
            gw,
            "/api/v1/admin/enrolments",
            &request_body("alice", &proof(gw, "alice", device("alice")), &weak),
            None,
        );
        assert_eq!(r.status, 400, "{}", r.body);

        // Alice requests; the enrolment is parked and olivia cannot log in.
        let alice = proof(gw, "alice", device("alice"));
        let r = post(
            gw,
            "/api/v1/admin/enrolments",
            &request_body("alice", &alice, &strong_hash),
            None,
        );
        assert_eq!(r.status, 200, "{}", r.body);
        let id = parse(&r.body).unwrap().str_field("id").unwrap().to_string();
        assert_eq!(login(gw).status, 401, "not enrolled until approved");

        // The spent challenge cannot be replayed.
        let replay = post(
            gw,
            "/api/v1/admin/enrolments/approve",
            &approve_body("bob", &alice, &id),
            None,
        );
        assert_eq!(replay.status, 401, "{}", replay.body);

        // Alice cannot approve her own request; the auditor cannot approve it at all.
        let r = post(
            gw,
            "/api/v1/admin/enrolments/approve",
            &approve_body("alice", &proof(gw, "alice", device("alice")), &id),
            None,
        );
        assert_eq!(r.status, 403, "{}", r.body);
        // The privilege service's reason stays in the gateway's log. [VLD-ANB-MIDGARD-0053]
        assert_eq!(r.body, r#"{"error":"refused"}"#);
        let r = post(
            gw,
            "/api/v1/admin/enrolments/approve",
            &approve_body("audra", &proof(gw, "audra", device("audra")), &id),
            None,
        );
        assert_eq!(r.status, 403, "{}", r.body);
        assert_eq!(login(gw).status, 401);
        id
    }; // restart

    let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots again");
    let gw = &served.gateway;
    let bob = proof(gw, "bob", device("bob"));
    let listed = post(
        gw,
        "/api/v1/admin/enrolments/list",
        &format!(
            r#"{{"admin":"bob","challenge":"{}","assertion":{}}}"#,
            bob.0, bob.1
        ),
        None,
    );
    assert_eq!(listed.status, 200, "{}", listed.body);
    assert!(
        listed.body.contains(&format!(r#""id":"{id}""#))
            && listed.body.contains(r#""requestedBy":"alice""#),
        "the parked enrolment survives a restart: {}",
        listed.body
    );

    let r = post(
        gw,
        "/api/v1/admin/enrolments/approve",
        &approve_body("bob", &proof(gw, "bob", device("bob")), &id),
        None,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let ok = login(gw);
    assert_eq!(ok.status, 200, "olivia logs in once approved: {}", ok.body);

    // Every step is in the governance ledger.
    let token = parse(&ok.body).unwrap();
    let token = token.str_field("token").unwrap();
    let audit = post(gw, "/api/v1/audit", "", Some(token));
    assert_eq!(audit.status, 200, "{}", audit.body);
    for (actor, action) in [
        ("alice", "ENROLL requested"),
        ("bob", "ENROLL approved"),
        ("alice", "ADMIN refused (403)"),
        ("audra", "ADMIN refused (403)"),
    ] {
        assert!(
            audit
                .body
                .contains(&format!(r#""actor":"{actor}","action":"{action}""#)),
            "{actor} {action} is audited: {}",
            audit.body
        );
    }
    drop(served);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_development_gateway_serves_no_administration() {
    let gw = SecuredGateway::dev();
    let r = gw.handle(
        &Request {
            method: "POST".into(),
            path: "/api/v1/admin/challenge".into(),
            query: String::new(),
            body: String::new(),
            authorization: None,
            origin: None,
        },
        NOW,
    );
    assert_eq!(r.status, 404, "{}", r.body);
}

// ---- WP-16: challenges bound to the administrator who asked ---------------------

/// The listing body for `admin` answering with `proof`: the cheapest request that
/// verifies a challenge.
fn list_body(admin: &str, proof: &(String, String)) -> String {
    format!(
        r#"{{"admin":"{admin}","challenge":"{}","assertion":{}}}"#,
        proof.0, proof.1
    )
}

/// Whether `admin` answering `challenge` with `device` signs in.
fn verifies(
    gw: &DurableGateway,
    admin: &str,
    challenge: String,
    device: &SoftwareAdminAuthenticator,
) -> Response {
    post(
        gw,
        "/api/v1/admin/enrolments/list",
        &list_body(admin, &answer(challenge, device)),
        None,
    )
}

#[test]
fn a_challenge_is_bound_to_the_administrator_who_asked() {
    // [VLD-ANB-MIDGARD-0020] [AC-16.2]
    let dir = scratch("bound");
    let (state, devices) = provision(&dir);
    let device = |name: &str| &devices.iter().find(|(n, _)| *n == name).unwrap().1;
    let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
    let gw = &served.gateway;

    // Alice's challenge, signed by and presented as Bob.
    let alices = fetch_challenge(gw, "alice");
    let r = verifies(gw, "bob", alices.clone(), device("bob"));
    assert_eq!(r.status, 401, "{}", r.body);
    assert!(
        r.body.contains("the challenge is unknown, used or expired"),
        "{}",
        r.body
    );
    // The nonce was consumed: not even Alice can answer it now.
    let r = verifies(gw, "alice", alices, device("alice"));
    assert_eq!(r.status, 401, "consumed: {}", r.body);

    // A challenge of Bob's own works.
    let r = verifies(gw, "bob", fetch_challenge(gw, "bob"), device("bob"));
    assert_eq!(r.status, 200, "{}", r.body);
    drop(served);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_flood_of_challenges_cannot_evict_another_administrators() {
    // [VLD-ANB-MIDGARD-0020] [AC-16.1]
    let dir = scratch("flood");
    let (state, devices) = provision(&dir);
    let device = |name: &str| &devices.iter().find(|(n, _)| *n == name).unwrap().1;
    let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
    let gw = &served.gateway;

    let alices = fetch_challenge(gw, "alice");
    for _ in 0..1_000 {
        fetch_challenge(gw, "bob");
    }
    let bob = AdminId("bob".into());
    assert_eq!(
        gw.admin_control().unwrap().outstanding_for(&bob),
        MAX_CHALLENGES_PER_ADMIN
    );
    let r = verifies(gw, "alice", alices, device("alice"));
    assert_eq!(r.status, 200, "Alice's challenge survives: {}", r.body);
    drop(served);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_challenge_request_without_admin_is_400() {
    // [VLD-ANB-MIDGARD-0020] [AC-16.3]
    let dir = scratch("no-admin");
    let (state, _devices) = provision(&dir);
    let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
    let gw = &served.gateway;
    for body in ["", "{}", r#"{"admin":5}"#, r#"{"admin":"alice","extra":1}"#] {
        let r = post(gw, "/api/v1/admin/challenge", body, None);
        assert_eq!(r.status, 400, "{body:?}: {}", r.body);
    }
    assert_eq!(gw.admin_control().unwrap().outstanding(), 0);
    drop(served);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_administrators_get_a_nonce_but_no_table_entry() {
    // [VLD-ANB-MIDGARD-0020] [AC-16.3]
    let dir = scratch("unknown");
    let (state, _devices) = provision(&dir);
    let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
    let gw = &served.gateway;
    fetch_challenge(gw, "alice");
    let before = gw.admin_control().unwrap().outstanding();
    for i in 0..1_000 {
        let challenge = fetch_challenge(gw, &format!("nobody-{i}"));
        assert!(
            challenge.len() == 64 && challenge.bytes().all(|b| b.is_ascii_hexdigit()),
            "{challenge}"
        );
    }
    assert_eq!(gw.admin_control().unwrap().outstanding(), before);
    drop(served);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_global_ceiling_refuses_rather_than_evicts() {
    // [VLD-ANB-MIDGARD-0020] [AC-16.4]
    let dir = scratch("ceiling");
    let (state, devices) = provision(&dir);
    let device = |name: &str| &devices.iter().find(|(n, _)| *n == name).unwrap().1;
    let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
    let gw = &served.gateway;
    gw.admin_control().unwrap().set_challenge_ceiling(8);

    let held: Vec<(&str, String)> = ["alice", "bob"]
        .into_iter()
        .flat_map(|admin| (0..4).map(move |_| admin))
        .map(|admin| (admin, fetch_challenge(gw, admin)))
        .collect();

    // Audra holds none and the table is full: refused, and recorded once a minute.
    let audited = gw.audit_len();
    let audra = r#"{"admin":"audra"}"#;
    let r = post(gw, "/api/v1/admin/challenge", audra, None);
    assert_eq!(r.status, 429, "{}", r.body);
    assert!(
        r.body.contains("too many outstanding challenges"),
        "{}",
        r.body
    );
    assert_eq!(gw.audit_len(), audited + 1, "the first refusal is recorded");
    let r = post(gw, "/api/v1/admin/challenge", audra, None);
    assert_eq!(r.status, 429, "{}", r.body);
    assert_eq!(
        gw.audit_len(),
        audited + 1,
        "within the minute: counted only"
    );
    assert_eq!(
        gw.admin_control().unwrap().note_challenge_refusal(NOW + 60),
        Some(1),
        "a minute on, the next record carries the count"
    );

    // Neither earlier administrator lost a challenge.
    for (admin, challenge) in held {
        let r = verifies(gw, admin, challenge, device(admin));
        assert_eq!(r.status, 200, "{admin}: {}", r.body);
    }
    drop(served);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The audit key the operator pins the gateway to, from outside the keystore it
/// checks. [MG-SEC-05]
fn pin(state: &StateDir) -> audit_writer::PinnedKey {
    audit_writer::PinnedKey::new(
        attest::Keystore::read(&state.audit_keystore())
            .unwrap()
            .verifying_key,
    )
}
