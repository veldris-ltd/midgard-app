//! Administrators, and online enrolment under dual control. [FR-AD-5] [RC-29] [RC-30]
//!
//! `admin add` runs offline, with the gateway stopped: it creates an administrator's
//! post-quantum passkey in a sealed keystore **the administrator keeps** — never in the
//! state directory — registers only its public half, and binds a standard role for a
//! bounded time. The `enrol` commands run against a live gateway: each fetches a
//! single-use challenge issued to the administrator it names, answers it with the
//! administrator's passkey, and sends the request. A requested enrolment waits until a
//! second administrator approves it, and carries no credentials: the gateway creates them at approval and returns a
//! single-use activation code, which the approver hands to the principal out of band.
//! [VLD-ANB-MIDGARD-0002] [VLD-ANB-MIDGARD-0066]
//!
//! An administrator proves two factors on every request: the passkey, and a one-time
//! code from the TOTP secret `admin add` enrols in the state directory and prints once
//! (`admin totp-reset` replaces a lost one). The gateway checks the code and accepts
//! it once; the passkey assertion's user-verification flag is signed truthfully, and
//! the gateway relies on it for nothing. [VLD-ANB-MIDGARD-0008]
//!
//! The passkey keystore's root is wrapped under `--passkey-passphrase-file`, which
//! defaults to the keystore passphrase; the administrator should be given one of their
//! own. [VLD-ANB-MIDGARD-0007]

use std::fmt::Write as _;
use std::io::{BufRead, IsTerminal, Write as _};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use admin_identity::{
    AdminChallenge, AdminId, DurableAdminPasskeys, DurableIdentityDirectory, RegisteredPasskey,
    ResourceScope, RoleBinding, RoleCatalog, SoftwareAdminAuthenticator, PASSKEY_SIG_ALG,
};
use attest::{Keystore, RootWrap};
use http_gateway::admin_totp::DurableAdminTotp;
use http_gateway::json::Json;
use http_gateway::state_dir::{StateDir, StateLock};
use iam::TotpSecret;
use kms::{Kms, KmsSealer};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{OsRng, Rng, SecretBytes, SigningKey};

use crate::http_client::post_json;
use crate::{
    list, passphrase, passphrase_or_default, record_admin_event, Failure, Options, TOTP_ISSUER,
    TOTP_SECRET_LEN,
};

/// The option naming the passphrase for an administrator's passkey keystore.
const PASSKEY_PASSPHRASE: &str = "passkey-passphrase-file";

/// The longest an administrator's role binding may run, in days.
const MAX_BINDING_DAYS: u64 = 3_650;
/// Whether opening the passkey keystore passes a local knowledge gate: what the
/// assertion's user-verification flag reports. The passkey keystore is wrapped under
/// its holder's passphrase (keystore format version 2, `--passkey-passphrase-file`), so
/// a passkey that opens at all was unlocked with that passphrase, and the flag is
/// signed true. The gateway never relies on the flag either way.
/// [VLD-ANB-MIDGARD-0008] [VLD-ANB-MIDGARD-0007]
const PASSKEY_OPENS_UNDER_A_PASSPHRASE: bool = true;
/// Digits in a one-time code.
const TOTP_DIGITS: usize = 6;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn hex_of(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| s.get(i..i + 2).and_then(|p| u8::from_str_radix(p, 16).ok()))
        .collect()
}

// ---- admin add (offline) ---------------------------------------------------------------

