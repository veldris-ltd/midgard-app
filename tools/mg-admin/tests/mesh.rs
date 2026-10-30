//! `mg-admin mesh issue` end to end, driving the real binary: a service's identity key,
//! audit key, certificate and the mesh trust anchor land in its output directory; the
//! certificate names its roles, binds the service's own key and verifies against the
//! anchor; an existing identity is never overwritten; and the issuance is audited.
//! [NFR-SEC-1] [RC-35]

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use attest::Keystore;
use audit_writer::{FileLedger, LedgerId, LedgerStore};
use http_gateway::state_dir::{StateDir, LEDGER_ID};
use kms::{Kms, KmsSealer};
use policy::Role;
use pq_mtls::{decode_anchor, decode_cert, verify_cert};
use pqc_agility::backend::fips::provider as fips_provider;

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    std::env::temp_dir().join(format!(
        "mg-admin-mesh-{tag}-{}-{}",
        std::process::id(),
        uniq
    ))
}

fn mg_admin(args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mg-admin"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mg-admin starts");
    child.stdin.take().expect("piped").write_all(b"").unwrap();
    child.wait_with_output().expect("mg-admin exits")
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn mesh_issue_writes_a_verifiable_identity_and_never_overwrites_one() {
    let dir = scratch("issue");
    let dir_arg = dir.display().to_string();
    let out = dir.with_extension("mimir-identity");
    let out_arg = out.display().to_string();
    let init = mg_admin(&["init", "--state-dir", &dir_arg]);
    assert!(init.status.success(), "{}", stderr(&init));

    let issue = |service: &str| {
        mg_admin(&[
            "mesh",
            "issue",
            "--state-dir",
            &dir_arg,
            "--service",
            service,
            "--roles",
            "inference-server,audited",
            "--out",
            &out_arg,
        ])
    };
    let issued = issue("mimir-server");
    assert!(issued.status.success(), "{}", stderr(&issued));

    let cert = decode_cert(&std::fs::read(out.join("mimir-server.cert")).unwrap())
        .expect("the certificate decodes");
    let anchor = decode_anchor(&std::fs::read(out.join("mesh.anchor")).unwrap())
        .expect("the anchor decodes");
    verify_cert(&cert, &anchor, &fips_provider(), now())
        .expect("the certificate chains to the anchor");
    assert_eq!(cert.identity.id.0, "mimir-server");
    assert!(cert
        .identity
        .roles
        .contains(&Role("inference-server".into())));
    assert_eq!(cert.identity.roles.len(), 2);

    let identity = Keystore::read(&out.join("mimir-server.keystore")).expect("identity keystore");
    assert_eq!(
        identity.verifying_key, cert.identity.verifying_key,
        "the certificate binds the service's own key"
    );
    let audit_key =
        Keystore::read(&out.join("mimir-server.audit.keystore")).expect("audit keystore");
    assert_ne!(
        audit_key.verifying_key, identity.verifying_key,
        "audit signing uses a separate key"
    );
    let state = StateDir::new(&dir);
    let issuer = Keystore::read(&state.idp_keystore()).unwrap();
    assert_eq!(
        anchor.ca_key, issuer.verifying_key,
        "the anchor is the enclosure's issuer"
    );

    let again = issue("mimir-server");
    assert_eq!(again.status.code(), Some(1), "{}", stderr(&again));
    assert!(stderr(&again).contains("never overwrites"));

    let bad = issue("mimir/server");
    assert_eq!(bad.status.code(), Some(2), "{}", stderr(&bad));

    let audit = Keystore::read(&state.audit_keystore()).unwrap();
    let sealer = KmsSealer::new(Kms::new(fips_provider(), audit.trust_root()));
    let ledger = FileLedger::open(state.ledger(), Box::new(sealer), LedgerId(LEDGER_ID.into()))
        .expect("the ledger opens");
    let rows: Vec<(&str, &str)> = ledger
        .entries()
        .iter()
        .map(|e| (e.record.action.0.as_str(), e.record.resource.0.as_str()))
        .collect();
    assert_eq!(
        rows,
        [(
            "ISSUE workload certificate",
            "service:mimir-server:audited,inference-server"
        )]
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&out);
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn read_crl(dir: &std::path::Path) -> pq_mtls::RevocationList {
    pq_mtls::decode_crl(&std::fs::read(dir.join("mesh.crl")).expect("mesh.crl")).unwrap()
}

/// MG-SEC-02/03/04: issued certificates carry a validity window and a clearance; a
/// revocation produces a newer signed list in every service directory named; renewal
/// is a no-op until the certificate is due, then rekeys.
#[test]
fn mesh_certificates_expire_revoke_and_renew() {
    let dir = scratch("lifecycle");
    let dir_arg = dir.display().to_string();
    let out_a = dir.with_extension("a");
    let out_b = dir.with_extension("b");
    let (a_arg, b_arg) = (out_a.display().to_string(), out_b.display().to_string());
    assert!(mg_admin(&["init", "--state-dir", &dir_arg])
        .status
        .success());

    let issued = mg_admin(&[
        "mesh",
        "issue",
        "--state-dir",
        &dir_arg,
        "--service",
        "svc-a",
        "--roles",
        "client",
        "--out",
        &a_arg,
        "--clearance",
        "restricted",
    ]);
    assert!(issued.status.success(), "{}", stderr(&issued));
    let issued = mg_admin(&[
        "mesh",
        "issue",
        "--state-dir",
        &dir_arg,
        "--service",
        "svc-b",
        "--roles",
        "server",
        "--out",
        &b_arg,
        "--days",
        "7",
    ]);
    assert!(issued.status.success(), "{}", stderr(&issued));
    let bad = mg_admin(&[
        "mesh",
        "issue",
        "--state-dir",
        &dir_arg,
        "--service",
        "svc-c",
        "--roles",
        "server",
        "--out",
        &b_arg,
        "--days",
        "0",
    ]);
    assert_eq!(bad.status.code(), Some(2), "{}", stderr(&bad));

    let anchor = decode_anchor(&std::fs::read(out_a.join("mesh.anchor")).unwrap()).unwrap();
    let cert_a = decode_cert(&std::fs::read(out_a.join("svc-a.cert")).unwrap()).unwrap();
    let cert_b = decode_cert(&std::fs::read(out_b.join("svc-b.cert")).unwrap()).unwrap();
    let t = now();
    assert!(cert_a.terms.not_after <= t + 30 * 86_400 && cert_a.terms.not_after > t + 29 * 86_400);
    assert!(cert_b.terms.not_after <= t + 7 * 86_400);
    assert_eq!(
        cert_a.terms.clearance.level,
        http_gateway::enclosure::level_of("restricted").unwrap()
    );
    assert!(verify_cert(&cert_a, &anchor, &fips_provider(), t + 31 * 86_400).is_err());

    // Each service directory starts with the signed version-1 list.
    let v1 = read_crl(&out_a);
    assert_eq!(v1.version, 1);
    v1.verify(&anchor, &fips_provider()).unwrap();

    let cert_a_path = out_a.join("svc-a.cert").display().to_string();
    let both = format!("{a_arg},{b_arg}");
    let revoked = mg_admin(&[
        "mesh",
        "revoke",
        "--state-dir",
        &dir_arg,
        "--cert",
        &cert_a_path,
        "--out",
        &both,
    ]);
    assert!(revoked.status.success(), "{}", stderr(&revoked));
    for out in [&out_a, &out_b] {
        let crl = read_crl(out);
        assert_eq!(crl.version, 2);
        assert!(crl.revoked.contains(&cert_a.fingerprint()));
        let fresh = decode_anchor(&std::fs::read(out_a.join("mesh.anchor")).unwrap()).unwrap();
        fresh.install_crl(&crl, &fips_provider()).unwrap();
        assert!(matches!(
            verify_cert(&cert_a, &fresh, &fips_provider(), t),
            Err(pq_mtls::MtlsError::PeerCertRevoked)
        ));
    }
    // Revoking again changes nothing.
    let again = mg_admin(&[
        "mesh",
        "revoke",
        "--state-dir",
        &dir_arg,
        "--cert",
        &cert_a_path,
        "--out",
        &a_arg,
    ]);
    assert!(again.status.success(), "{}", stderr(&again));
    assert_eq!(read_crl(&out_a).version, 2);

    // svc-b (7 days) is not due within 3 days, and is within 30.
    let early = mg_admin(&[
        "mesh",
        "renew",
        "--state-dir",
        &dir_arg,
        "--service",
        "svc-b",
        "--out",
        &b_arg,
        "--within",
        "3",
    ]);
    assert!(early.status.success(), "{}", stderr(&early));
    assert_eq!(
        decode_cert(&std::fs::read(out_b.join("svc-b.cert")).unwrap()).unwrap(),
        cert_b
    );
    let due = mg_admin(&[
        "mesh",
        "renew",
        "--state-dir",
        &dir_arg,
        "--service",
        "svc-b",
        "--out",
        &b_arg,
        "--within",
        "30",
    ]);
    assert!(due.status.success(), "{}", stderr(&due));
    let renewed = decode_cert(&std::fs::read(out_b.join("svc-b.cert")).unwrap()).unwrap();
    assert_ne!(
        renewed.identity.verifying_key,
        cert_b.identity.verifying_key
    );
    assert_eq!(renewed.identity.roles, cert_b.identity.roles);
    let key = Keystore::read(&out_b.join("svc-b.keystore")).unwrap();
    assert_eq!(key.verifying_key, renewed.identity.verifying_key);
    verify_cert(&renewed, &anchor, &fips_provider(), t).unwrap();

    for d in [&dir, &out_a, &out_b] {
        let _ = std::fs::remove_dir_all(d);
    }
}

/// MG-SEC-02 upgrade: a service issued by an earlier release (its certificate and
/// trust anchor in a format this release refuses, and no revocation list) is brought up
/// to date in place. Both keystores are kept byte for byte, so its ledger and sealed
/// state still open; the new certificate binds the key it already had; and a second
/// run changes nothing.
#[test]
fn mesh_upgrade_recertifies_the_existing_keys_in_place() {
    let dir = scratch("upgrade");
    let dir_arg = dir.display().to_string();
    let out = dir.with_extension("svc");
    let out_arg = out.display().to_string();
    assert!(mg_admin(&["init", "--state-dir", &dir_arg])
        .status
        .success());
    let issued = mg_admin(&[
        "mesh",
        "issue",
        "--state-dir",
        &dir_arg,
        "--service",
        "svc-old",
        "--roles",
        "inference-server",
        "--out",
        &out_arg,
    ]);
    assert!(issued.status.success(), "{}", stderr(&issued));

    // What an earlier release left: files this release cannot read, and no list.
    std::fs::write(out.join("svc-old.cert"), b"MGCERT1\x00an earlier format").unwrap();
    std::fs::write(out.join("mesh.anchor"), b"MGANCHOR1\x00an earlier format").unwrap();
    std::fs::remove_file(out.join("mesh.crl")).unwrap();
    let identity_before = std::fs::read(out.join("svc-old.keystore")).unwrap();
    let audit_before = std::fs::read(out.join("svc-old.audit.keystore")).unwrap();

    let upgrade = || {
        mg_admin(&[
            "mesh",
            "upgrade",
            "--state-dir",
            &dir_arg,
            "--service",
            "svc-old",
            "--roles",
            "inference-server",
            "--out",
            &out_arg,
        ])
    };
    let upgraded = upgrade();
    assert!(upgraded.status.success(), "{}", stderr(&upgraded));

    assert_eq!(
        std::fs::read(out.join("svc-old.keystore")).unwrap(),
        identity_before
    );
    assert_eq!(
        std::fs::read(out.join("svc-old.audit.keystore")).unwrap(),
        audit_before
    );
    let anchor = decode_anchor(&std::fs::read(out.join("mesh.anchor")).unwrap()).unwrap();
    let cert = decode_cert(&std::fs::read(out.join("svc-old.cert")).unwrap()).unwrap();
    verify_cert(&cert, &anchor, &fips_provider(), now()).expect("the new certificate verifies");
    let key = Keystore::read(&out.join("svc-old.keystore")).unwrap();
    assert_eq!(
        cert.identity.verifying_key, key.verifying_key,
        "the key it already had"
    );
    assert!(cert
        .identity
        .roles
        .contains(&Role("inference-server".into())));
    read_crl(&out).verify(&anchor, &fips_provider()).unwrap();

    let again = upgrade();
    assert!(again.status.success(), "{}", stderr(&again));
    assert!(String::from_utf8_lossy(&again.stdout).contains("already current"));
    assert_eq!(
        decode_cert(&std::fs::read(out.join("svc-old.cert")).unwrap()).unwrap(),
        cert,
        "a current certificate is kept"
    );

    // Upgrade never makes keys: a directory without them is refused.
    std::fs::remove_file(out.join("svc-old.audit.keystore")).unwrap();
    let refused = upgrade();
    assert_eq!(refused.status.code(), Some(1), "{}", stderr(&refused));
    assert!(stderr(&refused).contains("keeps a service's keys"));

    for d in [&dir, &out] {
        let _ = std::fs::remove_dir_all(d);
    }
}

/// MG-SEC-05 upgrade: a mesh service's own ledger written before ledgers had
/// checkpoints stops the service starting; `audit checkpoint --identity-dir` verifies
/// it under the service's audit keystore and records its first checkpoint, after which
/// the service opens it and appends again. Running it twice is safe, and a ledger that
/// does not verify under a pinned key that is not the service's is not adopted.
#[test]
fn audit_checkpoint_adopts_a_mesh_services_own_ledger() {
    use audit_writer::{
        checkpoint_path, Action, Actor, AuditRecord, RecordId, Resource, Timestamp,
    };
    use service_runtime::ServiceIdentity;

    let dir = scratch("svc-ledger");
    let dir_arg = dir.display().to_string();
    let out = dir.with_extension("svc");
    let out_arg = out.display().to_string();
    let state = dir.with_extension("svc-state");
    std::fs::create_dir_all(&state).unwrap();
    assert!(mg_admin(&["init", "--state-dir", &dir_arg])
        .status
        .success());
    let issued = mg_admin(&[
        "mesh",
        "issue",
        "--state-dir",
        &dir_arg,
        "--service",
        "mimir-server",
        "--roles",
        "inference-server",
        "--out",
        &out_arg,
    ]);
    assert!(issued.status.success(), "{}", stderr(&issued));

    let ledger = state.join("mimir.ledger");
    let record = |n: &str| AuditRecord {
        id: RecordId(n.into()),
        actor: Actor("svc-client".into()),
        action: Action("Infer".into()),
        resource: Resource("model:granite".into()),
        ts: Timestamp("2026-10-30T00:00:00Z".into()),
    };
    {
        let identity = ServiceIdentity::load(&out, "mimir-server").expect("the identity loads");
        let mut writer = identity.audit_writer(&ledger, "mimir-inference").unwrap();
        writer.append(record("r1")).unwrap();
        writer.append(record("r2")).unwrap();
    }
    // What an earlier release left: the same entries, no checkpoint.
    std::fs::remove_file(checkpoint_path(&ledger)).unwrap();
    let identity = ServiceIdentity::load(&out, "mimir-server").unwrap();
    assert!(
        identity.audit_writer(&ledger, "mimir-inference").is_err(),
        "a ledger without its checkpoint stops the service"
    );

    let ledger_arg = ledger.display().to_string();
    let adopt = |extra: &[&str]| {
        let mut args = vec![
            "audit",
            "checkpoint",
            "--identity-dir",
            &out_arg,
            "--service",
            "mimir-server",
            "--ledger",
            &ledger_arg,
            "--ledger-id",
            "mimir-inference",
        ];
        args.extend_from_slice(extra);
        mg_admin(&args)
    };

    // Under a pinned key that is not this service's audit key, nothing is adopted.
    let other = StateDir::new(&dir)
        .audit_trust_anchor()
        .display()
        .to_string();
    let refused = adopt(&["--audit-pin", &other]);
    assert_eq!(refused.status.code(), Some(1), "{}", stderr(&refused));
    assert!(
        stderr(&refused).contains("pinned audit key"),
        "{}",
        stderr(&refused)
    );
    assert!(!checkpoint_path(&ledger).exists());

    let adopted = adopt(&[]);
    assert!(adopted.status.success(), "{}", stderr(&adopted));
    assert!(
        String::from_utf8_lossy(&adopted.stdout).contains("vouches for 2 entries"),
        "{}",
        String::from_utf8_lossy(&adopted.stdout)
    );
    let again = adopt(&[]);
    assert!(again.status.success(), "{}", stderr(&again));
    assert!(String::from_utf8_lossy(&again.stdout).contains("already has a checkpoint"));

    let mut writer = identity
        .audit_writer(&ledger, "mimir-inference")
        .expect("the adopted ledger opens");
    writer.append(record("r3")).expect("and takes new entries");

    for d in [&dir, &out, &state] {
        let _ = std::fs::remove_dir_all(d);
    }
}
