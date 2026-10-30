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