pub(crate) fn admin_add(o: &Options) -> Result<String, Failure> {
    let state = StateDir::new(o.require("state-dir")?);
    let admin = o.require("admin")?;
    if admin.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(Failure::Usage(format!(
            "--admin must not contain whitespace, not '{admin}'"
        )));
    }
    let catalog = RoleCatalog::standard();
    let role = o.require("role")?;
    if catalog.get(role).is_none() {
        let names: Vec<&str> = catalog.roles().iter().map(|r| r.name.as_str()).collect();
        return Err(Failure::Usage(format!(
            "--role must be one of {}, not '{role}'",
            names.join(", ")
        )));
    }
    let scope = o.get("scope").unwrap_or("iam");
    let days = match o.get("days") {
        None => 365,
        Some(d) => d
            .parse::<u64>()
            .ok()
            .filter(|d| (1..=MAX_BINDING_DAYS).contains(d))
            .ok_or_else(|| {
                Failure::Usage(format!("--days must be 1 to {MAX_BINDING_DAYS}, not '{d}'"))
            })?,
    };
    let keystore_path = Path::new(o.require("keystore")?);
    if keystore_path.exists() {
        return Err(Failure::Run(format!(
            "{} exists; admin add never overwrites a passkey",
            keystore_path.display()
        )));
    }

    let passphrase = passphrase(o)?;
    let wrap = RootWrap::Passphrase(&passphrase);
    let passkey_passphrase = passphrase_or_default(o, PASSKEY_PASSPHRASE)?;

    let _lock = StateLock::acquire(&state, "mg-admin admin add").map_err(Failure::Run)?;
    let idp = Keystore::read(&state.idp_keystore(), &wrap).map_err(|e| {
        Failure::Run(format!(
            "cannot open the issuer keystore (has `mg-admin init` been run?): {e}"
        ))
    })?;
    let sealer = || Box::new(KmsSealer::new(Kms::new(fips_provider(), idp.trust_root())));
    std::fs::create_dir_all(state.admin())
        .map_err(|e| Failure::Run(format!("cannot create {}: {e}", state.admin().display())))?;
    let mut passkeys =
        DurableAdminPasskeys::open(state.admin_passkeys(), sealer(), fips_provider())
            .map_err(|e| Failure::Run(format!("administrator passkeys do not open: {e}")))?;
    let id = AdminId(admin.to_string());
    if passkeys.is_registered(&id) {
        return Err(Failure::Run(format!(
            "'{admin}' is already an administrator; a passkey is never replaced here"
        )));
    }
    let mut roles = DurableIdentityDirectory::open(state.admin_roles(), sealer(), catalog)
        .map_err(|e| Failure::Run(format!("administrator roles do not open: {e}")))?;
    let mut totp = open_totp(&state, sealer())?;

    let passkey = Keystore::generate(admin, PASSKEY_SIG_ALG, &mut OsRng)
        .map_err(|e| Failure::Run(format!("cannot generate the passkey: {e}")))?;
    let secret = fresh_totp_secret();
    let enrolment_uri = secret.provisioning_uri(TOTP_ISSUER, admin);
    let now = now_secs();

    // Audit first: a grant the ledger cannot record does not happen.
    record_admin_event(
        &state,
        &wrap,
        "GRANT administrator",
        &format!("admin:{admin}:{role}:{scope}"),
    )?;
    passkey
        .write(
            keystore_path,
            &RootWrap::Passphrase(&passkey_passphrase),
            &mut OsRng,
        )
        .map_err(|e| Failure::Run(format!("cannot write {}: {e}", keystore_path.display())))?;
    passkeys
        .register(
            id.clone(),
            RegisteredPasskey {
                credential_id: admin.to_string(),
                public_key: passkey.verifying_key.clone(),
            },
        )
        .map_err(|e| Failure::Run(format!("cannot register the passkey: {e}")))?;
    roles
        .bind(RoleBinding {
            admin: id,
            role: role.to_string(),
            scope: ResourceScope::all(scope),
            not_before: now,
            not_after: now.saturating_add(days * 86_400),
        })
        .map_err(|e| Failure::Run(format!("cannot bind the role: {e}")))?;
    totp.enrol(admin, secret).map_err(|e| {
        Failure::Run(format!(
            "cannot record the one-time-code secret: {e}; run `mg-admin admin totp-reset` \
             for '{admin}'"
        ))
    })?;

    Ok(format!(
        "'{admin}' is an administrator: {role} in scope {scope} for {days} day(s)\n\
         passkey: {} — this file is their credential. Keep it with them, never in the \
         state directory, and never copy it to anyone else.\n{}",
        keystore_path.display(),
        totp_message(&enrolment_uri)
    ))
}

// ---- admin totp-reset (offline) --------------------------------------------------------

