//! An audit row is shown as signed only after a real check of its own signature,
//! under the demo-audit domain and a pinned demo-audit key; any other signature on a
//! row refuses the whole dataset. The keys here are made in memory for the test.
//! [NFR-PQ-3] [AC-C-13]

use attest::accept::{preimage, DEMO_AUDIT, DEMO_DATASET};
use attest::envelope::SigEnvelope;
use attest::hex::hex;
use attest::json::{parse, Json};
use attest::TrustAnchors;
use midgard_client_core::demo::audit::{self, SignatureStatus};
use midgard_client_core::demo::enclosure::DemoEnclosure;
use midgard_client_core::marking::{MarkingLevel, RowMarking};
use midgard_client_core::source::{DataSource, Delivered, SourceError};
use pqc_agility::backend::fips::provider;
use pqc_agility::{OsRng, SigId, SigningKey, VerifyingKey};

const ITEM: &str = r#"{"id":"AUD-1","actor":"k.rao","action":"Query","resource":"r","ts":"14:00:00","pqSignature":"ML-DSA ✓"}"#;

struct Key {
    id: &'static str,
    alg: SigId,
    sk: SigningKey,
    vk: VerifyingKey,
}

impl Key {
    fn new(id: &'static str, alg: SigId) -> Self {
        let (vk, sk) = provider()
            .sig_for(alg)
            .unwrap()
            .keypair(&mut OsRng)
            .unwrap();
        Self { id, alg, sk, vk }
    }
    fn anchors(&self) -> TrustAnchors {
        let mut a = TrustAnchors::new();
        a.insert(self.id, self.vk.clone());
        a
    }
    /// The hex envelope over `message`, labelled `domain`.
    fn sig(&self, domain: &[u8], message: &[u8]) -> String {
        let signature = provider()
            .sig_for(self.alg)
            .unwrap()
            .sign(&self.sk, message)
            .unwrap();
        hex(&SigEnvelope {
            domain: domain.to_vec(),
            key_id: self.id.into(),
            signature,
        }
        .encode())
    }
    /// A correct signature over the row `AUD-1` of `midgard.audit` as [`ITEM`].
    fn row_sig(&self) -> String {
        self.sig(
            DEMO_AUDIT.label(),
            &audit::row_preimage("midgard.audit", "AUD-1", &marking(), &item()),
        )
    }
}

fn marking() -> RowMarking {
    RowMarking::new(MarkingLevel::Internal, Vec::<String>::new())
}

fn item() -> Json {
    parse(ITEM).unwrap()
}

fn body(row_extra: &str, ty: &str) -> String {
    format!(
        r#"{{"collections":{{"midgard.audit":{{"type":"{ty}","rows":[
          {{"id":"AUD-1","marking":{{"level":"internal","compartments":[]}},"item":{ITEM}{row_extra}}}]}}}},
         "personas":[{{"id":"k.rao","display":"K. Rao","role":"analyst","clearance":"restricted","compartments":[],"roles":["analyst"]}}],
         "format":"midgard-demo-dataset","version":1,"release":"test"}}"#
    )
}

fn load(row_extra: &str, anchors: &TrustAnchors) -> Result<DemoEnclosure, SourceError> {
    DemoEnclosure::from_body(
        "test",
        &parse(&body(row_extra, "AuditRecord")).unwrap(),
        anchors,
    )
}

