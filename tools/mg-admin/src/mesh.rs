//! Workload identities for the service mesh. [NFR-SEC-1] [RC-35]
//!
//! `mesh issue` gives a service what it needs to join the mesh: its own identity key and
//! a separate audit-signing key, each in a sealed keystore, and a certificate for its
//! roles signed by the enclosure's issuer — plus the mesh trust anchor every service
//! verifies its peers against, and the current signed revocation list. The keys never
//! leave the output directory, which belongs to that service alone.
//!
//! Certificates expire: the validity window (`--days`, default 30) and the clearance the
//! service may be authorised at (`--clearance`, default open) are signed into the
//! certificate. `mesh renew` reissues a service with a fresh key before expiry and is
//! meant to run on a timer; `--within` makes it a no-op until the certificate is that
//! close to expiring. [MG-SEC-02] [MG-SEC-04]
//!
//! `mesh revoke` adds a certificate to the enclosure's revocation list, signs the next
//! version of it, and writes it as `mesh.crl` into each service directory given. A
//! running service picks a newer list up within seconds, without a restart. The list's
//! master copy lives in the state directory's `trust/` and only ever moves forward.
//! [MG-SEC-03]
//!
//! `mesh upgrade` brings a service issued before certificates carried terms up to date
//! in place: it certifies the identity key the service already holds, and rewrites the
//! trust anchor and revocation list in their current formats. It never touches the
//! keystores — the audit keystore signs the service's ledger and seals its state, so a
//! new one would lock the service out of both. A service that is already current is
//! left alone, apart from a missing revocation list, which is written. [MG-SEC-02]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use attest::Keystore;
use http_gateway::enclosure::level_of;
use http_gateway::state_dir::{StateDir, StateLock, IDP_KEY_ID};
use iam::{Idp, IDP_SIG_ALG};
use policy::{Compartment, Role};
use pq_mtls::{
    cert_fingerprint, decode_anchor, decode_cert, decode_crl, encode_anchor, encode_cert,
    encode_crl, CertTerms, Posture, RevocationList, ServiceId, WorkloadIdentity,
};
use pqc_agility::{OsRng, Rng, SecretBytes, SigningKey};

use crate::{list, record_admin_event, Failure, Options};

/// Default certificate lifetime in days. [MG-SEC-02]
pub(crate) const DEFAULT_DAYS: u64 = 30;
/// Longest certificate lifetime `--days` accepts.
const MAX_DAYS: u64 = 365;
const DAY: u64 = 86_400;

/// The revocation list file name, in the state directory's `trust/` and in each
/// service's identity directory.
const CRL_FILE: &str = "mesh.crl";