/// `admin totp-reset`: give an administrator who lost their authenticator a new TOTP
/// secret, printed once. The old secret stops working; codes already accepted stay
/// spent. [VLD-ANB-MIDGARD-0008]
pub(crate) fn admin_totp_reset(o: &Options) -> Result<String, Failure> {
    let state = StateDir::new(o.require("state-dir")?);
    let admin = o.require("admin")?;

    let passphrase = passphrase(o)?;
    let wrap = RootWrap::Passphrase(&passphrase);

    let _lock = StateLock::acquire(&state, "mg-admin admin totp-reset").map_err(Failure::Run)?;
    let idp = Keystore::read(&state.idp_keystore(), &wrap).map_err(|e| {
        Failure::Run(format!(
            "cannot open the issuer keystore (has `mg-admin init` been run?): {e}"
        ))
    })?;
    let sealer = || Box::new(KmsSealer::new(Kms::new(fips_provider(), idp.trust_root())));
    let passkeys = DurableAdminPasskeys::open(state.admin_passkeys(), sealer(), fips_provider())
        .map_err(|e| Failure::Run(format!("administrator passkeys do not open: {e}")))?;
    if !passkeys.is_registered(&AdminId(admin.to_string())) {
        return Err(Failure::Run(format!(
            "'{admin}' is not an administrator; `mg-admin admin add` makes one"
        )));
    }
    let mut totp = open_totp(&state, sealer())?;
    let secret = fresh_totp_secret();
    let enrolment_uri = secret.provisioning_uri(TOTP_ISSUER, admin);

    // Audit first: a reset the ledger cannot record does not happen.
    record_admin_event(
        &state,
        &wrap,
        "ROTATE administrator totp",
        &format!("admin:{admin}"),
    )?;
    totp.enrol(admin, secret)
        .map_err(|e| Failure::Run(format!("cannot record the one-time-code secret: {e}")))?;
    Ok(format!(
        "'{admin}' has a new one-time-code secret; the old one no longer works\n{}",
        totp_message(&enrolment_uri)
    ))
}

/// The administrators' TOTP journal in `state`, sealed by `sealer`.
fn open_totp(
    state: &StateDir,
    sealer: Box<dyn audit_writer::Sealer>,
) -> Result<DurableAdminTotp, Failure> {
    DurableAdminTotp::open(state.admin_totp(), sealer).map_err(|e| {
        Failure::Run(format!(
            "administrator one-time-code secrets do not open: {e}"
        ))
    })
}

fn fresh_totp_secret() -> TotpSecret {
    let mut secret = vec![0u8; TOTP_SECRET_LEN];
    OsRng.fill(&mut secret);
    TotpSecret::new(secret)
}

/// What an administrator is told about their new TOTP secret: `user add`'s wording.
fn totp_message(uri: &str) -> String {
    format!(
        "TOTP second factor — add this to the administrator's authenticator app, then \
         discard it; it is not shown again. Every enrol command needs a code from it:\n  {uri}"
    )
}

// ---- enrol request | approve | reject | list (online) ----------------------------------

/// An administrator ready to sign: who they are, their passkey, whether opening it
/// verified them locally, and the one-time code they gave.
struct Signer {
    admin: String,
    device: SoftwareAdminAuthenticator,
    user_verified: bool,
    totp: String,
}

/// The administrator named by `--admin`, their passkey from `--keystore` and their
/// one-time code, all gathered before anything is sent to the gateway.
fn device(o: &Options) -> Result<Signer, Failure> {
    let admin = o.require("admin")?.to_string();
    let path = o.require("keystore")?;
    let passkey_passphrase = passphrase_or_default(o, PASSKEY_PASSPHRASE)?;
    let keystore = Keystore::read(Path::new(path), &RootWrap::Passphrase(&passkey_passphrase))
        .map_err(|e| Failure::Run(format!("cannot open the passkey keystore {path}: {e}")))?;
    let credential_id = keystore.key_id.clone();
    let verifying_key = keystore.verifying_key.clone();
    let alg = keystore.alg;
    let root = keystore
        .into_signing_root()
        .map_err(|e| Failure::Run(format!("cannot unseal the passkey: {e}")))?;
    let signing_key = SigningKey::new(
        alg,
        SecretBytes::new(root.expose_signing_key().expose().to_vec()),
    );
    let device = SoftwareAdminAuthenticator::from_key(
        &fips_provider(),
        &credential_id,
        verifying_key,
        signing_key,
    )
    .map_err(|e| Failure::Run(format!("{path} does not hold a usable passkey: {e}")))?;
    let totp = one_time_code(o, &admin)?;
    Ok(Signer {
        admin,
        device,
        user_verified: PASSKEY_OPENS_UNDER_A_PASSPHRASE,
        totp,
    })
}

