//! Online enrolment under dual control, through a real gateway serving a state
//! directory. An IAM administrator signs a challenge with their post-quantum passkey
//! and requests an enrolment; it is parked, and the new principal cannot log in. The
//! requester cannot approve their own, an auditor cannot approve it and a spent
//! challenge cannot be replayed. The parked enrolment survives a restart, a second IAM
//! administrator approves it, the principal activates their account with the code the
//! approval returned and logs in with both factors, and every step is in the audit
//! ledger. [FR-AD-5] [AC-AD-3] [AC-AD-4] [RC-29]
//!
//! Neither administrator ever holds the principal's credentials: a request carrying
//! credential material is refused, the gateway mints them at approval, and only the
//! principal sees the second factor. Roles and purposes come from the served policy,
//! and a failed approval can be retried or the enrolment rejected.
//! [VLD-ANB-MIDGARD-0002] [VLD-ANB-MIDGARD-0066] [AC-15.1] [AC-15.2] [AC-15.3]
//! [AC-15.4] [AC-15.5] [AC-15.6]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};

use admin_identity::{
    AdminChallenge, AdminId, DurableAdminPasskeys, DurableIdentityDirectory, ResourceScope,
    RoleBinding, RoleCatalog, SoftwareAdminAuthenticator, ROLE_AUDITOR, ROLE_IAM_ADMIN,
};
use admin_privilege::{ActionState, DurablePrivilege};
use attest::Keystore;
use audit_writer::{LedgerId, Sealer};
use http_gateway::admin_api::{AdminControl, RoleCatalogue, ACTIVATION_TTL, MAX_CHALLENGES_PER_ADMIN};
use http_gateway::boot::{open_state_dir, DurableGateway};
use http_gateway::http::{Request, Response};
use http_gateway::json::parse;
use http_gateway::policy_file::render_policy;
use http_gateway::state_dir::{StateDir, AUDIT_KEY_ID, IDP_KEY_ID, PRIVILEGE_LEDGER_ID};
use http_gateway::SecuredGateway;
use iam::{DurableDirectory, Enrollment, Pbkdf2Hasher, TotpSecret};
use kms::{Kms, KmsSealer};
use policy::{Action, Effect, PolicySet, ResourceId, Role, Rule, RuleId, Selector};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{OsRng, SecretBytes, SigningKey};

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
    post_at(gw, path, body, token, NOW)
}

