//! The device-audit key: made in the core, sealed to the device's key root (never
//! exported in the clear), signing device statements (an audit row plus the action
//! this device recorded) under its own domain only; a row's `device` status rests on
//! a check under this device's key and nothing else. [NFR-PQ-3] [AC-C-13] [FR-C-2]

use attest::accept::{sign_in_domain, DEMO_AUDIT, DEVICE_AUDIT};
use attest::envelope::SigEnvelope;
use attest::hex::{hex, unhex};
use attest::json::{parse, Json};
use midgard_client_core::demo::audit::SignatureStatus;
use midgard_client_core::device_audit::{
    statement_bytes, DeviceAuditKey, DeviceKeyError, DeviceStatement,
};
use midgard_client_core::keystore::SoftwareKeystore;
use midgard_client_core::marking::{MarkingLevel, RowMarking};
use pqc_agility::backend::fips::provider;
use pqc_agility::{OsRng, SigId};

const C: &str = "midgard.audit";
const ID: &str = "AUD-D0001";

fn root() -> SoftwareKeystore {
    SoftwareKeystore::generate(&provider(), "device-root", &mut OsRng).unwrap()
}

fn row() -> (RowMarking, Json) {
    let item = parse(
        r#"{"id":"AUD-D0001","actor":"k.rao","action":"applied · demo.ping","resource":"midgard.audit/AUD-1","ts":"15:00:01","pqSignature":"none"}"#,
    )
    .unwrap();
    (
        RowMarking::new(MarkingLevel::Restricted, Vec::<String>::new()),
        item,
    )
}

fn st() -> DeviceStatement {
    DeviceStatement {
        action_id: "action:dev-1-00000000000000aa-1".into(),
        verb: "demo.ping".into(),
        target: "midgard.audit/AUD-1".into(),
        params_sha256: "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a".into(),
        status: "applied".into(),
    }
}

fn sign(key: &DeviceAuditKey, m: &RowMarking, item: &Json, s: &DeviceStatement) -> String {
    key.sign_statement_for_tests(C, ID, m, item, s).unwrap()
}

#[test]
fn a_sealed_key_reopens_only_under_its_root_and_is_the_same_key() {
    let r = root();
    let (key, blob) = DeviceAuditKey::create(&r, &mut OsRng).unwrap();
    let again = DeviceAuditKey::open(&r, &blob).unwrap();
    assert_eq!(key.key_id(), again.key_id());
    assert_eq!(key.verifying_key(), again.verifying_key());
    let (m, item) = row();
    let sig = sign(&key, &m, &item, &st());
    assert_eq!(
        again.verify_statement(C, ID, &m, &item, &st(), &sig),
        Ok(SignatureStatus::Device)
    );

    // Another root does not open it; nor does a changed byte anywhere.
    assert!(DeviceAuditKey::open(&root(), &blob).is_err());
    for at in [0, 9, blob.len() / 2, blob.len() - 1] {
        let mut bad = blob.clone();
        bad[at] ^= 1;
        assert!(DeviceAuditKey::open(&r, &bad).is_err(), "byte {at}");
    }
    assert!(matches!(
        DeviceAuditKey::open(&r, &blob[..blob.len() - 1]),
        Err(DeviceKeyError::Malformed(_) | DeviceKeyError::Seal(_))
    ));
}

#[test]
fn the_debug_form_names_the_key_and_nothing_else() {
    let (key, _) = DeviceAuditKey::create(&root(), &mut OsRng).unwrap();
    let shown = format!("{key:?}");
    assert!(shown.contains(key.key_id()), "{shown}");
    assert!(shown.len() < 200, "{shown}");
}