/// The administrator's one-time code: `--totp`, else a line from standard input (with
/// a prompt when that is a terminal). Six digits, or a usage error before the gateway
/// is contacted. [VLD-ANB-MIDGARD-0008]
fn one_time_code(o: &Options, admin: &str) -> Result<String, Failure> {
    let code = match o.get("totp") {
        Some(code) => code.to_string(),
        None => {
            let stdin = std::io::stdin();
            if stdin.is_terminal() {
                eprint!("one-time code for '{admin}': ");
                let _ = std::io::stderr().flush();
            }
            let mut line = String::new();
            stdin
                .lock()
                .read_line(&mut line)
                .map_err(|e| Failure::Run(format!("cannot read the one-time code: {e}")))?;
            line.trim().to_string()
        }
    };
    if code.len() != TOTP_DIGITS || !code.chars().all(|c| c.is_ascii_digit()) {
        return Err(Failure::Usage(format!(
            "a {TOTP_DIGITS}-digit one-time code from the administrator's authenticator app \
             is required: give --totp <code> or type it when asked"
        )));
    }
    Ok(code)
}

/// A fresh challenge from `gateway`, issued to `signer`'s administrator and answered by
/// `signer`: the fields every administration request starts with, the one-time code
/// among them. The gateway binds the challenge to the administrator named, so only
/// they can answer it. [VLD-ANB-MIDGARD-0020]
fn signed(gateway: &str, signer: &Signer) -> Result<Vec<(&'static str, Json)>, Failure> {
    let request = Json::obj(vec![("admin", Json::s(&signer.admin))]).encode();
    let reply = post_json(gateway, "/api/v1/admin/challenge", &request)?;
    let challenge = reply
        .str_field("challenge")
        .ok_or_else(|| Failure::Run("the gateway sent no challenge".into()))?;
    let nonce: [u8; 32] = unhex(challenge)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| Failure::Run("the gateway sent a malformed challenge".into()))?;
    // The flag is signed with the nonce, and says only what opening the passkey proved.
    let assertion = signer
        .device
        .authenticate(
            &fips_provider(),
            &AdminChallenge::new(nonce),
            signer.user_verified,
        )
        .map_err(|e| Failure::Run(format!("the passkey could not sign the challenge: {e}")))?;
    Ok(vec![
        ("admin", Json::s(&signer.admin)),
        ("challenge", Json::s(challenge)),
        (
            "assertion",
            Json::obj(vec![
                ("credentialId", Json::s(&assertion.credential_id)),
                ("userVerified", Json::Bool(assertion.user_verified)),
                (
                    "signature",
                    Json::s(&hex_of(assertion.signature.as_bytes())),
                ),
            ]),
        ),
        ("totp", Json::s(&signer.totp)),
    ])
}

fn strings(values: &[String]) -> Json {
    Json::Arr(values.iter().map(|v| Json::s(v)).collect())
}

