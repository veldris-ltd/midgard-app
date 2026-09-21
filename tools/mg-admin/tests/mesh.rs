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
    verify_cert(&cert, &anchor, &fips_provider()).expect("the certificate chains to the anchor");
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
