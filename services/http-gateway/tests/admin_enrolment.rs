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
//!
//! Every administration request carries two factors: the passkey assertion and a
//! one-time code the gateway checks against the administrator's TOTP secret and
//! accepts once. Each test runs on its own clock, which moves on one 30 s step before
//! each administrator proof, so every proof has a code nobody has used.
//! [VLD-ANB-MIDGARD-0008] [AC-10.1] [AC-10.3]
#![cfg(feature = "backend-fips")]

use std::cell::Cell;
use std::path::{Path, PathBuf};

use admin_identity::{
    AdminChallenge, AdminId, DurableAdminPasskeys, DurableIdentityDirectory, ResourceScope,
    RoleBinding, RoleCatalog, SoftwareAdminAuthenticator, ROLE_AUDITOR, ROLE_IAM_ADMIN,
};
use admin_privilege::{ActionState, DurablePrivilege};
use attest::keystore::test_support::test_passphrase;
use attest::Keystore;
use attest::RootWrap;
use audit_writer::{LedgerId, Sealer};
use http_gateway::admin_api::{
    AdminControl, RoleCatalogue, ACTIVATION_TTL, MAX_CHALLENGES_PER_ADMIN,
};
use http_gateway::admin_totp::DurableAdminTotp;
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
use pqc_agility::{OsRng, Rng, SecretBytes, SigningKey};

const NOW: u64 = 1_800_000_000;
const PASSWORD: &str = "correct horse battery staple";
/// The TOTP time step, in seconds.
const STEP: u64 = 30;

thread_local! {
    /// This test's clock: it starts at [`NOW`] and only moves forward.
    static CLOCK: Cell<u64> = const { Cell::new(NOW) };
}

/// The time on this test's clock.
fn now() -> u64 {
    CLOCK.with(Cell::get)
}

/// Move this test's clock on one TOTP step, so the next administrator proof carries a
/// code for a step nobody has used.
fn next_step() -> u64 {
    CLOCK.with(|c| c.set(c.get() + STEP));
    now()
}

/// An administrator the test holds: their passkey and their TOTP secret.
struct Admin {
    device: SoftwareAdminAuthenticator,
    totp: TotpSecret,
}

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
/// three administrators: two IAM administrators and an auditor, each with a passkey and
/// a TOTP secret the test holds.
fn provision(dir: &Path) -> (StateDir, Devices) {
    let state = StateDir::new(dir);
    std::fs::create_dir_all(state.keys()).unwrap();
    std::fs::create_dir_all(state.trust()).unwrap();
    std::fs::create_dir_all(state.admin()).unwrap();
    let idp = Keystore::generate(IDP_KEY_ID, iam::IDP_SIG_ALG, &mut OsRng).unwrap();
    let audit = Keystore::generate(AUDIT_KEY_ID, governance::GOV_SIG_ALG, &mut OsRng).unwrap();
    idp.write(
        &state.idp_keystore(),
        &RootWrap::Passphrase(&test_passphrase()),
        &mut OsRng,
    )
    .unwrap();
    audit
        .write(
            &state.audit_keystore(),
            &RootWrap::Passphrase(&test_passphrase()),
            &mut OsRng,
        )
        .unwrap();
    // What `mg-admin init` also creates: the ledger and its signed checkpoint.
    http_gateway::boot::init_audit_ledger(&state, &RootWrap::Passphrase(&test_passphrase()))
        .unwrap();

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
    let mut totp = DurableAdminTotp::open(state.admin_totp(), sealer()).unwrap();
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
        let mut secret = vec![0u8; 20];
        OsRng.fill(&mut secret);
        totp.enrol(admin, TotpSecret::new(secret.clone())).unwrap();
        devices.push((
            admin,
            Admin {
                device,
                totp: TotpSecret::new(secret),
            },
        ));
    }
    assert_eq!(totp.administrators_with_totp(), 3);
    (state, devices)
}

fn post(gw: &DurableGateway, path: &str, body: &str, token: Option<&str>) -> Response {
    post_at(gw, path, body, token, now())
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
            forwarded_for: None,
        },
        now,
    )
}

/// What an administration request proves itself with: the `challenge` it answers, the
/// passkey `assertion`, and the fields after them (`"totp":"…"`, or nothing).
type Proof = (String, String, String);

