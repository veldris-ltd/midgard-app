//! Loading a service's mesh identity from files fails closed: a certificate issued to
//! another service, a keystore that does not hold the certified key, or an anchor from
//! another issuer is refused; and the service's own audit ledger survives a reopen but
//! not tampering. [NFR-SEC-1] [RC-09] [AC-M-4]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};

use attest::Keystore;
use governance::Governance;
use iam::{Idp, IDP_SIG_ALG};
use policy::{DefaultPdp, PolicySet, Role};
use pq_mtls::{encode_anchor, encode_cert, ServiceId, WorkloadIdentity};
use pqc_agility::OsRng;
use service_runtime::ServiceIdentity;

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "runtime-identity-{tag}-{}-{uniq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// What `mg-admin mesh issue` writes, for `service` under `idp`.
fn issue(idp: &Idp, dir: &Path, service: &str, roles: &[&str]) {
    let key = Keystore::generate(service, IDP_SIG_ALG, &mut OsRng).unwrap();
    let audit = Keystore::generate(&format!("{service}-audit"), IDP_SIG_ALG, &mut OsRng).unwrap();
    let cert = idp
        .certify(&WorkloadIdentity {
            id: ServiceId(service.into()),
            roles: roles.iter().map(|r| Role(r.to_string())).collect(),
            sig_alg: IDP_SIG_ALG,
            verifying_key: key.verifying_key.clone(),
        })
        .unwrap();
    key.write(&dir.join(format!("{service}.keystore"))).unwrap();
    audit
        .write(&dir.join(format!("{service}.audit.keystore")))
        .unwrap();
    std::fs::write(dir.join(format!("{service}.cert")), encode_cert(&cert)).unwrap();
    std::fs::write(dir.join("mesh.anchor"), encode_anchor(&idp.trust_anchor())).unwrap();
}

fn refused(dir: &Path, service: &str) -> String {
    ServiceIdentity::load(dir, service)
        .err()
        .expect("the identity is refused")
}

#[test]
fn an_issued_identity_loads_and_its_ledger_survives_a_reopen_but_not_tampering() {
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let dir = scratch("load");
    issue(&idp, &dir, "mimir-server", &["inference-server"]);

    let identity = ServiceIdentity::load(&dir, "mimir-server").expect("loads");
    assert_eq!(identity.roles(), ["inference-server"]);
    assert_eq!(identity.anchor, idp.trust_anchor());

    let ledger = dir.join("mimir.ledger");
    let pdp = || DefaultPdp::new(PolicySet::new(vec![]).unwrap());
    {
        let writer = identity.audit_writer(&ledger, "mimir-inference").unwrap();
        let mut gov = Governance::with_audit(pdp(), writer);
        assert_eq!(gov.record_event("test", "BOOT", "mimir", 1).unwrap(), 0);
    }
    {
        let writer = identity.audit_writer(&ledger, "mimir-inference").unwrap();
        assert_eq!(writer.entries().len(), 1, "the entry was recovered");
        let mut gov = Governance::with_audit(pdp(), writer);
        assert_eq!(gov.record_event("test", "BOOT", "mimir", 2).unwrap(), 1);
    }

    let mut bytes = std::fs::read(&ledger).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    std::fs::write(&ledger, bytes).unwrap();
    assert!(
        identity.audit_writer(&ledger, "mimir-inference").is_err(),
        "a tampered ledger is not appended to"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_identity_that_does_not_verify_is_refused() {
    let idp = Idp::generate("midgard-idp", &mut OsRng).unwrap();

    let missing = scratch("missing");
    assert!(refused(&missing, "mimir-server").contains("cannot read"));

    let other_cert = scratch("other-cert");
    issue(&idp, &other_cert, "mimir-server", &["inference-server"]);
    issue(&idp, &other_cert, "forge-worker", &["inference-consumer"]);
    std::fs::copy(
        other_cert.join("forge-worker.cert"),
        other_cert.join("mimir-server.cert"),
    )
    .unwrap();
    let e = refused(&other_cert, "mimir-server");
    assert!(e.contains("is issued to 'forge-worker'"), "{e}");

    let other_key = scratch("other-key");
    issue(&idp, &other_key, "mimir-server", &["inference-server"]);
    issue(&idp, &other_key, "forge-worker", &["inference-consumer"]);
    std::fs::copy(
        other_key.join("forge-worker.keystore"),
        other_key.join("mimir-server.keystore"),
    )
    .unwrap();
    let e = refused(&other_key, "mimir-server");
    assert!(e.contains("does not hold the key"), "{e}");

    let rogue = Idp::generate("midgard-idp", &mut OsRng).unwrap();
    let other_anchor = scratch("other-anchor");
    issue(&idp, &other_anchor, "mimir-server", &["inference-server"]);
    std::fs::write(
        other_anchor.join("mesh.anchor"),
        encode_anchor(&rogue.trust_anchor()),
    )
    .unwrap();
    let e = refused(&other_anchor, "mimir-server");
    assert!(e.contains("does not verify"), "{e}");

    for dir in [missing, other_cert, other_key, other_anchor] {
        let _ = std::fs::remove_dir_all(dir);
    }
}
