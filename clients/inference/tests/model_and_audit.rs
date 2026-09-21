//! AC-C-10 (on-device model PQ-signature verified on load, fail closed) and the
//! "audited" half of AC-C-6 (a served request seals a verifiable audit entry).
#![cfg(feature = "test-backend")]

use audit_writer::{verify_chain, AuditWriter, InMemoryLedger, LedgerId};
use midgard_client_inference::{
    seal_audit, serve, verify_and_load, Connectivity, DeviceCapability, InferenceError,
    InferenceRequest, MockOnDeviceRuntime, MockRemoteInference, PrivacyFirstReasoner,
    PrivacyPreference, SignedModel, TaskComplexity, TierContext,
};
use pqc_agility::backend::test_backend::{provider as test_provider, TestSig};
use pqc_agility::hash::sha256;
use pqc_agility::{SeedRng, SigId, SignatureScheme};

const ALG: SigId = SigId::MlDsa65;

// --- AC-C-10: model signature verification --------------------------------

#[test]
fn a_correctly_signed_model_loads() {
    let provider = test_provider();
    let scheme = TestSig::new(ALG);
    let (vk, sk) = scheme.keypair(&mut SeedRng::new(1)).unwrap();

    let bytes = b"int4-quantised-weights-blob";
    let sig = scheme.sign(&sk, &sha256(bytes)).unwrap();
    let model = SignedModel {
        id: "gemma-int4".into(),
        bytes,
        signature: sig,
    };

    let loaded = verify_and_load(&model, &vk, &provider).unwrap();
    assert_eq!(loaded.id, "gemma-int4");
    assert_eq!(loaded.digest, sha256(bytes));
}

#[test]
fn a_tampered_model_fails_closed() {
    let provider = test_provider();
    let scheme = TestSig::new(ALG);
    let (vk, sk) = scheme.keypair(&mut SeedRng::new(2)).unwrap();

    let bytes = b"int4-quantised-weights-blob";
    let sig = scheme.sign(&sk, &sha256(bytes)).unwrap();

    // The bytes are swapped after signing — the digest no longer matches.
    let tampered = b"int4-quantised-weights-TAMPERED";
    let model = SignedModel {
        id: "gemma-int4".into(),
        bytes: tampered,
        signature: sig,
    };

    let err = verify_and_load(&model, &vk, &provider).unwrap_err();
    assert_eq!(err, InferenceError::ModelSignatureInvalid);
}

#[test]
fn a_model_signed_by_the_wrong_key_fails_closed() {
    let provider = test_provider();
    let scheme = TestSig::new(ALG);
    let (_vk, sk) = scheme.keypair(&mut SeedRng::new(3)).unwrap();
    // A different (attacker) key pair — not the pinned Veldris key.
    let (attacker_vk, _attacker_sk) = scheme.keypair(&mut SeedRng::new(999)).unwrap();

    let bytes = b"weights";
    let sig = scheme.sign(&sk, &sha256(bytes)).unwrap();
    let model = SignedModel {
        id: "m".into(),
        bytes,
        signature: sig,
    };

    // Verifying against the wrong key must fail closed.
    let err = verify_and_load(&model, &attacker_vk, &provider).unwrap_err();
    assert_eq!(err, InferenceError::ModelSignatureInvalid);
}

// --- AC-C-6 (audited): a served request seals a verifiable audit entry ------

#[test]
fn a_served_request_seals_a_verifiable_audit_entry() {
    // Serve a remote request …
    let c = TierContext {
        capability: DeviceCapability {
            ram_mb: 1024,
            has_accelerator: false,
            on_device_model: false,
        },
        connectivity: Connectivity::Online,
        allow_egress: true,
        privacy: PrivacyPreference::Standard,
        complexity: TaskComplexity::Heavy,
    };
    let request = InferenceRequest::new("analyse", vec!["obj:1".into()]);
    let served = serve(
        &c,
        &request,
        Some(&MockOnDeviceRuntime),
        Some(&MockRemoteInference),
        &PrivacyFirstReasoner,
    )
    .unwrap();

    // … and seal its audit note into a PQ-signed ledger.
    let scheme = TestSig::new(ALG);
    let (vk, sk) = scheme.keypair(&mut SeedRng::new(7)).unwrap();
    let mut writer = AuditWriter::new(
        InMemoryLedger::new(),
        Box::new(TestSig::new(ALG)),
        sk,
        vk.clone(),
        LedgerId("client-inference".into()),
    );
    let seq = seal_audit(
        &mut writer,
        &served.audit,
        "device:analyst-7",
        "mimir:inference",
        "2026-07-09T12:00:00Z",
    )
    .unwrap();
    assert_eq!(seq, 0);

    // The chain verifies — the inference is auditable and tamper-evident.
    let provider = test_provider();
    let resolver = |alg: SigId| if alg == ALG { Some(vk.clone()) } else { None };
    verify_chain(writer.entries(), &provider, writer.ledger_id(), &resolver).unwrap();
    assert!(writer.entries()[0].record.action.0.contains("tier=Remote"));
}
