//! Workload identities for the service mesh. [NFR-SEC-1] [RC-35]
//!
//! `mesh issue` gives a service what it needs to join the mesh: its own identity key and
//! a separate audit-signing key, each in a sealed keystore, and a certificate for its
//! roles signed by the enclosure's issuer — plus the mesh trust anchor every service
//! verifies its peers against. The keys never leave the output directory, which belongs
//! to that service alone.

use std::collections::BTreeSet;
use std::path::Path;

use attest::Keystore;
use http_gateway::state_dir::{StateDir, StateLock, IDP_KEY_ID};
use iam::{Idp, IDP_SIG_ALG};
use policy::Role;
use pq_mtls::{encode_anchor, encode_cert, ServiceId, WorkloadIdentity};
use pqc_agility::{OsRng, SecretBytes, SigningKey};

use crate::{list, record_admin_event, Failure, Options};

/// The file a service's identity keystore is written to.
pub(crate) fn identity_keystore(out: &Path, service: &str) -> std::path::PathBuf {
    out.join(format!("{service}.keystore"))
}

pub(crate) fn mesh_issue(o: &Options) -> Result<String, Failure> {
    let state = StateDir::new(o.require("state-dir")?);
    let service = o.require("service")?;
    if !service
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(Failure::Usage(format!(
            "--service may contain only letters, digits, _ and -, not '{service}'"
        )));
    }
    let roles: BTreeSet<Role> = list(o.require("roles")?).into_iter().map(Role).collect();
    if roles.is_empty() {
        return Err(Failure::Usage("--roles needs at least one role".into()));
    }
    let out = Path::new(o.require("out")?);
    let key_path = identity_keystore(out, service);
    let audit_path = out.join(format!("{service}.audit.keystore"));
    let cert_path = out.join(format!("{service}.cert"));
    let anchor_path = out.join("mesh.anchor");
    for path in [&key_path, &audit_path, &cert_path] {
        if path.exists() {
            return Err(Failure::Run(format!(
                "{} exists; mesh issue never overwrites a service's identity",
                path.display()
            )));
        }
    }

    let _lock = StateLock::acquire(&state, "mg-admin mesh issue").map_err(Failure::Run)?;
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
    let idp = Idp::from_key(IDP_KEY_ID, issuer_vk, issuer_sk)
        .map_err(|e| Failure::Run(format!("the issuer keystore is not usable: {e}")))?;

    let identity_key = Keystore::generate(service, IDP_SIG_ALG, &mut OsRng)
        .map_err(|e| Failure::Run(format!("cannot generate the identity key: {e}")))?;
    let audit_key = Keystore::generate(&format!("{service}-audit"), IDP_SIG_ALG, &mut OsRng)
        .map_err(|e| Failure::Run(format!("cannot generate the audit key: {e}")))?;
    let cert = idp
        .certify(&WorkloadIdentity {
            id: ServiceId(service.to_string()),
            roles: roles.clone(),
            sig_alg: IDP_SIG_ALG,
            verifying_key: identity_key.verifying_key.clone(),
        })
        .map_err(|e| Failure::Run(format!("cannot certify the service: {e:?}")))?;
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
    let write = |path: &Path, bytes: &[u8]| {
        std::fs::write(path, bytes)
            .map_err(|e| Failure::Run(format!("cannot write {}: {e}", path.display())))
    };
    identity_key
        .write(&key_path)
        .map_err(|e| Failure::Run(format!("cannot write {}: {e}", key_path.display())))?;
    audit_key
        .write(&audit_path)
        .map_err(|e| Failure::Run(format!("cannot write {}: {e}", audit_path.display())))?;
    write(&cert_path, &encode_cert(&cert))?;
    write(&anchor_path, &encode_anchor(&idp.trust_anchor()))?;

    Ok(format!(
        "issued a mesh identity for '{service}' (roles {role_names})\n\
         \x20 {}  its identity key — a secret\n\
         \x20 {}  its audit-signing key — a secret\n\
         \x20 {}  its certificate\n\
         \x20 {}  the mesh trust anchor every service verifies peers against\n\
         The certificate does not expire by itself: reissue a service to rotate its key.",
        key_path.display(),
        audit_path.display(),
        cert_path.display(),
        anchor_path.display()
    ))
}