/// The `challenge`, `assertion` and `totp` fields for `admin`, answering `challenge`,
/// issued for `purpose`, with their passkey (claiming user verification as
/// `user_verified` says) and the code for the current step.
fn answer(admin: &Admin, challenge: String, user_verified: bool, purpose: &str) -> Proof {
    let nonce: [u8; 32] = unhex(&challenge).try_into().unwrap();
    let assertion = admin
        .device
        .authenticate(
            &fips_provider(),
            &AdminChallenge::new(nonce).for_purpose(purpose),
            user_verified,
        )
        .unwrap();
    (
        challenge,
        format!(
            r#"{{"credentialId":"{}","userVerified":{user_verified},"signature":"{}"}}"#,
            assertion.credential_id,
            hex(assertion.signature.as_bytes())
        ),
        format!(r#","totp":"{}""#, admin.totp.code_at(now())),
    )
}

/// A fresh challenge issued to `name` for `purpose`, answered by `admin` at the
/// current step.
fn proof_now(
    gw: &DurableGateway,
    name: &str,
    admin: &Admin,
    user_verified: bool,
    purpose: &str,
) -> Proof {
    answer(
        admin,
        fetch_challenge(gw, name, purpose),
        user_verified,
        purpose,
    )
}

/// Fetch a challenge for `admin` to answer for `purpose` (`request`, `approve`,
/// `reject` or `list`): the nonce, as hex. The gateway binds it to the administrator
/// named and the operation. [VLD-ANB-MIDGARD-0020]
fn fetch_challenge(gw: &DurableGateway, admin: &str, purpose: &str) -> String {
    let r = post(
        gw,
        "/api/v1/admin/challenge",
        &format!(r#"{{"admin":"{admin}","purpose":"{purpose}"}}"#),
        None,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    parse(&r.body)
        .unwrap()
        .str_field("challenge")
        .unwrap()
        .to_string()
}

/// Move to the next step, fetch a challenge for `purpose` and answer it with `admin`'s
/// passkey and one-time code: the fields an administration request carries.
fn proof(gw: &DurableGateway, name: &str, admin: &Admin, purpose: &str) -> Proof {
    next_step();
    proof_now(gw, name, admin, true, purpose)
}

/// `proof` without its one-time code: the passkey alone.
fn passkey_only(proof: Proof) -> Proof {
    (proof.0, proof.1, String::new())
}

/// The principal every test enrols unless it says otherwise: who they are and what
/// they may do, and nothing about how they authenticate.
fn principal(subject: &str) -> String {
    format!(
        r#"{{"subject":"{subject}","roles":["operator"],"clearance":"internal","purposes":["operations"]}}"#
    )
}

fn request_with(admin: &str, proof: &Proof, principal: &str) -> String {
    format!(
        r#"{{"admin":"{admin}","challenge":"{}","assertion":{}{},"justification":"new operator","principal":{principal}}}"#,
        proof.0, proof.1, proof.2
    )
}

fn request_body(admin: &str, proof: &Proof) -> String {
    request_with(admin, proof, &principal("olivia"))
}

fn approve_body(admin: &str, proof: &Proof, id: &str, subject: &str) -> String {
    format!(
        r#"{{"admin":"{admin}","challenge":"{}","assertion":{}{},"id":"{id}","subject":"{subject}"}}"#,
        proof.0, proof.1, proof.2
    )
}

fn reject_body(admin: &str, proof: &Proof, id: &str) -> String {
    format!(
        r#"{{"admin":"{admin}","challenge":"{}","assertion":{}{},"id":"{id}","reason":"not a known starter"}}"#,
        proof.0, proof.1, proof.2
    )
}

fn list_body(admin: &str, proof: &Proof) -> String {
    format!(
        r#"{{"admin":"{admin}","challenge":"{}","assertion":{}{}}}"#,
        proof.0, proof.1, proof.2
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

type Devices = Vec<(&'static str, Admin)>;

fn device<'a>(devices: &'a Devices, name: &str) -> &'a Admin {
    &devices.iter().find(|(n, _)| *n == name).unwrap().1
}

/// `subject` requested by alice: the enrolment id.
fn request(gw: &DurableGateway, devices: &Devices, subject: &str) -> String {
    let r = post(
        gw,
        "/api/v1/admin/enrolments",
        &request_with(
            "alice",
            &proof(gw, "alice", device(devices, "alice"), "request"),
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
        &approve_body(
            "bob",
            &proof(gw, "bob", device(devices, "bob"), "approve"),
            &id,
            subject,
        ),
        None,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    r
}

/// The custody roots' sealers, as the gateway builds them at boot.
fn sealer(keystore: &Path) -> Box<dyn Sealer> {
    let keystore = Keystore::read(keystore, &RootWrap::Passphrase(&test_passphrase())).unwrap();
    Box::new(KmsSealer::new(Kms::new(
        fips_provider(),
        keystore.trust_root(),
    )))
}

/// The dual-control service over `state`, opened as the gateway opens it.
fn privilege(state: &StateDir) -> DurablePrivilege {
    let keystore = Keystore::read(
        &state.audit_keystore(),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .unwrap();
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
    let totp = DurableAdminTotp::open(state.admin_totp(), sealer(&state.idp_keystore())).unwrap();
    let roles = DurableIdentityDirectory::open(
        state.admin_roles(),
        sealer(&state.idp_keystore()),
        RoleCatalog::standard(),
    )
    .unwrap();
    AdminControl::open(
        passkeys,
        totp,
        roles,
        privilege(state),
        state.admin_pending(),
        sealer(&state.audit_keystore()),
    )
    .unwrap()
}

/// A challenge from `control` at the next step, issued to `name` for `purpose` and
/// answered by `admin`.
fn direct_proof(control: &mut AdminControl, name: &str, admin: &Admin, purpose: &str) -> Proof {
    let body = parse(&format!(r#"{{"admin":"{name}","purpose":"{purpose}"}}"#)).unwrap();
    let challenge = control
        .challenge(&body, next_step())
        .expect("a challenge")
        .str_field("challenge")
        .unwrap()
        .to_string();
    answer(admin, challenge, true, purpose)
}

#[test]
fn an_enrolment_needs_a_second_administrator_and_survives_a_restart() {
    let dir = scratch("dual");
    let (state, devices) = provision(&dir);
    let device = |name: &str| &devices.iter().find(|(n, _)| *n == name).unwrap().1;

    let id = {
        let served = open_state_dir(
            &state,
            Vec::new(),
            &pin(&state),
            &RootWrap::Passphrase(&test_passphrase()),
        )
        .expect("boots");
        assert_eq!(served.administrators, 3);
        assert_eq!(served.administrators_with_totp, 3);
        let gw = &served.gateway;

        // Alice requests; the enrolment is parked and olivia cannot log in.
        let alice = proof(gw, "alice", device("alice"), "request");
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
            &approve_body(
                "alice",
                &proof(gw, "alice", device("alice"), "approve"),
                &id,
                "olivia",
            ),
            None,
        );
        assert_eq!(r.status, 403, "{}", r.body);
        // The privilege service's reason stays in the gateway's log. [VLD-ANB-MIDGARD-0053]
        assert_eq!(r.body, r#"{"error":"refused"}"#);
        let r = post(
            gw,
            "/api/v1/admin/enrolments/approve",
            &approve_body(
                "audra",
                &proof(gw, "audra", device("audra"), "approve"),
                &id,
                "olivia",
            ),
            None,
        );
        assert_eq!(r.status, 403, "{}", r.body);
        assert_eq!(login(gw).status, 401);
        id
    }; // restart

    let served = open_state_dir(
        &state,
        Vec::new(),
        &pin(&state),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .expect("boots again");
    let gw = &served.gateway;
    let bob = proof(gw, "bob", device("bob"), "list");
    let listed = post(
        gw,
        "/api/v1/admin/enrolments/list",
        &list_body("bob", &bob),
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
        &approve_body(
            "bob",
            &proof(gw, "bob", device("bob"), "approve"),
            &id,
            "olivia",
        ),
        None,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(login(gw).status, 401, "not until olivia activates");
    let activated = activate_at(gw, "olivia", &field(&r, "activationCode"), now());
    assert_eq!(activated.status, 200, "{}", activated.body);
    let totp = totp_of(&field(&activated, "otpauth")).code_at(now());
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
        let served = open_state_dir(
            &state,
            Vec::new(),
            &pin(&state),
            &RootWrap::Passphrase(&test_passphrase()),
        )
        .expect("boots");
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
                &request_with(
                    "alice",
                    &proof(gw, "alice", device(&devices, "alice"), "request"),
                    &carrying,
                ),
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
            &list_body("bob", &proof(gw, "bob", device(&devices, "bob"), "list")),
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
        let served = open_state_dir(
            &state,
            Vec::new(),
            &pin(&state),
            &RootWrap::Passphrase(&test_passphrase()),
        )
        .expect("boots");
        let gw = &served.gateway;
        let requested = post(
            gw,
            "/api/v1/admin/enrolments",
            &request_body(
                "alice",
                &proof(gw, "alice", device(&devices, "alice"), "request"),
            ),
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
            &approve_body(
                "bob",
                &proof(gw, "bob", device(&devices, "bob"), "approve"),
                &id,
                "mallory",
            ),
            None,
        );
        assert_eq!(r.status, 409, "{}", r.body);

        let approved = post(
            gw,
            "/api/v1/admin/enrolments/approve",
            &approve_body(
                "bob",
                &proof(gw, "bob", device(&devices, "bob"), "approve"),
                &id,
                "olivia",
            ),
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
                now() + ACTIVATION_TTL
            )),
            "{}",
            approved.body
        );
        assert!(!approved.body.contains("otpauth://"), "{}", approved.body);
        assert!(!approved.body.contains("totpSecret"), "{}", approved.body);
        assert_eq!(login(gw).status, 401, "nothing logs in before activation");
    }
    let approved_at = now();

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
    assert_eq!(activation.expires_at, approved_at + ACTIVATION_TTL);
    let _ = std::fs::remove_dir_all(&dir);
}

/// [AC-15.3] [VLD-ANB-MIDGARD-0002] The activation code sets the principal's password
/// once and hands them the TOTP factor; the first login needs both factors; a code
/// past its 72 hours activates nothing.
#[test]
fn activation_is_single_use_and_first_login_needs_both_factors() {
    let dir = scratch("activate");
    let (state, devices) = provision(&dir);
    let served = open_state_dir(
        &state,
        Vec::new(),
        &pin(&state),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .expect("boots");
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
    let wrong = activate_at(gw, "olivia", &"0".repeat(32), now());
    assert_eq!(wrong.status, 401, "{}", wrong.body);

    let first = activate_at(gw, "olivia", &code, now());
    assert_eq!(first.status, 200, "{}", first.body);
    let uri = field(&first, "otpauth");
    assert!(uri.starts_with("otpauth://totp/Midgard:olivia?"), "{uri}");
    let again = activate_at(gw, "olivia", &code, now());
    assert_eq!(again.status, 401, "single use: {}", again.body);
    assert!(again.body.contains("invalid activation"), "{}", again.body);

    assert_eq!(login(gw).status, 401, "the password alone is not enough");
    let totp = totp_of(&uri).code_at(now());
    let ok = login_as(gw, "olivia", Some(&totp));
    assert_eq!(ok.status, 200, "{}", ok.body);

    // A fresh approval, its code presented a second after its 72 hours.
    let late = field(&enrol(gw, &devices, "oscar"), "activationCode");
    let expired = activate_at(gw, "oscar", &late, now() + 72 * 3600 + 1);
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
    let served = open_state_dir(
        &state,
        Vec::new(),
        &pin(&state),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .expect("boots");
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
            &request_with(
                "alice",
                &proof(gw, "alice", device(&devices, "alice"), "request"),
                &body,
            ),
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
        &request_with(
            "alice",
            &proof(gw, "alice", device(&devices, "alice"), "request"),
            &valid,
        ),
        None,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let listed = post(
        gw,
        "/api/v1/admin/enrolments/list",
        &list_body("bob", &proof(gw, "bob", device(&devices, "bob"), "list")),
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
        let alice = direct_proof(&mut control, "alice", device(&devices, "alice"), "request");
        let (requested, _) = control
            .request_enrolment(
                &parse(&request_body("alice", &alice)).unwrap(),
                &catalogue,
                &|_| false,
                now(),
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
            let proof = direct_proof(control, admin, device(&devices, admin), "approve");
            parse(&approve_body(admin, &proof, &id, "olivia")).unwrap()
        };

        let body = approve(&mut control, "bob");
        let failed = control
            .approve_enrolment(&body, &hasher, &mut enrol, now())
            .expect_err("the write failed");
        assert_eq!(failed.status, 500, "{failed:?}");
        assert_eq!(control.parked(), 1, "the enrolment stays parked");
        let bob = direct_proof(&mut control, "bob", device(&devices, "bob"), "list");
        let listing = control
            .list_enrolments(&parse(&list_body("bob", &bob)).unwrap(), now())
            .unwrap()
            .encode();
        assert!(
            listing.contains(&id) && listing.contains("approved by bob"),
            "{listing}"
        );

        // Only the administrator who approved it may complete it.
        let body = approve(&mut control, "audra");
        let other = control
            .approve_enrolment(&body, &hasher, &mut enrol, now())
            .expect_err("not the approver");
        assert_eq!(other.status, 403, "{other:?}");

        let body = approve(&mut control, "bob");
        let (retried, _, subject) = control
            .approve_enrolment(&body, &hasher, &mut enrol, now())
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
        let served = open_state_dir(
            &state,
            Vec::new(),
            &pin(&state),
            &RootWrap::Passphrase(&test_passphrase()),
        )
        .expect("boots");
        let gw = &served.gateway;
        let id = request(gw, &devices, "oscar");
        // The auditor holds no enrolment capability, and the requester cannot
        // dispose of their own request.
        for admin in ["audra", "alice"] {
            let r = post(
                gw,
                "/api/v1/admin/enrolments/reject",
                &reject_body(
                    admin,
                    &proof(gw, admin, device(&devices, admin), "reject"),
                    &id,
                ),
                None,
            );
            assert_eq!(r.status, 403, "{admin}: {}", r.body);
        }
        let r = post(
            gw,
            "/api/v1/admin/enrolments/reject",
            &reject_body(
                "bob",
                &proof(gw, "bob", device(&devices, "bob"), "reject"),
                &id,
            ),
            None,
        );
        assert_eq!(r.status, 200, "{}", r.body);
        assert!(r.body.contains(r#""rejected":"oscar""#), "{}", r.body);
        let listed = post(
            gw,
            "/api/v1/admin/enrolments/list",
            &list_body("bob", &proof(gw, "bob", device(&devices, "bob"), "list")),
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
            let a = activate_at(gw, "olivia2", &code, now());
            let totp = totp_of(&field(&a, "otpauth")).code_at(now());
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
                forwarded_for: None,
            },
            now(),
        )
    };
    next_step();
    let challenge = field(
        &call(
            "/api/v1/admin/challenge",
            r#"{"admin":"alice","purpose":"request"}"#.into(),
        ),
        "challenge",
    );
    let proof = answer(device(&devices, "alice"), challenge, true, "request");
    let r = call("/api/v1/admin/enrolments", request_body("alice", &proof));
    assert_eq!(r.status, 501, "{}", r.body);
    drop(gw);
    assert_eq!(admin_control(&state).parked(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// [AC-10.1] [VLD-ANB-MIDGARD-0008] A registered administrator whose passkey signs a
/// fresh challenge claiming user verification, with no one-time code, is refused with
/// 401 on every route, and each refusal is in the ledger as ADMIN refused (401); a
/// wrong code is the same refusal. The user-verification flag is a hint: with the
/// right code an assertion claiming none is accepted, and enrolment events record the
/// factors verified. [AC-10.2]
#[test]
fn a_passkey_without_a_second_factor_is_refused() {
    let dir = scratch("passkey-alone");
    let (state, devices) = provision(&dir);
    let served = open_state_dir(
        &state,
        Vec::new(),
        &pin(&state),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .expect("boots");
    let gw = &served.gateway;
    let alice = device(&devices, "alice");
    let bob = device(&devices, "bob");
    let id = request(gw, &devices, "olivia");

    for (path, body) in [
        (
            "/api/v1/admin/enrolments",
            request_with(
                "alice",
                &passkey_only(proof(gw, "alice", alice, "request")),
                &principal("oscar"),
            ),
        ),
        (
            "/api/v1/admin/enrolments/approve",
            approve_body(
                "bob",
                &passkey_only(proof(gw, "bob", bob, "approve")),
                &id,
                "olivia",
            ),
        ),
        (
            "/api/v1/admin/enrolments/list",
            list_body("bob", &passkey_only(proof(gw, "bob", bob, "list"))),
        ),
    ] {
        let r = post(gw, path, &body, None);
        assert_eq!(r.status, 401, "{path}: {}", r.body);
        // The client reads `unauthorised`; "second factor required" is logged.
        // [VLD-ANB-MIDGARD-0053]
        assert_eq!(r.body, r#"{"error":"unauthorised"}"#, "{}", r.body);
    }
    // A wrong code is refused the same way, and a malformed one too.
    for code in ["000000", "12345"] {
        let (challenge, assertion, _) = proof(gw, "bob", bob, "list");
        let wrong = (challenge, assertion, format!(r#","totp":"{code}""#));
        let r = post(
            gw,
            "/api/v1/admin/enrolments/list",
            &list_body("bob", &wrong),
            None,
        );
        assert_eq!(r.status, 401, "{}", r.body);
        // The client reads `unauthorised`; "second factor required" is logged.
        // [VLD-ANB-MIDGARD-0053]
        assert_eq!(r.body, r#"{"error":"unauthorised"}"#, "{}", r.body);
    }
    assert_eq!(admin_control_parked(gw, bob), 1, "nothing more was parked");

    // The user-verification flag is not a factor either way: with the right code an
    // assertion claiming none is accepted.
    next_step();
    let r = post(
        gw,
        "/api/v1/admin/enrolments/approve",
        &approve_body(
            "bob",
            &proof_now(gw, "bob", bob, false, "approve"),
            &id,
            "olivia",
        ),
        None,
    );
    assert_eq!(r.status, 200, "{}", r.body);

    let activated = activate_at(gw, "olivia", &field(&r, "activationCode"), now());
    let totp = totp_of(&field(&activated, "otpauth")).code_at(now());
    let ok = login_as(gw, "olivia", Some(&totp));
    assert_eq!(ok.status, 200, "{}", ok.body);
    let audit = post(gw, "/api/v1/audit", "", Some(&field(&ok, "token")));
    assert_eq!(audit.status, 200, "{}", audit.body);
    for actor in ["alice", "bob"] {
        assert!(
            audit.body.contains(&format!(
                r#""actor":"{actor}","action":"ADMIN refused (401)""#
            )),
            "{actor}'s refusal is audited: {}",
            audit.body
        );
    }
    for evidence in [
        "principal:olivia factors=totp+webauthn uv-claimed=true",
        "principal:olivia factors=totp+webauthn uv-claimed=false",
    ] {
        assert!(audit.body.contains(evidence), "{evidence}: {}", audit.body);
    }
    drop(served);
    let _ = std::fs::remove_dir_all(&dir);
}

/// [VLD-ANB-MIDGARD-0002] [VLD-ANB-MIDGARD-0066] [AC-15.4] [AC-AD-3] Nothing about the
/// served policy or the parked enrolments is told to a caller who has not proved
/// themselves: with a bad proof, a role outside the policy and an enrolment id that is
/// not waiting are refused 401 like any other request, not 400 or 404. Once the
/// administrator has proved themselves and holds the approving capability, they are
/// told; an auditor, signed in but not qualified, is refused 403 before the id is
/// looked up.
#[test]
fn an_unproven_caller_learns_nothing_about_roles_or_enrolments() {
    let dir = scratch("unproven");
    let (state, devices) = provision(&dir);
    let served = open_state_dir(
        &state,
        Vec::new(),
        &pin(&state),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .expect("boots");
    let gw = &served.gateway;
    let alice = device(&devices, "alice");
    let bob = device(&devices, "bob");
    let audra = device(&devices, "audra");
    // bob's passkey answering a challenge issued to alice: a proof that does not verify.
    let forged = |purpose: &str| {
        next_step();
        let mut proof = answer(bob, fetch_challenge(gw, "alice", purpose), true, purpose);
        proof.2 = format!(r#","totp":"{}""#, alice.totp.code_at(now()));
        proof
    };
    let unknown_role = r#"{"subject":"olivia","roles":["zz-unknown-role"],"clearance":"internal","purposes":["operations"]}"#;
    let missing = "enrol-nobody-1";
    for (path, body) in [
        (
            "/api/v1/admin/enrolments",
            request_with("alice", &forged("request"), unknown_role),
        ),
        (
            "/api/v1/admin/enrolments/approve",
            approve_body("alice", &forged("approve"), missing, "nobody"),
        ),
        (
            "/api/v1/admin/enrolments/reject",
            reject_body("alice", &forged("reject"), missing),
        ),
    ] {
        let r = post(gw, path, &body, None);
        assert_eq!(r.status, 401, "{path}: {}", r.body);
        assert_eq!(r.body, r#"{"error":"unauthorised"}"#, "{path}: {}", r.body);
    }

    // Signed in, but not qualified to approve: refused before the id is looked up.
    for (path, body) in [
        (
            "/api/v1/admin/enrolments/approve",
            approve_body(
                "audra",
                &proof(gw, "audra", audra, "approve"),
                missing,
                "nobody",
            ),
        ),
        (
            "/api/v1/admin/enrolments/reject",
            reject_body("audra", &proof(gw, "audra", audra, "reject"), missing),
        ),
    ] {
        let r = post(gw, path, &body, None);
        assert_eq!(r.status, 403, "{path}: {}", r.body);
    }

    // Proved and qualified, the administrator is told what is wrong.
    let r = post(
        gw,
        "/api/v1/admin/enrolments",
        &request_with("alice", &proof(gw, "alice", alice, "request"), unknown_role),
        None,
    );
    assert_eq!(r.status, 400, "{}", r.body);
    assert!(r.body.contains("zz-unknown-role"), "{}", r.body);
    for (path, body) in [
        (
            "/api/v1/admin/enrolments/approve",
            approve_body("bob", &proof(gw, "bob", bob, "approve"), missing, "nobody"),
        ),
        (
            "/api/v1/admin/enrolments/reject",
            reject_body("bob", &proof(gw, "bob", bob, "reject"), missing),
        ),
    ] {
        let r = post(gw, path, &body, None);
        assert_eq!(r.status, 404, "{path}: {}", r.body);
    }
    assert_eq!(admin_control_parked(gw, bob), 0, "nothing was parked");
    drop(served);
    let _ = std::fs::remove_dir_all(&dir);
}

/// [VLD-ANB-MIDGARD-0020] [AC-AD-3] [AC-AD-4] A challenge is fetched for one
/// operation and answers that operation only: bob's challenge for `list`, answered
/// honestly, cannot approve; signed as though it were for `approve`, it cannot either;
/// nor can a challenge fetched for `approve` whose assertion signs `list`. Each was
/// spent trying. A challenge fetched for `approve`, and signed for it, approves.
#[test]
fn a_challenge_for_one_operation_cannot_be_used_for_another() {
    let dir = scratch("purpose");
    let (state, devices) = provision(&dir);
    let served = open_state_dir(
        &state,
        Vec::new(),
        &pin(&state),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .expect("boots");
    let gw = &served.gateway;
    let bob = device(&devices, "bob");
    let id = request(gw, &devices, "olivia");

    next_step();
    let honest = proof_now(gw, "bob", bob, true, "list");
    next_step();
    let relabelled = answer(bob, fetch_challenge(gw, "bob", "list"), true, "approve");
    next_step();
    let signed_for_list = answer(bob, fetch_challenge(gw, "bob", "approve"), true, "list");
    for proof in [&honest, &relabelled, &signed_for_list] {
        let r = post(
            gw,
            "/api/v1/admin/enrolments/approve",
            &approve_body("bob", proof, &id, "olivia"),
            None,
        );
        assert_eq!(r.status, 401, "{}", r.body);
        assert_eq!(r.body, r#"{"error":"unauthorised"}"#, "{}", r.body);
        // The challenge was consumed: it does not list either.
        let r = post(
            gw,
            "/api/v1/admin/enrolments/list",
            &list_body("bob", proof),
            None,
        );
        assert_eq!(r.status, 401, "{}", r.body);
    }
    assert_eq!(admin_control_parked(gw, bob), 1, "still waiting");

    let r = post(
        gw,
        "/api/v1/admin/enrolments/approve",
        &approve_body("bob", &proof(gw, "bob", bob, "approve"), &id, "olivia"),
        None,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    drop(served);
    let _ = std::fs::remove_dir_all(&dir);
}

/// How many enrolments are parked, as bob lists them.
fn admin_control_parked(gw: &DurableGateway, bob: &Admin) -> usize {
    let listed = post(
        gw,
        "/api/v1/admin/enrolments/list",
        &list_body("bob", &proof(gw, "bob", bob, "list")),
        None,
    );
    assert_eq!(listed.status, 200, "{}", listed.body);
    listed.body.matches(r#""subject":"#).count()
}

/// [AC-10.3] [VLD-ANB-MIDGARD-0008] [VLD-ANB-MIDGARD-0026] An administrator's one-time
/// code is accepted once: the same code on a second fresh challenge within its step is
/// refused, after a restart too, and the next step's code is accepted.
#[test]
fn an_administrator_totp_code_is_single_use() {
    let dir = scratch("admin-totp-once");
    let (state, devices) = provision(&dir);
    let bob = device(&devices, "bob");
    next_step();
    {
        let served = open_state_dir(
            &state,
            Vec::new(),
            &pin(&state),
            &RootWrap::Passphrase(&test_passphrase()),
        )
        .expect("boots");
        let gw = &served.gateway;
        let list = |proof: &Proof| {
            post(
                gw,
                "/api/v1/admin/enrolments/list",
                &list_body("bob", proof),
                None,
            )
        };
        let first = proof_now(gw, "bob", bob, true, "list");
        let second = proof_now(gw, "bob", bob, true, "list");
        assert_ne!(first.0, second.0, "two fresh challenges");
        assert_eq!(first.2, second.2, "one code");
        let r = list(&first);
        assert_eq!(r.status, 200, "{}", r.body);
        let r = list(&second);
        assert_eq!(r.status, 401, "the code is spent: {}", r.body);
        // The client reads `unauthorised`; "second factor required" is logged.
        // [VLD-ANB-MIDGARD-0053]
        assert_eq!(r.body, r#"{"error":"unauthorised"}"#, "{}", r.body);
    } // restart

    let served = open_state_dir(
        &state,
        Vec::new(),
        &pin(&state),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .expect("boots again");
    let gw = &served.gateway;
    let r = post(
        gw,
        "/api/v1/admin/enrolments/list",
        &list_body("bob", &proof_now(gw, "bob", bob, true, "list")),
        None,
    );
    assert_eq!(r.status, 401, "still spent after a restart: {}", r.body);
    let r = post(
        gw,
        "/api/v1/admin/enrolments/list",
        &list_body("bob", &proof(gw, "bob", bob, "list")),
        None,
    );
    assert_eq!(r.status, 200, "the next step's code: {}", r.body);
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
            forwarded_for: None,
        },
        NOW,
    );
    assert_eq!(r.status, 404, "{}", r.body);
}

// ---- WP-16: challenges bound to the administrator who asked ---------------------

/// Whether `admin` answering `challenge` with `device` (and its one-time code for a
/// fresh step, so only the challenge decides) signs in.
fn verifies(gw: &DurableGateway, admin: &str, challenge: String, device: &Admin) -> Response {
    next_step();
    verifies_now(gw, admin, challenge, device)
}

/// [`verifies`] with the code for the current step.
fn verifies_now(gw: &DurableGateway, admin: &str, challenge: String, device: &Admin) -> Response {
    post(
        gw,
        "/api/v1/admin/enrolments/list",
        &list_body(admin, &answer(device, challenge, true, "list")),
        None,
    )
}

#[test]
fn a_challenge_is_bound_to_the_administrator_who_asked() {
    // [VLD-ANB-MIDGARD-0020] [AC-16.2]
    let dir = scratch("bound");
    let (state, devices) = provision(&dir);
    let device = |name: &str| &devices.iter().find(|(n, _)| *n == name).unwrap().1;
    let served = open_state_dir(
        &state,
        Vec::new(),
        &pin(&state),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .expect("boots");
    let gw = &served.gateway;

    // Alice's challenge, signed by and presented as Bob.
    let alices = fetch_challenge(gw, "alice", "list");
    let r = verifies(gw, "bob", alices.clone(), device("bob"));
    assert_eq!(r.status, 401, "{}", r.body);
    // The client reads only `unauthorised`; why is logged under the request's
    // reference. [VLD-ANB-MIDGARD-0053]
    assert_eq!(r.body, r#"{"error":"unauthorised"}"#, "{}", r.body);
    // The nonce was consumed: not even Alice can answer it now.
    let r = verifies(gw, "alice", alices, device("alice"));
    assert_eq!(r.status, 401, "consumed: {}", r.body);

    // A challenge of Bob's own works.
    let r = verifies(gw, "bob", fetch_challenge(gw, "bob", "list"), device("bob"));
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
    let served = open_state_dir(
        &state,
        Vec::new(),
        &pin(&state),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .expect("boots");
    let gw = &served.gateway;

    let alices = fetch_challenge(gw, "alice", "list");
    for _ in 0..1_000 {
        fetch_challenge(gw, "bob", "list");
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
    let served = open_state_dir(
        &state,
        Vec::new(),
        &pin(&state),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .expect("boots");
    let gw = &served.gateway;
    for body in [
        "",
        "{}",
        r#"{"admin":5}"#,
        r#"{"admin":"alice","purpose":"list","extra":1}"#,
        r#"{"admin":"alice"}"#,
        r#"{"admin":"alice","purpose":"everything"}"#,
    ] {
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
    let served = open_state_dir(
        &state,
        Vec::new(),
        &pin(&state),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .expect("boots");
    let gw = &served.gateway;
    fetch_challenge(gw, "alice", "list");
    let before = gw.admin_control().unwrap().outstanding();
    for i in 0..1_000 {
        let challenge = fetch_challenge(gw, &format!("nobody-{i}"), "list");
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
    let served = open_state_dir(
        &state,
        Vec::new(),
        &pin(&state),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .expect("boots");
    let gw = &served.gateway;
    gw.admin_control().unwrap().set_challenge_ceiling(8);

    let held: Vec<(&str, String)> = ["alice", "bob"]
        .into_iter()
        .flat_map(|admin| (0..4).map(move |_| admin))
        .map(|admin| (admin, fetch_challenge(gw, admin, "list")))
        .collect();

    // Audra holds none and the table is full: refused, and recorded once a minute.
    let audited = gw.audit_len();
    let audra = r#"{"admin":"audra","purpose":"list"}"#;
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

    // Neither earlier administrator lost a challenge. Each code is accepted once, so
    // alice and bob answer one each per step, within the challenges' lifetime.
    // [VLD-ANB-MIDGARD-0008]
    let (alices, bobs): (Vec<_>, Vec<_>) = held.into_iter().partition(|(a, _)| *a == "alice");
    for (i, pair) in alices.into_iter().zip(bobs).enumerate() {
        if i > 0 {
            next_step();
        }
        for (admin, challenge) in [pair.0, pair.1] {
            let r = verifies_now(gw, admin, challenge, device(admin));
            assert_eq!(r.status, 200, "{admin}: {}", r.body);
        }
    }
    drop(served);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The audit key the operator pins the gateway to, from outside the keystore it
/// checks. [MG-SEC-05]
fn pin(state: &StateDir) -> audit_writer::PinnedKey {
    audit_writer::PinnedKey::new(
        attest::Keystore::read(
            &state.audit_keystore(),
            &RootWrap::Passphrase(&test_passphrase()),
        )
        .unwrap()
        .verifying_key,
    )
}