pub(crate) fn enrol_request(o: &Options) -> Result<String, Failure> {
    let gateway = o.require("gateway")?;
    let subject = o.require("subject")?;
    let roles = list(o.require("roles")?);
    let purposes = list(o.require("purposes")?);
    if roles.is_empty() || purposes.is_empty() {
        return Err(Failure::Usage(
            "--roles and --purposes each need at least one value".into(),
        ));
    }
    let compartments = o.get("compartments").map(list).unwrap_or_default();
    let clearance = o.require("clearance")?;
    let justification = o.require("justification")?;
    let signer = device(o)?;

    // Who the principal is and what they may do; how they authenticate is set by the
    // gateway at approval and by the principal at activation.
    let principal = vec![
        ("subject", Json::s(subject)),
        ("roles", strings(&roles)),
        ("clearance", Json::s(clearance)),
        ("compartments", strings(&compartments)),
        ("purposes", strings(&purposes)),
    ];

    let mut body = signed(gateway, &signer)?;
    body.push(("justification", Json::s(justification)));
    body.push(("principal", Json::obj(principal)));
    let reply = post_json(
        gateway,
        "/api/v1/admin/enrolments",
        &Json::obj(body).encode(),
    )?;

    let id = reply.str_field("id").unwrap_or_default();
    Ok(format!(
        "enrolment {id} of '{subject}' is waiting for a second administrator to approve it"
    ))
}

pub(crate) fn enrol_approve(o: &Options) -> Result<String, Failure> {
    let gateway = o.require("gateway")?;
    let id = o.require("id")?;
    let subject = o.require("subject")?;
    let signer = device(o)?;
    let mut body = signed(gateway, &signer)?;
    body.push(("id", Json::s(id)));
    body.push(("subject", Json::s(subject)));
    let reply = post_json(
        gateway,
        "/api/v1/admin/enrolments/approve",
        &Json::obj(body).encode(),
    )?;
    let code = reply
        .str_field("activationCode")
        .ok_or_else(|| Failure::Run("the gateway sent no activation code".into()))?;
    let expires = match reply.get("activationExpiresAt") {
        Some(Json::Int(at)) => format!(" (unix time {at})"),
        _ => String::new(),
    };
    Ok(format!(
        "approved {id}: '{}' is enrolled and must activate before logging in\n\
         activation code — hand it to '{subject}' yourself, out of band, then discard it; \
         it is shown once, works once and expires in 72 hours{expires}:\n  {code}\n\
         '{subject}' activates with POST /api/v1/activate \
         {{\"username\",\"activationCode\",\"password\"}} and receives their TOTP second \
         factor there.",
        reply.str_field("enrolled").unwrap_or_default()
    ))
}

pub(crate) fn enrol_reject(o: &Options) -> Result<String, Failure> {
    let gateway = o.require("gateway")?;
    let id = o.require("id")?;
    let reason = o.require("reason")?;
    let signer = device(o)?;
    let mut body = signed(gateway, &signer)?;
    body.push(("id", Json::s(id)));
    body.push(("reason", Json::s(reason)));
    let reply = post_json(
        gateway,
        "/api/v1/admin/enrolments/reject",
        &Json::obj(body).encode(),
    )?;
    Ok(format!(
        "rejected {id}: the enrolment of '{}' is discarded",
        reply.str_field("rejected").unwrap_or_default()
    ))
}

pub(crate) fn enrol_list(o: &Options) -> Result<String, Failure> {
    let gateway = o.require("gateway")?;
    let signer = device(o)?;
    let body = signed(gateway, &signer)?;
    let reply = post_json(
        gateway,
        "/api/v1/admin/enrolments/list",
        &Json::obj(body).encode(),
    )?;
    let rows = match reply.get("enrolments") {
        Some(Json::Arr(rows)) => rows.as_slice(),
        _ => &[],
    };
    let mut message = format!("{} enrolment(s) waiting for approval", rows.len());
    for row in rows {
        let field = |key: &str| row.str_field(key).unwrap_or_default().to_string();
        let names = |key: &str| match row.get(key) {
            Some(Json::Arr(items)) => items
                .iter()
                .filter_map(|item| match item {
                    Json::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(","),
            _ => String::new(),
        };
        let _ = write!(
            message,
            "\n  {}  subject={}  roles={}  clearance={}  compartments={}  purposes={}  \
             mfa={}  requested-by={}  justification={:?}  state={}",
            field("id"),
            field("subject"),
            names("roles"),
            field("clearance"),
            names("compartments"),
            names("purposes"),
            field("mfa"),
            field("requestedBy"),
            field("justification"),
            field("state"),
        );
    }
    Ok(message)
}