fn post_at(gw: &DurableGateway, path: &str, body: &str, token: Option<&str>, now: u64) -> Response {
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
        now,
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

/// The principal every test enrols unless it says otherwise: who they are and what
/// they may do, and nothing about how they authenticate.
fn principal(subject: &str) -> String {
    format!(
        r#"{{"subject":"{subject}","roles":["operator"],"clearance":"internal","purposes":["operations"]}}"#
    )
}

fn request_with(admin: &str, proof: &(String, String), principal: &str) -> String {
    format!(
        r#"{{"admin":"{admin}","challenge":"{}","assertion":{},"justification":"new operator","principal":{principal}}}"#,
        proof.0, proof.1
    )
}

fn request_body(admin: &str, proof: &(String, String)) -> String {
    request_with(admin, proof, &principal("olivia"))
}

fn approve_body(admin: &str, proof: &(String, String), id: &str, subject: &str) -> String {
    format!(
        r#"{{"admin":"{admin}","challenge":"{}","assertion":{},"id":"{id}","subject":"{subject}"}}"#,
        proof.0, proof.1
    )
}

fn reject_body(admin: &str, proof: &(String, String), id: &str) -> String {
    format!(
        r#"{{"admin":"{admin}","challenge":"{}","assertion":{},"id":"{id}","reason":"not a known starter"}}"#,
        proof.0, proof.1
    )
}

fn list_body(admin: &str, proof: &(String, String)) -> String {
    format!(
        r#"{{"admin":"{admin}","challenge":"{}","assertion":{}}}"#,
        proof.0, proof.1
    )
}

fn login_as(gw: &DurableGateway, subject: &str, totp: Option<&str>) -> Response {
    let totp = totp.map_or(String::new(), |code| format!(r#","totp":"{code}""#));
    post(
        gw,
        "/api/v1/login",
        &format!(r#"{{"username":"{subject}","password":"{PASSWORD}"{totp}}}"#),
        None,
    )
}

fn login(gw: &DurableGateway) -> Response {
    login_as(gw, "olivia", None)
}

fn activate_at(gw: &DurableGateway, subject: &str, code: &str, now: u64) -> Response {
    post_at(
        gw,
        "/api/v1/activate",
        &format!(r#"{{"username":"{subject}","activationCode":"{code}","password":"{PASSWORD}"}}"#),
        None,
        now,
    )
}

fn field(r: &Response, key: &str) -> String {
    parse(&r.body)
        .unwrap()
        .str_field(key)
        .unwrap_or_else(|| panic!("no {key} in {}", r.body))
        .to_string()
}

/// The TOTP secret in an `otpauth://` URI (RFC 4648 base32, unpadded).
fn totp_of(uri: &str) -> TotpSecret {
    let encoded = uri
        .split(['?', '&'])
        .find_map(|part| part.strip_prefix("secret="))
        .expect("a secret parameter");
    let (mut bits, mut acc, mut out) = (0u32, 0u32, Vec::new());
    for c in encoded.chars() {
        let v = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"
            .find(c.to_ascii_uppercase())
            .expect("base32") as u32;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    TotpSecret::new(out)
}

type Devices = Vec<(&'static str, SoftwareAdminAuthenticator)>;

fn device<'a>(devices: &'a Devices, name: &str) -> &'a SoftwareAdminAuthenticator {
    &devices.iter().find(|(n, _)| *n == name).unwrap().1
}

/// `subject` requested by alice: the enrolment id.
fn request(gw: &DurableGateway, devices: &Devices, subject: &str) -> String {
    let r = post(
        gw,
        "/api/v1/admin/enrolments",
        &request_with(
            "alice",
            &proof(gw, "alice", device(devices, "alice")),
            &principal(subject),
        ),
        None,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    field(&r, "id")
}

/// `subject` requested by alice and approved by bob: the approval response.
fn enrol(gw: &DurableGateway, devices: &Devices, subject: &str) -> Response {
    let id = request(gw, devices, subject);
    let r = post(
        gw,
        "/api/v1/admin/enrolments/approve",
        &approve_body("bob", &proof(gw, "bob", device(devices, "bob")), &id, subject),
        None,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    r
}

/// The custody roots' sealers, as the gateway builds them at boot.
fn sealer(keystore: &Path) -> Box<dyn Sealer> {
    let keystore = Keystore::read(keystore).unwrap();
    Box::new(KmsSealer::new(Kms::new(
        fips_provider(),
        keystore.trust_root(),
    )))
}

/// The dual-control service over `state`, opened as the gateway opens it.
fn privilege(state: &StateDir) -> DurablePrivilege {
    let keystore = Keystore::read(&state.audit_keystore()).unwrap();
    let verifying_key = keystore.verifying_key.clone();
    let alg = keystore.alg;
    let root = keystore.into_signing_root().unwrap();
    let signing_key = SigningKey::new(
        alg,
        SecretBytes::new(root.expose_signing_key().expose().to_vec()),
    );
    DurablePrivilege::open(
        state.admin_privilege(),
        sealer(&state.audit_keystore()),
        sealer(&state.audit_keystore()),
        Box::new(FipsSig::new(alg)),
        signing_key,
        verifying_key,
        LedgerId(PRIVILEGE_LEDGER_ID.into()),
    )
    .unwrap()
}

/// Administration over `state`, opened as the gateway opens it at boot, for driving
/// directly with a directory write the test controls.
fn admin_control(state: &StateDir) -> AdminControl {
    let passkeys = DurableAdminPasskeys::open(
        state.admin_passkeys(),
        sealer(&state.idp_keystore()),
        fips_provider(),
    )
    .unwrap();
    let roles = DurableIdentityDirectory::open(
        state.admin_roles(),
        sealer(&state.idp_keystore()),
        RoleCatalog::standard(),
    )
    .unwrap();
    AdminControl::open(
        passkeys,
        roles,
        privilege(state),
        state.admin_pending(),
        sealer(&state.audit_keystore()),
    )
    .unwrap()
}

/// A challenge from `control` for the administrator `name`, answered with `device`.
fn direct_proof(
    control: &mut AdminControl,
    name: &str,
    device: &SoftwareAdminAuthenticator,
) -> (String, String) {
    let body = parse(&format!(r#"{{"admin":"{name}"}}"#)).unwrap();
    let challenge = control
        .challenge(&body, NOW)
        .expect("a challenge")
        .str_field("challenge")
        .unwrap()
        .to_string();
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

#[test]
fn an_enrolment_needs_a_second_administrator_and_survives_a_restart() {
    let dir = scratch("dual");
    let (state, devices) = provision(&dir);
    let device = |name: &str| &devices.iter().find(|(n, _)| *n == name).unwrap().1;

    let id = {
        let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
        assert_eq!(served.administrators, 3);
        let gw = &served.gateway;

        // Alice requests; the enrolment is parked and olivia cannot log in.
        let alice = proof(gw, "alice", device("alice"));
        let r = post(
            gw,
            "/api/v1/admin/enrolments",
            &request_body("alice", &alice),
            None,
        );
        assert_eq!(r.status, 200, "{}", r.body);
        let id = parse(&r.body).unwrap().str_field("id").unwrap().to_string();
        assert_eq!(login(gw).status, 401, "not enrolled until approved");

        // The spent challenge cannot be replayed.
        let replay = post(
            gw,
            "/api/v1/admin/enrolments/approve",
            &approve_body("bob", &alice, &id, "olivia"),
            None,
        );
        assert_eq!(replay.status, 401, "{}", replay.body);

        // Alice cannot approve her own request; the auditor cannot approve it at all.
        let r = post(
            gw,
            "/api/v1/admin/enrolments/approve",
            &approve_body("alice", &proof(gw, "alice", device("alice")), &id, "olivia"),
            None,
        );
        assert_eq!(r.status, 403, "{}", r.body);
        // The privilege service's reason stays in the gateway's log. [VLD-ANB-MIDGARD-0053]
        assert_eq!(r.body, r#"{"error":"refused"}"#);
        let r = post(
            gw,
            "/api/v1/admin/enrolments/approve",
            &approve_body("audra", &proof(gw, "audra", device("audra")), &id, "olivia"),
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
        &approve_body("bob", &proof(gw, "bob", device("bob")), &id, "olivia"),
        None,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(login(gw).status, 401, "not until olivia activates");
    let activated = activate_at(gw, "olivia", &field(&r, "activationCode"), NOW);
    assert_eq!(activated.status, 200, "{}", activated.body);
    let totp = totp_of(&field(&activated, "otpauth")).code_at(NOW);
    let ok = login_as(gw, "olivia", Some(&totp));
    assert_eq!(ok.status, 200, "olivia logs in once activated: {}", ok.body);

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
        ("olivia", "ACTIVATE succeeded"),
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

/// [AC-15.1] [VLD-ANB-MIDGARD-0002] A request that carries a password hash or a TOTP
/// secret for the principal is refused, and nothing is parked.
#[test]
fn a_requester_cannot_supply_credential_material() {
    let dir = scratch("material");
    let (state, devices) = provision(&dir);
    {
        let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
        let gw = &served.gateway;
        let base = principal("olivia");
        for extra in [
            r#""passwordHash":"pbkdf2-sha256$i=1$00$00""#,
            r#""totpSecret":"00""#,
        ] {
            let carrying = format!("{},{extra}}}", base.trim_end_matches('}'));
            let r = post(
                gw,
                "/api/v1/admin/enrolments",
                &request_with("alice", &proof(gw, "alice", device(&devices, "alice")), &carrying),
                None,
            );
            assert_eq!(r.status, 400, "{extra}: {}", r.body);
            assert!(
                r.body.contains("unknown") || r.body.contains("not allowed"),
                "{}",
                r.body
            );
        }
        let listed = post(
            gw,
            "/api/v1/admin/enrolments/list",
            &list_body("bob", &proof(gw, "bob", device(&devices, "bob"))),
            None,
        );
        assert_eq!(listed.status, 200, "{}", listed.body);
        assert!(
            listed.body.contains(r#""enrolments":[]"#),
            "{}",
            listed.body
        );
    }
    assert_eq!(admin_control(&state).parked(), 0, "nothing was parked");
    let _ = std::fs::remove_dir_all(&dir);
}

/// [AC-15.2] [VLD-ANB-MIDGARD-0002] The approval returns a single-use activation code
/// and never the second factor; the directory holds no usable password and an
/// unconfirmed TOTP factor; the request's response carried neither.
#[test]
fn approval_returns_an_activation_code_and_no_second_factor_secret() {
    let dir = scratch("minted");
    let (state, devices) = provision(&dir);
    {
        let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
        let gw = &served.gateway;
        let requested = post(
            gw,
            "/api/v1/admin/enrolments",
            &request_body("alice", &proof(gw, "alice", device(&devices, "alice"))),
            None,
        );
        assert_eq!(requested.status, 200, "{}", requested.body);
        for secret in ["activationCode", "otpauth://", "totpSecret", "passwordHash"] {
            assert!(!requested.body.contains(secret), "{}", requested.body);
        }

        let id = field(&requested, "id");
        // The approver states what they approve: another subject is refused.
        let r = post(
            gw,
            "/api/v1/admin/enrolments/approve",
            &approve_body("bob", &proof(gw, "bob", device(&devices, "bob")), &id, "mallory"),
            None,
        );
        assert_eq!(r.status, 409, "{}", r.body);

        let approved = post(
            gw,
            "/api/v1/admin/enrolments/approve",
            &approve_body("bob", &proof(gw, "bob", device(&devices, "bob")), &id, "olivia"),
            None,
        );
        assert_eq!(approved.status, 200, "{}", approved.body);
        let code = field(&approved, "activationCode");
        assert!(
            code.len() == 32 && code.chars().all(|c| c.is_ascii_hexdigit()),
            "a 128-bit code: {code}"
        );
        assert!(
            approved.body.contains(&format!(
                r#""activationExpiresAt":{}"#,
                NOW + ACTIVATION_TTL
            )),
            "{}",
            approved.body
        );
        assert!(!approved.body.contains("otpauth://"), "{}", approved.body);
        assert!(!approved.body.contains("totpSecret"), "{}", approved.body);
        assert_eq!(login(gw).status, 401, "nothing logs in before activation");
    }

    let directory = DurableDirectory::open(state.directory(), sealer(&state.idp_keystore()))
        .expect("the directory opens");
    let entry = directory.get("olivia").expect("olivia is enrolled");
    assert!(
        entry.credentials.password_hash.is_empty(),
        "no usable password"
    );
    assert!(entry.credentials.totp.is_some(), "a TOTP factor was minted");
    let activation = entry.credentials.activation.as_ref().expect("pending");
    assert!(!activation.totp_confirmed);
    assert_eq!(activation.expires_at, NOW + ACTIVATION_TTL);
    let _ = std::fs::remove_dir_all(&dir);
}

/// [AC-15.3] [VLD-ANB-MIDGARD-0002] The activation code sets the principal's password
/// once and hands them the TOTP factor; the first login needs both factors; a code
/// past its 72 hours activates nothing.
#[test]
fn activation_is_single_use_and_first_login_needs_both_factors() {
    let dir = scratch("activate");
    let (state, devices) = provision(&dir);
    let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
    let gw = &served.gateway;
    let code = field(&enrol(gw, &devices, "olivia"), "activationCode");

    // A password too short is refused before the code is looked at.
    let short = post(
        gw,
        "/api/v1/activate",
        &format!(r#"{{"username":"olivia","activationCode":"{code}","password":"short"}}"#),
        None,
    );
    assert_eq!(short.status, 400, "{}", short.body);
    // A wrong code is the uniform refusal.
    let wrong = activate_at(gw, "olivia", &"0".repeat(32), NOW);
    assert_eq!(wrong.status, 401, "{}", wrong.body);

    let first = activate_at(gw, "olivia", &code, NOW);
    assert_eq!(first.status, 200, "{}", first.body);
    let uri = field(&first, "otpauth");
    assert!(uri.starts_with("otpauth://totp/Midgard:olivia?"), "{uri}");
    let again = activate_at(gw, "olivia", &code, NOW);
    assert_eq!(again.status, 401, "single use: {}", again.body);
    assert!(again.body.contains("invalid activation"), "{}", again.body);

    assert_eq!(login(gw).status, 401, "the password alone is not enough");
    let totp = totp_of(&uri).code_at(NOW);
    let ok = login_as(gw, "olivia", Some(&totp));
    assert_eq!(ok.status, 200, "{}", ok.body);

    // A fresh approval, its code presented a second after its 72 hours.
    let late = field(&enrol(gw, &devices, "oscar"), "activationCode");
    let expired = activate_at(gw, "oscar", &late, NOW + 72 * 3600 + 1);
    assert_eq!(expired.status, 401, "{}", expired.body);
    drop(served);
    let _ = std::fs::remove_dir_all(&dir);
}

/// [AC-15.4] [VLD-ANB-MIDGARD-0002] Roles and purposes outside the served policy are
/// refused with the catalogue named, compartments are pattern-checked, and the
/// approver's listing shows the whole record.
#[test]
fn roles_and_purposes_outside_the_policy_are_refused_and_the_approver_sees_the_full_record() {
    let dir = scratch("catalogue");
    let (state, devices) = provision(&dir);
    let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
    let gw = &served.gateway;
    let principal_with = |roles: &str, purposes: &str, compartments: &str| {
        format!(
            r#"{{"subject":"olivia","roles":[{roles}],"clearance":"restricted","purposes":[{purposes}],"compartments":[{compartments}]}}"#
        )
    };
    for (body, named) in [
        (
            principal_with(r#""zz-unknown-role""#, r#""operations""#, ""),
            "zz-unknown-role",
        ),
        (
            principal_with(r#""operator""#, r#""zz-unknown-purpose""#, ""),
            "zz-unknown-purpose",
        ),
        (
            principal_with(r#""operator""#, r#""operations""#, r#""no spaces""#),
            "no spaces",
        ),
    ] {
        let r = post(
            gw,
            "/api/v1/admin/enrolments",
            &request_with("alice", &proof(gw, "alice", device(&devices, "alice")), &body),
            None,
        );
        assert_eq!(r.status, 400, "{}", r.body);
        assert!(r.body.contains(named), "{}", r.body);
        if named != "no spaces" {
            // The refusal names what may be granted: the policy's role, the login
            // purpose.
            assert!(
                r.body.contains("operator") || r.body.contains("operations"),
                "{}",
                r.body
            );
        }
    }

    let valid = principal_with(r#""operator""#, r#""operations""#, r#""ops-1""#);
    let r = post(
        gw,
        "/api/v1/admin/enrolments",
        &request_with("alice", &proof(gw, "alice", device(&devices, "alice")), &valid),
        None,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let listed = post(
        gw,
        "/api/v1/admin/enrolments/list",
        &list_body("bob", &proof(gw, "bob", device(&devices, "bob"))),
        None,
    );
    assert_eq!(listed.status, 200, "{}", listed.body);
    for shown in [
        r#""subject":"olivia""#,
        r#""requestedBy":"alice""#,
        r#""roles":["operator"]"#,
        r#""clearance":"restricted""#,
        r#""compartments":["ops-1"]"#,
        r#""purposes":["operations"]"#,
        r#""justification":"new operator""#,
        r#""mfa":"totp at activation""#,
    ] {
        assert!(listed.body.contains(shown), "{shown} in {}", listed.body);
    }
    drop(served);
    let _ = std::fs::remove_dir_all(&dir);
}

/// [AC-15.5] [AC-15.6] [VLD-ANB-MIDGARD-0066] A directory write that fails at approval
/// leaves the enrolment parked for its approver to complete, without a second
/// privilege approval; and a parked enrolment can be rejected by another
/// administrator, which settles it and frees the subject.
#[test]
fn a_failed_approval_can_be_retried_and_a_parked_enrolment_can_be_rejected() {
    let dir = scratch("recover");
    let (state, devices) = provision(&dir);
    let catalogue = RoleCatalogue::from_policy(
        &http_gateway::policy_file::parse_policy(&std::fs::read_to_string(state.policy()).unwrap())
            .unwrap(),
        "operations",
    );
    let hasher = Pbkdf2Hasher::new(1_000);

    let id = {
        let mut control = admin_control(&state);
        let alice = direct_proof(&mut control, "alice", device(&devices, "alice"));
        let (requested, _) = control
            .request_enrolment(
                &parse(&request_body("alice", &alice)).unwrap(),
                &catalogue,
                &|_| false,
                NOW,
            )
            .expect("requested");
        let id = requested.str_field("id").unwrap().to_string();

        // The directory write fails once.
        let mut failing = true;
        let mut written: Vec<(String, Enrollment)> = Vec::new();
        let mut enrol = |subject: &str, enrollment: Enrollment| {
            if std::mem::take(&mut failing) {
                return Err("the disk is full".to_string());
            }
            written.push((subject.to_string(), enrollment));
            Ok(())
        };
        let approve = |control: &mut AdminControl, admin: &str| {
            let proof = direct_proof(control, admin, device(&devices, admin));
            parse(&approve_body(admin, &proof, &id, "olivia")).unwrap()
        };

        let body = approve(&mut control, "bob");
        let failed = control
            .approve_enrolment(&body, &hasher, &mut enrol, NOW)
            .expect_err("the write failed");
        assert_eq!(failed.status, 500, "{failed:?}");
        assert_eq!(control.parked(), 1, "the enrolment stays parked");
        let bob = direct_proof(&mut control, "bob", device(&devices, "bob"));
        let listing = control
            .list_enrolments(&parse(&list_body("bob", &bob)).unwrap(), NOW)
            .unwrap()
            .encode();
        assert!(
            listing.contains(&id) && listing.contains("approved by bob"),
            "{listing}"
        );

        // Only the administrator who approved it may complete it.
        let body = approve(&mut control, "audra");
        let other = control
            .approve_enrolment(&body, &hasher, &mut enrol, NOW)
            .expect_err("not the approver");
        assert_eq!(other.status, 403, "{other:?}");

        let body = approve(&mut control, "bob");
        let (retried, _, subject) = control
            .approve_enrolment(&body, &hasher, &mut enrol, NOW)
            .expect("the retry completes it");
        assert_eq!(subject, "olivia");
        assert!(retried.str_field("activationCode").is_some());
        assert_eq!(control.parked(), 0);
        assert_eq!(written.len(), 1, "enrolled exactly once");
        assert_eq!(written[0].0, "olivia");
        id
    };
    assert!(
        matches!(
            privilege(&state).service().state_of(&id),
            Some(ActionState::Authorised { approver: Some(a) }) if a.0 == "bob"
        ),
        "one privilege approval, by bob"
    );

    // Separately: a parked enrolment rejected through the gateway.
    let rejected = {
        let served = open_state_dir(&state, Vec::new(), &pin(&state)).expect("boots");
        let gw = &served.gateway;
        let id = request(gw, &devices, "oscar");
        // The auditor holds no enrolment capability, and the requester cannot
        // dispose of their own request.
        for admin in ["audra", "alice"] {
            let r = post(
                gw,
                "/api/v1/admin/enrolments/reject",
                &reject_body(admin, &proof(gw, admin, device(&devices, admin)), &id),
                None,
            );
            assert_eq!(r.status, 403, "{admin}: {}", r.body);
        }
        let r = post(
            gw,
            "/api/v1/admin/enrolments/reject",
            &reject_body("bob", &proof(gw, "bob", device(&devices, "bob")), &id),
            None,
        );
        assert_eq!(r.status, 200, "{}", r.body);
        assert!(r.body.contains(r#""rejected":"oscar""#), "{}", r.body);
        let listed = post(
            gw,
            "/api/v1/admin/enrolments/list",
            &list_body("bob", &proof(gw, "bob", device(&devices, "bob"))),
            None,
        );
        assert!(
            listed.body.contains(r#""enrolments":[]"#),
            "{}",
            listed.body
        );
        // The subject is free to be requested again.
        request(gw, &devices, "oscar");

        let ok = {
            let code = field(&enrol(gw, &devices, "olivia2"), "activationCode");
            let a = activate_at(gw, "olivia2", &code, NOW);
            let totp = totp_of(&field(&a, "otpauth")).code_at(NOW);
            login_as(gw, "olivia2", Some(&totp))
        };
        assert_eq!(ok.status, 200, "{}", ok.body);
        let audit = post(gw, "/api/v1/audit", "", Some(&field(&ok, "token")));
        assert!(
            audit
                .body
                .contains(r#""actor":"bob","action":"ENROLL rejected""#),
            "the rejection is audited: {}",
            audit.body
        );
        id
    };
    assert!(
        matches!(
            privilege(&state).service().state_of(&rejected),
            Some(ActionState::Rejected { approver, .. }) if approver.0 == "bob"
        ),
        "the privilege record is Rejected"
    );
    // Settled in the journal: after a restart only the re-request is parked.
    assert_eq!(admin_control(&state).parked(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// [AC-15.5] [VLD-ANB-MIDGARD-0066] A gateway without a durable identity directory
/// parks nothing it could never write.
#[test]
fn a_gateway_without_a_durable_directory_parks_nothing() {
    let dir = scratch("in-memory");
    let (state, devices) = provision(&dir);
    // Administration over a gateway whose logins live in memory.
    let gw = SecuredGateway::dev().with_admin(admin_control(&state));
    let call = |path: &str, body: String| {
        gw.handle(
            &Request {
                method: "POST".into(),
                path: path.into(),
                query: String::new(),
                body,
                authorization: None,
                origin: None,
            },
            NOW,
        )
    };
    let challenge = field(
        &call("/api/v1/admin/challenge", r#"{"admin":"alice"}"#.into()),
        "challenge",
    );
    let nonce: [u8; 32] = unhex(&challenge).try_into().unwrap();
    let assertion = device(&devices, "alice")
        .authenticate(&fips_provider(), &AdminChallenge::new(nonce), true)
        .unwrap();
    let proof = (
        challenge,
        format!(
            r#"{{"credentialId":"{}","userVerified":true,"signature":"{}"}}"#,
            assertion.credential_id,
            hex(assertion.signature.as_bytes())
        ),
    );
    let r = call("/api/v1/admin/enrolments", request_body("alice", &proof));
    assert_eq!(r.status, 501, "{}", r.body);
    drop(gw);
    assert_eq!(admin_control(&state).parked(), 0);
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
    // The client reads only `unauthorised`; why is logged under the request's
    // reference. [VLD-ANB-MIDGARD-0053]
    assert_eq!(r.body, r#"{"error":"unauthorised"}"#, "{}", r.body);
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
