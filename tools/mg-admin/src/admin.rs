//! Administrators, and online enrolment under dual control. [FR-AD-5] [RC-29] [RC-30]
//!
//! `admin add` runs offline, with the gateway stopped: it creates an administrator's
//! post-quantum passkey in a sealed keystore **the administrator keeps** — never in the
//! state directory — registers only its public half, and binds a standard role for a
//! bounded time. The `enrol` commands run against a live gateway: each fetches a
//! single-use challenge, answers it with the administrator's passkey, and sends the
//! request. A requested enrolment waits until a second administrator approves it.

use std::fmt::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use admin_identity::{
    AdminChallenge, AdminId, DurableAdminPasskeys, DurableIdentityDirectory, RegisteredPasskey,
    ResourceScope, RoleBinding, RoleCatalog, SoftwareAdminAuthenticator, PASSKEY_SIG_ALG,
};
use attest::Keystore;
use http_gateway::json::Json;
use http_gateway::state_dir::{StateDir, StateLock};
use iam::{PasswordHasher, Pbkdf2Hasher, TotpSecret};
use kms::{Kms, KmsSealer};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{OsRng, Rng, SecretBytes, SigningKey};

use crate::http_client::post_json;
use crate::{list, read_password, record_admin_event, Failure, Options};
use crate::{TOTP_ISSUER, TOTP_SECRET_LEN};

/// The longest an administrator's role binding may run, in days.
const MAX_BINDING_DAYS: u64 = 3_650;

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

    let _lock = StateLock::acquire(&state, "mg-admin admin add").map_err(Failure::Run)?;
    let idp = Keystore::read(&state.idp_keystore()).map_err(|e| {
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

    let passkey = Keystore::generate(admin, PASSKEY_SIG_ALG, &mut OsRng)
        .map_err(|e| Failure::Run(format!("cannot generate the passkey: {e}")))?;
    let now = now_secs();

    // Audit first: a grant the ledger cannot record does not happen.
    record_admin_event(
        &state,
        "GRANT administrator",
        &format!("admin:{admin}:{role}:{scope}"),
    )?;
    passkey
        .write(keystore_path)
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

    Ok(format!(
        "'{admin}' is an administrator: {role} in scope {scope} for {days} day(s)\n\
         passkey: {} — this file is their credential. Keep it with them, never in the \
         state directory, and never copy it to anyone else.",
        keystore_path.display()
    ))
}

// ---- enrol request | approve | list (online) -------------------------------------------

/// The administrator named by `--admin`, and their passkey from `--keystore`.
fn device(o: &Options) -> Result<(String, SoftwareAdminAuthenticator), Failure> {
    let admin = o.require("admin")?.to_string();
    let path = o.require("keystore")?;
    let keystore = Keystore::read(Path::new(path))
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
    Ok((admin, device))
}

/// A fresh challenge from `gateway`, answered with `device`: the fields every
/// administration request starts with.
fn signed(
    gateway: &str,
    admin: &str,
    device: &SoftwareAdminAuthenticator,
) -> Result<Vec<(&'static str, Json)>, Failure> {
    let reply = post_json(gateway, "/api/v1/admin/challenge", "")?;
    let challenge = reply
        .str_field("challenge")
        .ok_or_else(|| Failure::Run("the gateway sent no challenge".into()))?;
    let nonce: [u8; 32] = unhex(challenge)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| Failure::Run("the gateway sent a malformed challenge".into()))?;
    let assertion = device
        .authenticate(&fips_provider(), &AdminChallenge::new(nonce), true)
        .map_err(|e| Failure::Run(format!("the passkey could not sign the challenge: {e}")))?;
    Ok(vec![
        ("admin", Json::s(admin)),
        ("challenge", Json::s(challenge)),
        (
            "assertion",
            Json::obj(vec![
                ("credentialId", Json::s(&assertion.credential_id)),
                ("userVerified", Json::Bool(true)),
                (
                    "signature",
                    Json::s(&hex_of(assertion.signature.as_bytes())),
                ),
            ]),
        ),
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
    let password = read_password()?;
    let (admin, device) = device(o)?;

    let totp = o.flag("totp").then(|| {
        let mut secret = vec![0u8; TOTP_SECRET_LEN];
        OsRng.fill(&mut secret);
        secret
    });
    // Only the hash leaves this machine, at the production work factor the gateway
    // requires.
    let mut principal = vec![
        ("subject", Json::s(subject)),
        ("roles", strings(&roles)),
        ("clearance", Json::s(clearance)),
        ("compartments", strings(&compartments)),
        ("purposes", strings(&purposes)),
        (
            "passwordHash",
            Json::s(&Pbkdf2Hasher::production().hash(password.as_bytes())),
        ),
    ];
    if let Some(secret) = &totp {
        principal.push(("totpSecret", Json::s(&hex_of(secret))));
    }

    let mut body = signed(gateway, &admin, &device)?;
    body.push(("justification", Json::s(justification)));
    body.push(("principal", Json::obj(principal)));
    let reply = post_json(
        gateway,
        "/api/v1/admin/enrolments",
        &Json::obj(body).encode(),
    )?;

    let id = reply.str_field("id").unwrap_or_default();
    let mut message = format!(
        "enrolment {id} of '{subject}' is waiting for a second administrator to approve it"
    );
    if let Some(secret) = totp {
        message.push_str(
            "\nTOTP second factor — give this to the principal for their authenticator app, \
             then discard it; it is not shown again:\n  ",
        );
        message.push_str(&TotpSecret::new(secret).provisioning_uri(TOTP_ISSUER, subject));
    }
    Ok(message)
}

pub(crate) fn enrol_approve(o: &Options) -> Result<String, Failure> {
    let gateway = o.require("gateway")?;
    let id = o.require("id")?;
    let (admin, device) = device(o)?;
    let mut body = signed(gateway, &admin, &device)?;
    body.push(("id", Json::s(id)));
    let reply = post_json(
        gateway,
        "/api/v1/admin/enrolments/approve",
        &Json::obj(body).encode(),
    )?;
    Ok(format!(
        "approved {id}: '{}' is enrolled and can log in",
        reply.str_field("enrolled").unwrap_or_default()
    ))
}

pub(crate) fn enrol_list(o: &Options) -> Result<String, Failure> {
    let gateway = o.require("gateway")?;
    let (admin, device) = device(o)?;
    let body = signed(gateway, &admin, &device)?;
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
        let _ = write!(
            message,
            "\n  {}  subject={}  clearance={}  requested-by={}",
            field("id"),
            field("subject"),
            field("clearance"),
            field("requestedBy"),
        );
    }
    Ok(message)
}