#[test]
fn a_tampered_row_does_not_verify() {
    let (key, _) = DeviceAuditKey::create(&root(), &mut OsRng).unwrap();
    let (m, item) = row();
    let sig = sign(&key, &m, &item, &st());
    let mut changed = item.clone();
    if let Json::Obj(pairs) = &mut changed {
        for (k, v) in pairs.iter_mut() {
            if k == "actor" {
                *v = Json::Str("someone.else".into());
            }
        }
    }
    assert!(key
        .verify_statement(C, ID, &m, &changed, &st(), &sig)
        .is_err());
    // Nor under another id, collection or marking.
    assert!(key
        .verify_statement(C, "AUD-D0002", &m, &item, &st(), &sig)
        .is_err());
    assert!(key
        .verify_statement("admin.audit", ID, &m, &item, &st(), &sig)
        .is_err());
    let lower = RowMarking::new(MarkingLevel::Open, Vec::<String>::new());
    assert!(key
        .verify_statement(C, ID, &lower, &item, &st(), &sig)
        .is_err());
}

#[test]
fn the_signature_covers_every_field_of_the_statement() {
    let (key, _) = DeviceAuditKey::create(&root(), &mut OsRng).unwrap();
    let (m, item) = row();
    let sig = sign(&key, &m, &item, &st());
    let variants: [fn(&mut DeviceStatement); 5] = [
        |s| s.action_id.push('9'),
        |s| s.verb = "admin.approve".into(),
        |s| s.target = "midgard.audit/AUD-2".into(),
        |s| s.params_sha256 = "00".repeat(32),
        |s| s.status = "refused".into(),
    ];
    for f in variants {
        let mut s = st();
        f(&mut s);
        assert!(key.verify_statement(C, ID, &m, &item, &s, &sig).is_err());
    }
}

#[test]
fn a_signature_under_another_domain_is_refused() {
    let r = root();
    let (key, _) = DeviceAuditKey::create(&r, &mut OsRng).unwrap();
    let (m, item) = row();
    let good = sign(&key, &m, &item, &st());

    // The device's own signature with the envelope relabelled as demo-audit.
    let mut env = SigEnvelope::parse(&unhex(&good).unwrap()).unwrap();
    assert_eq!(env.domain, DEVICE_AUDIT.label());
    env.domain = DEMO_AUDIT.label().to_vec();
    let relabelled = hex(&env.encode());
    let err = key
        .verify_statement(C, ID, &m, &item, &st(), &relabelled)
        .unwrap_err();
    assert!(err.contains("domain"), "{err}");

    // A real signature over the same statement under the demo-audit domain,
    // presented with the device-audit label and this key's id: it does not verify.
    let (_vk, sk) = provider()
        .sig_for(SigId::MlDsa65)
        .unwrap()
        .keypair(&mut OsRng)
        .unwrap();
    let body = statement_bytes(C, ID, &m, &item, &st());
    let other = SigEnvelope {
        domain: DEVICE_AUDIT.label().to_vec(),
        key_id: key.key_id().to_string(),
        signature: sign_in_domain(&DEMO_AUDIT, &body, &sk).unwrap(),
    };
    assert!(key
        .verify_statement(C, ID, &m, &item, &st(), &hex(&other.encode()))
        .is_err());

    // Not hex, not an envelope.
    assert!(key.verify_statement(C, ID, &m, &item, &st(), "zz").is_err());
    assert!(key.verify_statement(C, ID, &m, &item, &st(), "00").is_err());
}

#[test]
fn a_row_signed_by_another_device_key_is_not_this_devices() {
    let r = root();
    let (mine, _) = DeviceAuditKey::create(&r, &mut OsRng).unwrap();
    let (theirs, _) = DeviceAuditKey::create(&r, &mut OsRng).unwrap();
    assert_ne!(mine.key_id(), theirs.key_id());
    let (m, item) = row();
    let sig = sign(&theirs, &m, &item, &st());
    assert!(mine
        .verify_statement(C, ID, &m, &item, &st(), &sig)
        .is_err());

    // Even when the other key's envelope claims this key's id.
    let mut env = SigEnvelope::parse(&unhex(&sig).unwrap()).unwrap();
    env.key_id = mine.key_id().to_string();
    assert!(mine
        .verify_statement(C, ID, &m, &item, &st(), &hex(&env.encode()))
        .is_err());
}