fn sig_field(sig: &str) -> String {
    format!(r#","{}":"{sig}""#, audit::ROW_SIG_FIELD)
}

#[test]
fn a_row_signature_that_verifies_shows_as_verified() {
    let k = Key::new("midgard-demo-audit-test", SigId::MlDsa65);
    let e = load(&sig_field(&k.row_sig()), &k.anchors()).unwrap();
    let g = e.grant("k.rao").unwrap();
    let Delivered::Row {
        body, signature, ..
    } = &e.fetch(&g, "midgard.audit").unwrap()[0]
    else {
        panic!("withheld")
    };
    assert_eq!(*signature, Some(SignatureStatus::Verified));
    assert!(SignatureStatus::Verified.checked());
    let shown = parse(std::str::from_utf8(body).unwrap()).unwrap();
    assert_eq!(
        shown.get("pqSignature").and_then(Json::as_str),
        Some("verified")
    );
}

#[test]
fn a_row_without_a_signature_shows_none_and_no_check_mark() {
    let e = load("", &TrustAnchors::new()).unwrap();
    let g = e.grant("k.rao").unwrap();
    let Delivered::Row {
        body, signature, ..
    } = &e.fetch(&g, "midgard.audit").unwrap()[0]
    else {
        panic!("withheld")
    };
    assert_eq!(*signature, Some(SignatureStatus::Unsigned));
    assert!(!SignatureStatus::Unsigned.checked());
    assert_eq!(SignatureStatus::Unsigned.as_str(), "none");
    assert!(!String::from_utf8(body.clone()).unwrap().contains('✓'));
}

#[test]
fn any_row_signature_that_does_not_verify_refuses_the_dataset() {
    let k = Key::new("midgard-demo-audit-test", SigId::MlDsa65);
    let other = Key::new("midgard-demo-audit-test", SigId::MlDsa65);
    let small = Key::new("midgard-demo-audit-test", SigId::MlDsa44);
    let pre =
        |coll: &str, id: &str, m: &RowMarking, it: &Json| audit::row_preimage(coll, id, m, it);
    let tampered = {
        let mut t = ITEM.replace("Query", "Delete");
        t.insert(0, ' ');
        parse(&t).unwrap()
    };
    let cases: Vec<(&str, String, TrustAnchors)> = vec![
        ("no anchor pinned", k.row_sig(), TrustAnchors::new()),
        (
            "another key under the same id",
            other.row_sig(),
            k.anchors(),
        ),
        (
            "the dataset domain",
            k.sig(
                DEMO_DATASET.label(),
                &preimage(
                    &DEMO_DATASET,
                    &audit::signed_bytes("midgard.audit", "AUD-1", &marking(), &item()),
                ),
            ),
            k.anchors(),
        ),
        (
            "labelled demo-audit but made over another domain",
            k.sig(
                DEMO_AUDIT.label(),
                &preimage(
                    &DEMO_DATASET,
                    &audit::signed_bytes("midgard.audit", "AUD-1", &marking(), &item()),
                ),
            ),
            k.anchors(),
        ),
        (
            "another item",
            k.sig(
                DEMO_AUDIT.label(),
                &pre("midgard.audit", "AUD-1", &marking(), &tampered),
            ),
            k.anchors(),
        ),
        (
            "another id",
            k.sig(
                DEMO_AUDIT.label(),
                &pre("midgard.audit", "AUD-2", &marking(), &item()),
            ),
            k.anchors(),
        ),
        (
            "another collection",
            k.sig(
                DEMO_AUDIT.label(),
                &pre("forge.audit", "AUD-1", &marking(), &item()),
            ),
            k.anchors(),
        ),
        (
            "another marking",
            k.sig(
                DEMO_AUDIT.label(),
                &pre(
                    "midgard.audit",
                    "AUD-1",
                    &RowMarking::new(MarkingLevel::Open, Vec::<String>::new()),
                    &item(),
                ),
            ),
            k.anchors(),
        ),
        ("an unapproved algorithm", small.row_sig(), small.anchors()),
        ("not hex", "zz".into(), k.anchors()),
        ("not an envelope", "00ff".into(), k.anchors()),
    ];
    for (what, sig, anchors) in cases {
        assert!(
            matches!(
                load(&sig_field(&sig), &anchors),
                Err(SourceError::Malformed(_))
            ),
            "accepted a row signed with {what}"
        );
    }
    // Not a string.
    assert!(load(&format!(r#","{}":1"#, audit::ROW_SIG_FIELD), &k.anchors()).is_err());
}

#[test]
fn only_audit_rows_carry_a_signature_field() {
    let k = Key::new("midgard-demo-audit-test", SigId::MlDsa65);
    let text = body(&sig_field(&k.row_sig()), "MidgardSloMetric");
    match DemoEnclosure::from_body("test", &parse(&text).unwrap(), &k.anchors()) {
        Err(SourceError::Malformed(m)) => assert!(m.contains(audit::ROW_SIG_FIELD), "{m}"),
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("a non-audit row carried a signature"),
    }
}

impl attest::SigningBackend for Key {
    fn key_id(&self) -> &str {
        self.id
    }
    fn sign(&self, msg: &[u8]) -> Result<pqc_agility::Signature, attest::SignError> {
        provider()
            .sig_for(self.alg)
            .unwrap()
            .sign(&self.sk, msg)
            .map_err(|e| attest::SignError::Custodian(e.to_string()))
    }
    fn verifying_key(&self) -> &VerifyingKey {
        &self.vk
    }
}

#[test]
fn the_demo_audit_signer_signs_whole_audit_row_preimages_and_nothing_else() {
    use attest::SigningBackend;
    let s = audit::DemoAuditOnly::new(Key::new("midgard-demo-audit-test", SigId::MlDsa65));
    let good = audit::row_preimage("midgard.audit", "AUD-1", &marking(), &item());
    s.sign(&good).unwrap();
    let signed = audit::signed_bytes("midgard.audit", "AUD-1", &marking(), &item());
    let text = String::from_utf8(signed.clone()).unwrap();
    let mut trailing = good.clone();
    trailing.push(b' ');
    for msg in [
        b"an arbitrary message".to_vec(),
        Vec::new(),
        signed.clone(),
        preimage(&DEMO_DATASET, &signed),
        trailing,
        preimage(&DEMO_AUDIT, b"{}"),
        preimage(&DEMO_AUDIT, b"not json"),
        // Not canonical (spaces), an extra key, a bad level, unsorted compartments.
        preimage(&DEMO_AUDIT, text.replacen(':', ": ", 1).as_bytes()),
        preimage(&DEMO_AUDIT, text.replacen('{', r#"{"a":1,"#, 1).as_bytes()),
        preimage(
            &DEMO_AUDIT,
            text.replace(r#""level":"internal""#, r#""level":"secret""#)
                .as_bytes(),
        ),
        preimage(
            &DEMO_AUDIT,
            text.replace(r#""compartments":[]"#, r#""compartments":["SIG","OPS-N"]"#)
                .as_bytes(),
        ),
        preimage(
            &DEMO_AUDIT,
            text.replace(r#""id":"AUD-1""#, r#""id":1"#).as_bytes(),
        ),
    ] {
        assert!(
            matches!(s.sign(&msg), Err(attest::SignError::Custodian(_))),
            "signed {:?}",
            String::from_utf8_lossy(&msg)
        );
    }
}

#[test]
fn signing_the_audit_rows_of_a_body_marks_each_verified_and_only_audit_rows() {
    let k = Key::new("midgard-demo-audit-test", SigId::MlDsa65);
    let s = audit::DemoAuditOnly::new(k);
    let plain = parse(&body("", "AuditRecord")).unwrap();
    let signed = audit::sign_rows(&plain, &s).unwrap();
    assert_eq!(audit::verify_rows(&signed, &s_anchors(&s)).unwrap(), 1);
    assert_eq!(audit::without_row_sigs(&signed), plain);
    // An unsigned audit row, or a body signed twice, is refused.
    assert!(audit::verify_rows(&plain, &s_anchors(&s)).is_err());
    assert!(audit::sign_rows(&signed, &s).is_err());
    let e = DemoEnclosure::from_body("test", &signed, &s_anchors(&s)).unwrap();
    let g = e.grant("k.rao").unwrap();
    let Delivered::Row { signature, .. } = &e.fetch(&g, "midgard.audit").unwrap()[0] else {
        panic!("withheld")
    };
    assert_eq!(*signature, Some(SignatureStatus::Verified));
    // Rows of other types are left alone.
    let other = parse(&body("", "AuditRecord").replace(
        r#""midgard.audit":{"#,
        r#""midgard.observability":{"type":"MidgardSloMetric","rows":[]},"midgard.audit":{"#,
    ))
    .unwrap();
    let signed = audit::sign_rows(&other, &s).unwrap();
    assert_eq!(audit::verify_rows(&signed, &s_anchors(&s)).unwrap(), 1);
}

fn s_anchors(s: &audit::DemoAuditOnly<Key>) -> TrustAnchors {
    use attest::SigningBackend;
    let mut a = TrustAnchors::new();
    a.insert(s.key_id(), s.verifying_key().clone());
    a
}