/// The file a service's identity keystore is written to.
pub(crate) fn identity_keystore(out: &Path, service: &str) -> std::path::PathBuf {
    out.join(format!("{service}.keystore"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The enclosure issuer, unsealed from the state directory.
fn issuer(state: &StateDir) -> Result<Idp, Failure> {
    let issuer = Keystore::read(&state.idp_keystore()).map_err(|e| {
        Failure::Run(format!(
            "cannot open the issuer keystore (has `mg-admin init` been run?): {e}"
        ))
    })?;
    let issuer_vk = issuer.verifying_key.clone();
    let issuer_alg = issuer.alg;
    let root = issuer
        .into_signing_root()
        .map_err(|e| Failure::Run(format!("cannot unseal the issuer key: {e}")))?;
    let issuer_sk = SigningKey::new(
        issuer_alg,
        SecretBytes::new(root.expose_signing_key().expose().to_vec()),
    );
    Idp::from_key(IDP_KEY_ID, issuer_vk, issuer_sk)
        .map_err(|e| Failure::Run(format!("the issuer keystore is not usable: {e}")))
}

fn check_service_name(service: &str) -> Result<(), Failure> {
    if service
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Ok(())
    } else {
        Err(Failure::Usage(format!(
            "--service may contain only letters, digits, _ and -, not '{service}'"
        )))
    }
}

/// `--days`, defaulting to [`DEFAULT_DAYS`].
fn lifetime_days(o: &Options) -> Result<u64, Failure> {
    let days = match o.get("days") {
        None => DEFAULT_DAYS,
        Some(d) => d
            .parse::<u64>()
            .map_err(|_| Failure::Usage(format!("--days must be a whole number, not '{d}'")))?,
    };
    if days == 0 || days > MAX_DAYS {
        return Err(Failure::Usage(format!(
            "--days must be between 1 and {MAX_DAYS}"
        )));
    }
    Ok(days)
}

/// `--clearance` (a band, default open) and `--compartments`: what the certificate
/// grants. [MG-SEC-04]
fn granted_clearance(o: &Options) -> Result<Posture, Failure> {
    let band = o.get("clearance").unwrap_or("open");
    let level = level_of(band).ok_or_else(|| {
        Failure::Usage(format!(
            "--clearance must be open, internal, restricted or confidential, not '{band}'"
        ))
    })?;
    let compartments: BTreeSet<Compartment> = o
        .get("compartments")
        .map(list)
        .unwrap_or_default()
        .into_iter()
        .map(Compartment)
        .collect();
    Ok(Posture {
        level,
        compartments,
    })
}

fn random_serial() -> u64 {
    let mut b = [0u8; 8];
    OsRng.fill(&mut b);
    u64::from_le_bytes(b) >> 1
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    std::fs::write(path, bytes)
        .map_err(|e| Failure::Run(format!("cannot write {}: {e}", path.display())))
}

/// Write `bytes` to `path` by way of a temporary file and a rename, so a reader never
/// sees half a file.
fn replace(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let tmp = path.with_extension("tmp");
    write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
        .map_err(|e| Failure::Run(format!("cannot replace {}: {e}", path.display())))
}

fn master_crl_path(state: &StateDir) -> PathBuf {
    state.trust().join(CRL_FILE)
}

/// The enclosure's current signed revocation list: the master copy, verified against
/// the issuer, or a signed, empty version 1 if none has been issued yet.
fn current_crl(state: &StateDir, idp: &Idp) -> Result<RevocationList, Failure> {
    let path = master_crl_path(state);
    if !path.exists() {
        let first = idp
            .sign_crl(1, now_secs(), BTreeSet::new())
            .map_err(|e| Failure::Run(format!("cannot sign the revocation list: {e}")))?;
        std::fs::create_dir_all(state.trust())
            .map_err(|e| Failure::Run(format!("cannot create {}: {e}", state.trust().display())))?;
        replace(&path, &encode_crl(&first))?;
        return Ok(first);
    }
    let bytes = std::fs::read(&path)
        .map_err(|e| Failure::Run(format!("cannot read {}: {e}", path.display())))?;
    let crl = decode_crl(&bytes)
        .map_err(|e| Failure::Run(format!("{} is not a revocation list: {e}", path.display())))?;
    crl.verify(&idp.trust_anchor(), idp.provider())
        .map_err(|e| {
            Failure::Run(format!(
                "{} is not signed by this enclosure's issuer: {e}",
                path.display()
            ))
        })?;
    Ok(crl)
}

/// Certify a fresh identity key for `service`.
fn certify_service(
    idp: &Idp,
    service: &str,
    roles: &BTreeSet<Role>,
    identity_key: &Keystore,
    days: u64,
    clearance: Posture,
) -> Result<pq_mtls::IdentityCert, Failure> {
    let now = now_secs();
    idp.certify(
        &WorkloadIdentity {
            id: ServiceId(service.to_string()),
            roles: roles.clone(),
            sig_alg: IDP_SIG_ALG,
            verifying_key: identity_key.verifying_key.clone(),
        },
        // A minute of slack for clocks that run slightly behind this one.
        CertTerms::new(random_serial(), now - 60, now + days * DAY, clearance),
    )
    .map_err(|e| Failure::Run(format!("cannot certify the service: {e:?}")))
}

pub(crate) fn mesh_issue(o: &Options) -> Result<String, Failure> {
    let state = StateDir::new(o.require("state-dir")?);
    let service = o.require("service")?;
    check_service_name(service)?;
    let roles: BTreeSet<Role> = list(o.require("roles")?).into_iter().map(Role).collect();
    if roles.is_empty() {
        return Err(Failure::Usage("--roles needs at least one role".into()));
    }
    let days = lifetime_days(o)?;
    let clearance = granted_clearance(o)?;
    let out = Path::new(o.require("out")?);
    let key_path = identity_keystore(out, service);
    let audit_path = out.join(format!("{service}.audit.keystore"));
    let cert_path = out.join(format!("{service}.cert"));
    let anchor_path = out.join("mesh.anchor");
    let crl_path = out.join(CRL_FILE);
    for path in [&key_path, &audit_path, &cert_path] {
        if path.exists() {
            return Err(Failure::Run(format!(
                "{} exists; mesh issue never overwrites a service's identity",
                path.display()
            )));
        }
    }

    let _lock = StateLock::acquire(&state, "mg-admin mesh issue").map_err(Failure::Run)?;
    let idp = issuer(&state)?;
    let crl = current_crl(&state, &idp)?;

    let identity_key = Keystore::generate(service, IDP_SIG_ALG, &mut OsRng)
        .map_err(|e| Failure::Run(format!("cannot generate the identity key: {e}")))?;
    let audit_key = Keystore::generate(&format!("{service}-audit"), IDP_SIG_ALG, &mut OsRng)
        .map_err(|e| Failure::Run(format!("cannot generate the audit key: {e}")))?;
    let cert = certify_service(&idp, service, &roles, &identity_key, days, clearance)?;
    let role_names: Vec<&str> = roles.iter().map(|r| r.0.as_str()).collect();
    let role_names = role_names.join(",");

    // Audit first: an identity the ledger cannot record is not issued.
    record_admin_event(
        &state,
        "ISSUE workload certificate",
        &format!("service:{service}:{role_names}"),
    )?;
    std::fs::create_dir_all(out)
        .map_err(|e| Failure::Run(format!("cannot create {}: {e}", out.display())))?;
    identity_key
        .write(&key_path)
        .map_err(|e| Failure::Run(format!("cannot write {}: {e}", key_path.display())))?;
    audit_key
        .write(&audit_path)
        .map_err(|e| Failure::Run(format!("cannot write {}: {e}", audit_path.display())))?;
    write(&cert_path, &encode_cert(&cert))?;
    write(&anchor_path, &encode_anchor(&idp.trust_anchor()))?;
    replace(&crl_path, &encode_crl(&crl))?;

    Ok(format!(
        "issued a mesh identity for '{service}' (roles {role_names}), valid for {days} days\n\
         \x20 {}  its identity key — a secret\n\
         \x20 {}  its audit-signing key — a secret\n\
         \x20 {}  its certificate\n\
         \x20 {}  the mesh trust anchor every service verifies peers against\n\
         \x20 {}  the signed revocation list (version {})\n\
         Renew it before it expires: `mg-admin mesh renew` on a timer does this.",
        key_path.display(),
        audit_path.display(),
        cert_path.display(),
        anchor_path.display(),
        crl_path.display(),
        crl.version,
    ))
}

/// Upgrade a service whose certificate, trust anchor or revocation list predates the
/// current formats, keeping both of its keystores. The certificate is reissued for the
/// identity key already in `--out`, with `--roles` (an old certificate cannot be read
/// for them) and the usual `--days` and `--clearance`. A certificate that is already
/// current, issued to this service for this key by this enclosure, is kept; then only
/// a trust anchor or revocation list that is missing or out of date is rewritten.
/// [MG-SEC-02] [MG-SEC-03]
pub(crate) fn mesh_upgrade(o: &Options) -> Result<String, Failure> {
    let state = StateDir::new(o.require("state-dir")?);
    let service = o.require("service")?;
    check_service_name(service)?;
    let roles: BTreeSet<Role> = list(o.require("roles")?).into_iter().map(Role).collect();
    if roles.is_empty() {
        return Err(Failure::Usage("--roles needs at least one role".into()));
    }
    let days = lifetime_days(o)?;
    let clearance = granted_clearance(o)?;
    let out = Path::new(o.require("out")?);
    let key_path = identity_keystore(out, service);
    let audit_path = out.join(format!("{service}.audit.keystore"));
    let cert_path = out.join(format!("{service}.cert"));
    let anchor_path = out.join("mesh.anchor");
    let crl_path = out.join(CRL_FILE);
    for path in [&key_path, &audit_path] {
        if !path.exists() {
            return Err(Failure::Run(format!(
                "{} is missing; mesh upgrade keeps a service's keys and cannot make them \
                 (use mesh issue for a new service)",
                path.display()
            )));
        }
    }
    let identity_key = Keystore::read(&key_path)
        .map_err(|e| Failure::Run(format!("cannot read {}: {e}", key_path.display())))?;

    let _lock = StateLock::acquire(&state, "mg-admin mesh upgrade").map_err(Failure::Run)?;
    let idp = issuer(&state)?;
    let anchor = idp.trust_anchor();
    let crl = current_crl(&state, &idp)?;

    let current = std::fs::read(&cert_path)
        .ok()
        .and_then(|bytes| decode_cert(&bytes).ok())
        .filter(|cert| {
            cert.identity.id.0 == service
                && cert.identity.verifying_key == identity_key.verifying_key
                && pq_mtls::verify_cert(cert, &anchor, idp.provider(), now_secs()).is_ok()
        });
    let mut done = Vec::new();
    if current.is_none() {
        let cert = certify_service(&idp, service, &roles, &identity_key, days, clearance)?;
        let role_names: Vec<&str> = roles.iter().map(|r| r.0.as_str()).collect();
        record_admin_event(
            &state,
            "UPGRADE workload certificate",
            &format!("service:{service}:{}", role_names.join(",")),
        )?;
        replace(&cert_path, &encode_cert(&cert))?;
        done.push(format!(
            "reissued {} for its existing key, valid for {days} days",
            cert_path.display()
        ));
    }
    let anchor_current = std::fs::read(&anchor_path)
        .ok()
        .and_then(|bytes| decode_anchor(&bytes).ok())
        .is_some_and(|a| a.ca_key == anchor.ca_key && a.ca_alg == anchor.ca_alg);
    if !anchor_current {
        replace(&anchor_path, &encode_anchor(&anchor))?;
        done.push(format!("rewrote {}", anchor_path.display()));
    }
    let crl_current = std::fs::read(&crl_path)
        .ok()
        .and_then(|bytes| decode_crl(&bytes).ok())
        .is_some_and(|c| c.version >= crl.version);
    if !crl_current {
        replace(&crl_path, &encode_crl(&crl))?;
        done.push(format!(
            "wrote {} (revocation list version {})",
            crl_path.display(),
            crl.version
        ));
    }
    if done.is_empty() {
        return Ok(format!("'{service}' is already current; nothing changed"));
    }
    Ok(format!(
        "upgraded '{service}', keeping its keystores:\n  {}\nRestart the service to present it.",
        done.join("\n  ")
    ))
}

/// Reissue a service's certificate with a fresh identity key, replacing its keystore
/// and certificate in `--out`. With `--within <days>` nothing happens until the current
/// certificate expires within that many days, so it can run on a timer. The service
/// keeps its roles; `--days` and `--clearance` apply to the new certificate. The old
/// certificate is not revoked — it simply expires. [MG-SEC-02]
pub(crate) fn mesh_renew(o: &Options) -> Result<String, Failure> {
    let state = StateDir::new(o.require("state-dir")?);
    let service = o.require("service")?;
    check_service_name(service)?;
    let days = lifetime_days(o)?;
    let clearance = granted_clearance(o)?;
    let out = Path::new(o.require("out")?);
    let key_path = identity_keystore(out, service);
    let cert_path = out.join(format!("{service}.cert"));

    let old = decode_cert(
        &std::fs::read(&cert_path)
            .map_err(|e| Failure::Run(format!("cannot read {}: {e}", cert_path.display())))?,
    )
    .map_err(|e| Failure::Run(format!("{} is not a certificate: {e}", cert_path.display())))?;
    if old.identity.id.0 != service {
        return Err(Failure::Run(format!(
            "{} is issued to '{}', not '{service}'",
            cert_path.display(),
            old.identity.id.0
        )));
    }
    let now = now_secs();
    if let Some(within) = o.get("within") {
        let within: u64 = within.parse().map_err(|_| {
            Failure::Usage(format!("--within must be a whole number, not '{within}'"))
        })?;
        if old.terms.not_after > now + within * DAY {
            return Ok(format!(
                "'{service}' does not need renewing: its certificate is valid for {} more days",
                (old.terms.not_after - now) / DAY
            ));
        }
    }

    let _lock = StateLock::acquire(&state, "mg-admin mesh renew").map_err(Failure::Run)?;
    let idp = issuer(&state)?;
    let identity_key = Keystore::generate(service, IDP_SIG_ALG, &mut OsRng)
        .map_err(|e| Failure::Run(format!("cannot generate the identity key: {e}")))?;
    let cert = certify_service(
        &idp,
        service,
        &old.identity.roles,
        &identity_key,
        days,
        clearance,
    )?;
    record_admin_event(
        &state,
        "RENEW workload certificate",
        &format!("service:{service}"),
    )?;
    let tmp_key = key_path.with_extension("keystore.tmp");
    identity_key
        .write(&tmp_key)
        .map_err(|e| Failure::Run(format!("cannot write {}: {e}", tmp_key.display())))?;
    replace(&cert_path, &encode_cert(&cert))?;
    std::fs::rename(&tmp_key, &key_path)
        .map_err(|e| Failure::Run(format!("cannot replace {}: {e}", key_path.display())))?;
    Ok(format!(
        "renewed '{service}': a new key and a certificate valid for {days} days. \
         Restart the service to present it."
    ))
}

/// Revoke a certificate: add it to the enclosure's revocation list, sign the next
/// version, keep it as the master copy and write it into every `--out` directory.
/// [MG-SEC-03]
pub(crate) fn mesh_revoke(o: &Options) -> Result<String, Failure> {
    let state = StateDir::new(o.require("state-dir")?);
    let cert_path = Path::new(o.require("cert")?);
    let outs: Vec<PathBuf> = list(o.require("out")?)
        .into_iter()
        .map(PathBuf::from)
        .collect();
    if outs.is_empty() {
        return Err(Failure::Usage(
            "--out needs at least one service directory".into(),
        ));
    }
    let cert = decode_cert(
        &std::fs::read(cert_path)
            .map_err(|e| Failure::Run(format!("cannot read {}: {e}", cert_path.display())))?,
    )
    .map_err(|e| Failure::Run(format!("{} is not a certificate: {e}", cert_path.display())))?;
    let fingerprint = cert_fingerprint(&cert);

    let _lock = StateLock::acquire(&state, "mg-admin mesh revoke").map_err(Failure::Run)?;
    let idp = issuer(&state)?;
    let current = current_crl(&state, &idp)?;
    let crl = if current.revoked.contains(&fingerprint) {
        current
    } else {
        let mut revoked = current.revoked.clone();
        revoked.insert(fingerprint);
        // Audit first: a revocation the ledger cannot record is not made.
        record_admin_event(
            &state,
            "REVOKE workload certificate",
            &format!(
                "service:{}:serial:{}",
                cert.identity.id.0, cert.terms.serial
            ),
        )?;
        let next = idp
            .sign_crl(current.version + 1, now_secs(), revoked)
            .map_err(|e| Failure::Run(format!("cannot sign the revocation list: {e}")))?;
        replace(&master_crl_path(&state), &encode_crl(&next))?;
        next
    };
    let bytes = encode_crl(&crl);
    for dir in &outs {
        replace(&dir.join(CRL_FILE), &bytes)?;
    }
    Ok(format!(
        "revoked '{}' (serial {}); revocation list version {} written to {} \
         director{}. Running services pick it up within a minute.",
        cert.identity.id.0,
        cert.terms.serial,
        crl.version,
        outs.len(),
        if outs.len() == 1 { "y" } else { "ies" }
    ))
}
