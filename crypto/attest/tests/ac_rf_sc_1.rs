//! RF-SC-1 — the real signer. Acceptance criteria AC-RF-SC-1a … 1f.
//!
//! Before this, `mg-sign` printed a stub message and returned success, so an
//! artefact that was never signed passed the release gate. These tests are the
//! contract that says it cannot any more. Each maps to one criterion in
//! `docs/dev/markdown/SAD_09_Completion_Plan.md`.
//!
//! Everything here runs offline with no network. `[NFR-PRV-1]`
//!
//! These exercise the *signing* side, so they need key custody. The
//! verifier-only build (`--no-default-features`, which is how `bifrost-artefact`
//! and `model-server` consume this crate) compiles them out; `mg-verify`'s
//! `ac_rf_sc_2.rs` covers verification from that side.

#![cfg(feature = "custody")]

use std::path::{Path, PathBuf};

use attest::{
    att_path, sig_path, verify_artefact, DevKeyFile, FileSigner, Keystore, KmsSigner, Provenance,
    ReproEntry, SignError, TrustAnchors, VerifyError,
};
use kms::{KmsError, SigningRoot};
use pqc_agility::{SigId, Signature, VerifyingKey};

// --- fixtures ----------------------------------------------------------------

const SBOM: &str = r#"{
  "bomFormat": "CycloneDX",
  "specVersion": "1.5",
  "version": 1,
  "components": [
    { "type": "library", "name": "aes-gcm", "version": "0.10.3", "purl": "pkg:cargo/aes-gcm@0.10.3" },
    { "type": "library", "name": "fips204", "version": "0.4.6", "purl": "pkg:cargo/fips204@0.4.6" }
  ]
}"#;

/// A scratch directory that cleans itself up. No external crates, so no tempfile.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        // A per-test subdirectory of the OS temp dir; the tag keeps parallel
        // tests from colliding without needing a clock or randomness.
        let dir = std::env::temp_dir().join(format!("attest-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Write an artefact and its sibling SBOM; return the artefact path.
    fn artefact(&self, name: &str, contents: &[u8]) -> PathBuf {
        let p = self.path(name);
        std::fs::write(&p, contents).expect("write artefact");
        std::fs::write(attest::sbom::sbom_path(&p), SBOM).expect("write sbom");
        p
    }

    /// Write an artefact with no SBOM beside it.
    fn artefact_without_sbom(&self, name: &str, contents: &[u8]) -> PathBuf {
        let p = self.path(name);
        std::fs::write(&p, contents).expect("write artefact");
        p
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn keystore(key_id: &str) -> Keystore {
    let mut rng = pqc_agility::OsRng;
    Keystore::generate(key_id, kms::RELEASE_SIG_ALG, &mut rng).expect("generate keystore")
}

fn anchors_for(k: &Keystore) -> TrustAnchors {
    TrustAnchors::parse(&k.export_verifying_key()).expect("parse exported anchor")
}

fn provenance() -> Provenance {
    Provenance {
        source_revision: "71e2a58".into(),
        toolchain_sha256: "ab".repeat(32),
        rustc_version: "rustc 1.82.0 (f6e511eec 2024-10-15)".into(),
        repro: vec![ReproEntry {
            package: "mg-sbom".into(),
            sha256: "cd".repeat(32),
        }],
    }
}

fn sign(artefact: &Path, k: Keystore) -> Result<attest::SignOutputs, SignError> {
    let signer = KmsSigner::new(k.into_signing_root()?);
    attest::sign_artefact(artefact, None, &signer, &provenance())
}

// --- AC-RF-SC-1a — signing produces a verifiable artefact ---------------------

#[test]
fn ac_rf_sc_1a_signing_produces_a_verifiable_artefact() {
    let s = Scratch::new("1a");
    let art = s.artefact("mg-sbom", b"deployable bytes");
    let k = keystore("release-key-1");
    let anchors = anchors_for(&k);

    let out = sign(&art, k).expect("sign");

    assert!(out.sig_path.exists(), ".sig written");
    assert!(out.att_path.exists(), ".att written");
    assert_eq!(out.alg, SigId::MlDsa87, "release signing is ML-DSA-87");
    assert_eq!(out.components, 2, "SBOM component count is carried through");

    let verdict = verify_artefact(&art, &anchors).expect("verify accepts what sign produced");
    assert_eq!(verdict.key_id, "release-key-1");
    assert_eq!(verdict.alg, SigId::MlDsa87);
    assert_eq!(verdict.artefact, "mg-sbom");
    assert!(verdict.line().starts_with("ok artefact=mg-sbom digest="));
}

// --- AC-RF-SC-1b — a missing or unusable SBOM fails closed -------------------

#[test]
fn ac_rf_sc_1b_missing_sbom_fails_closed_and_writes_no_signature() {
    let s = Scratch::new("1b-missing");
    let art = s.artefact_without_sbom("mg-sbom", b"deployable bytes");

    let err = sign(&art, keystore("release-key-1")).expect_err("must refuse");

    match &err {
        SignError::SbomMissing { expected } => {
            assert!(expected.to_string_lossy().ends_with(".sbom.cdx.json"));
        }
        other => panic!("expected SbomMissing, got {other:?}"),
    }
    assert!(
        err.to_string().contains(".sbom.cdx.json"),
        "the error must name the missing SBOM: {err}"
    );
    assert!(!sig_path(&art).exists(), "no .sig may be left behind");
    assert!(!att_path(&art).exists(), "no .att may be left behind");
}

#[test]
fn ac_rf_sc_1b_unusable_sbom_fails_closed() {
    for (tag, body) in [
        ("not-cyclonedx", r#"{"hello":"world"}"#),
        (
            "no-components",
            r#"{"bomFormat":"CycloneDX","specVersion":"1.5","components":[]}"#,
        ),
        ("truncated-json", "{\"bomFormat\": \"CycloneDX\""),
        // Carries every field a CycloneDX document needs and is still not one:
        // the closing brace is gone. Only a real parse refuses this.
        ("truncated-but-complete-looking", &SBOM[..SBOM.len() - 1]),
        (
            "one-component-without-a-purl",
            r#"{"bomFormat":"CycloneDX","specVersion":"1.5","components":[
                {"name":"a","purl":"pkg:cargo/a@1"},{"name":"b"}]}"#,
        ),
    ] {
        let s = Scratch::new(&format!("1b-{tag}"));
        let art = s.artefact_without_sbom("mg-sbom", b"bytes");
        std::fs::write(attest::sbom::sbom_path(&art), body).expect("write bad sbom");

        let err = sign(&art, keystore("release-key-1"))
            .unwrap_err_or_else(|| panic!("{tag}: must refuse"));
        assert!(
            matches!(err, SignError::SbomInvalid { .. }),
            "{tag}: expected SbomInvalid, got {err:?}"
        );
        assert!(
            !sig_path(&art).exists(),
            "{tag}: no .sig may be left behind"
        );
    }
}

/// Small helper so the loop above reads cleanly.
trait UnwrapErrOr<T, E> {
    fn unwrap_err_or_else(self, f: impl FnOnce() -> E) -> E;
}

impl<T: std::fmt::Debug, E> UnwrapErrOr<T, E> for Result<T, E> {
    fn unwrap_err_or_else(self, f: impl FnOnce() -> E) -> E {
        match self {
            Err(e) => e,
            Ok(_) => f(),
        }
    }
}

// --- AC-RF-SC-1c — an unreachable custodian fails closed ---------------------

/// A custodian that refuses every request, standing in for an unreachable KMS.
struct DeadRoot {
    vk: VerifyingKey,
}

impl SigningRoot for DeadRoot {
    fn key_id(&self) -> &str {
        "release-key-1"
    }
    fn algorithm(&self) -> SigId {
        SigId::MlDsa87
    }
    fn verifying_key(&self) -> &VerifyingKey {
        &self.vk
    }
    fn sign(&self, _msg: &[u8]) -> Result<Signature, KmsError> {
        Err(KmsError::Sign("KMS unreachable".into()))
    }
}

#[test]
fn ac_rf_sc_1c_unreachable_kms_fails_closed_leaving_no_signature() {
    let s = Scratch::new("1c");
    let art = s.artefact("mg-sbom", b"deployable bytes");

    let signer = KmsSigner::new(DeadRoot {
        vk: VerifyingKey::new(SigId::MlDsa87, vec![0u8; 32]),
    });
    let err = attest::sign_artefact(&art, None, &signer, &provenance())
        .expect_err("must refuse when the custodian is unreachable");

    assert!(matches!(err, SignError::Custodian(_)), "got {err:?}");
    assert!(err.to_string().contains("KMS unreachable"));
    assert!(
        !sig_path(&art).exists(),
        "no signature file may exist on disk afterwards"
    );
}

// --- no success path without a signature a verifier accepts ------------------

#[test]
fn signing_refuses_an_algorithm_no_verifier_accepts() {
    let s = Scratch::new("unapproved-alg");
    let art = s.artefact("mg-sbom", b"deployable bytes");

    // ML-DSA-44 is a real, working algorithm — and not an approved release one,
    // so a bundle signed under it could never pass `verify_artefact`.
    let mut rng = pqc_agility::OsRng;
    let root = kms::SoftwareSigningRoot::generate(
        pqc_agility::backend::fips::provider(),
        "weak-key",
        SigId::MlDsa44,
        &mut rng,
    )
    .expect("generate");
    let err = attest::sign_artefact(&art, None, &KmsSigner::new(root), &provenance())
        .expect_err("must refuse rather than emit an unverifiable bundle");

    assert!(
        matches!(err, SignError::UnapprovedAlgorithm { .. }),
        "got {err:?}"
    );
    assert!(err.to_string().contains("ML-DSA-44"));
    assert!(!sig_path(&art).exists(), "no .sig may be left behind");
    assert!(!att_path(&art).exists(), "no .att may be left behind");
}

/// A custodian that signs with one key while advertising another's verifying
/// key — a keystore whose public half was swapped or corrupted.
struct MismatchedRoot {
    signs: kms::SoftwareSigningRoot,
    advertises: VerifyingKey,
}

impl SigningRoot for MismatchedRoot {
    fn key_id(&self) -> &str {
        "release-key-1"
    }
    fn algorithm(&self) -> SigId {
        self.signs.algorithm()
    }
    fn verifying_key(&self) -> &VerifyingKey {
        &self.advertises
    }
    fn sign(&self, msg: &[u8]) -> Result<Signature, KmsError> {
        self.signs.sign(msg)
    }
}

#[test]
fn signing_refuses_when_the_signature_does_not_match_the_advertised_key() {
    let s = Scratch::new("self-verify");
    let art = s.artefact("mg-sbom", b"deployable bytes");

    let signs = keystore("release-key-1")
        .into_signing_root()
        .expect("unseal");
    let advertises = keystore("release-key-1").verifying_key.clone();
    let signer = KmsSigner::new(MismatchedRoot { signs, advertises });

    let err = attest::sign_artefact(&art, None, &signer, &provenance())
        .expect_err("a signature its own anchor cannot verify must not be written");

    assert!(matches!(err, SignError::SelfVerifyFailed(_)), "got {err:?}");
    assert!(!sig_path(&art).exists(), "no .sig may be left behind");
    assert!(!att_path(&art).exists(), "no .att may be left behind");
}

// --- AC-RF-SC-1d — no ambient key material -----------------------------------

/// Both halves of AC-RF-SC-1d live in one test on purpose: they mutate the same
/// process-wide environment variable, and `cargo test` runs tests in parallel
/// threads of a single process. Splitting them would make the pair race, and a
/// flaky security test is worse than none.
#[test]
fn ac_rf_sc_1d_file_signing_requires_the_explicit_env_opt_in() {
    let restore = std::env::var(attest::backend::FILE_SIGNING_ENV).ok();

    // --- refused without the opt-in ---
    std::env::remove_var(attest::backend::FILE_SIGNING_ENV);

    let mut rng = pqc_agility::OsRng;
    let provider = pqc_agility::backend::fips::provider();
    let (vk, sk) = provider
        .sig_for(SigId::MlDsa87)
        .expect("ml-dsa-87 available")
        .keypair(&mut rng)
        .expect("keypair");

    let err = FileSigner::new(
        pqc_agility::backend::fips::provider(),
        "dev-key",
        vk.clone(),
        sk,
    )
    .expect_err("a plaintext key file must be refused without the opt-in");
    assert!(matches!(err, SignError::FileSigningNotPermitted));
    assert!(err.to_string().contains("MIDGARD_ALLOW_FILE_SIGNING_KEY"));

    // A value other than exactly "1" must not enable the weaker mode either.
    for bad in ["0", "true", "yes", ""] {
        std::env::set_var(attest::backend::FILE_SIGNING_ENV, bad);
        assert!(
            !attest::backend::file_signing_permitted(),
            "{bad:?} must not enable file signing"
        );
    }

    // --- permitted, and fully functional, with the opt-in ---
    std::env::set_var(attest::backend::FILE_SIGNING_ENV, "1");

    let s = Scratch::new("1d-optin");
    let art = s.artefact("mg-sbom", b"deployable bytes");

    let mut rng = pqc_agility::OsRng;
    let provider = pqc_agility::backend::fips::provider();
    let (vk, sk) = provider
        .sig_for(SigId::MlDsa87)
        .unwrap()
        .keypair(&mut rng)
        .unwrap();
    let dev = DevKeyFile {
        key_id: "dev-key".into(),
        alg: SigId::MlDsa87,
        verifying_key: vk.clone(),
        signing_key: sk,
    };
    let signer = FileSigner::new(
        pqc_agility::backend::fips::provider(),
        &dev.key_id,
        vk.clone(),
        pqc_agility::SigningKey::new(
            dev.alg,
            pqc_agility::SecretBytes::new(dev.signing_key.expose().to_vec()),
        ),
    )
    .expect("permitted with the opt-in");

    attest::sign_artefact(&art, None, &signer, &provenance()).expect("sign");

    let mut anchors = TrustAnchors::new();
    anchors.insert("dev-key", vk);
    verify_artefact(&art, &anchors).expect("a dev-signed artefact still verifies normally");

    match restore {
        Some(v) => std::env::set_var(attest::backend::FILE_SIGNING_ENV, v),
        None => std::env::remove_var(attest::backend::FILE_SIGNING_ENV),
    }
}

// --- AC-RF-SC-1e — domain separation and artefact binding --------------------

#[test]
fn ac_rf_sc_1e_a_signature_from_another_artefact_does_not_verify() {
    let s = Scratch::new("1e");
    let a = s.artefact("artefact-a", b"payload A");
    let b = s.artefact("artefact-b", b"payload B");

    let k = keystore("release-key-1");
    let anchors = anchors_for(&k);
    let root = k.into_signing_root().expect("unseal");
    let signer = KmsSigner::new(root);

    attest::sign_artefact(&a, None, &signer, &provenance()).expect("sign A");
    attest::sign_artefact(&b, None, &signer, &provenance()).expect("sign B");

    verify_artefact(&b, &anchors).expect("B verifies under its own signature");

    // Move A's signature alongside B.
    std::fs::copy(sig_path(&a), sig_path(&b)).expect("copy A's .sig over B's");
    assert!(
        matches!(
            verify_artefact(&b, &anchors),
            Err(VerifyError::BadSignature)
        ),
        "A's signature must not verify B"
    );
}

#[test]
fn ac_rf_sc_1e_tampering_with_any_bound_file_is_caught() {
    /// Which file of the bound bundle to corrupt.
    type Target = fn(&Path) -> PathBuf;

    let targets: [(&str, Target); 3] = [
        ("artefact", |a| a.to_path_buf()),
        ("sbom", attest::sbom::sbom_path),
        ("attestation", att_path),
    ];
    for (tag, target) in targets {
        let s = Scratch::new(&format!("1e-{tag}"));
        let art = s.artefact("mg-sbom", b"deployable bytes");
        let k = keystore("release-key-1");
        let anchors = anchors_for(&k);
        sign(&art, k).expect("sign");
        verify_artefact(&art, &anchors).expect("verifies before tampering");

        // Flip one byte of the target file.
        let p = target(&art);
        let mut bytes = std::fs::read(&p).expect("read target");
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        std::fs::write(&p, &bytes).expect("write tampered target");

        let err = verify_artefact(&art, &anchors)
            .expect_err(&format!("{tag}: a single flipped byte must be caught"));
        assert!(
            matches!(
                err,
                VerifyError::BadSignature | VerifyError::SbomInvalid { .. }
            ),
            "{tag}: expected a rejection, got {err:?}"
        );
    }
}

#[test]
fn ac_rf_sc_1e_a_foreign_domain_label_is_rejected() {
    let s = Scratch::new("1e-domain");
    let art = s.artefact("mg-sbom", b"deployable bytes");
    let k = keystore("release-key-1");
    let anchors = anchors_for(&k);
    sign(&art, k).expect("sign");

    // Rewrite the envelope with a different domain — the shape a cross-protocol
    // replay would take.
    let bytes = std::fs::read(sig_path(&art)).expect("read sig");
    let env = attest::envelope::SigEnvelope::parse(&bytes).expect("parse");
    let forged = attest::envelope::SigEnvelope {
        domain: b"bifrost-manifest-v1".to_vec(),
        ..env
    };
    std::fs::write(sig_path(&art), forged.encode()).expect("write forged sig");

    assert!(
        matches!(
            verify_artefact(&art, &anchors),
            Err(VerifyError::WrongDomain { .. })
        ),
        "a signature made under another protocol's domain must be refused"
    );
}

// --- AC-RF-SC-1f — the attestation is reproducible ---------------------------

#[test]
fn ac_rf_sc_1f_attestation_is_byte_identical_across_runs() {
    let s = Scratch::new("1f");
    let art = s.artefact("mg-sbom", b"deployable bytes");

    let k = keystore("release-key-1");
    let root = k.into_signing_root().expect("unseal");
    let signer = KmsSigner::new(root);

    attest::sign_artefact(&art, None, &signer, &provenance()).expect("first sign");
    let first_att = std::fs::read(att_path(&art)).expect("read .att");
    let first_sig = std::fs::read(sig_path(&art)).expect("read .sig");

    attest::sign_artefact(&art, None, &signer, &provenance()).expect("second sign");
    let second_att = std::fs::read(att_path(&art)).expect("read .att");
    let second_sig = std::fs::read(sig_path(&art)).expect("read .sig");

    assert_eq!(
        first_att, second_att,
        "the attestation is a pure function of its inputs and must be byte-identical"
    );

    // Recorded deliberately: ML-DSA hedged signing draws fresh randomness, so the
    // signature differs between runs. That is the safer construction and it costs
    // nothing — reproducibility is established by mg-repro over the binary, and
    // verification checks that the signature *verifies*, never that it is
    // byte-equal. See preimage.rs and SAD_09_Completion_Plan.md RF-SC-1.
    assert_ne!(
        first_sig, second_sig,
        "hedged ML-DSA signatures are expected to differ between runs"
    );
}

// --- verification refusals that are not tied to one criterion ---------------

#[test]
fn verification_refuses_without_trust_anchors() {
    let s = Scratch::new("anchors");
    let art = s.artefact("mg-sbom", b"deployable bytes");
    sign(&art, keystore("release-key-1")).expect("sign");

    assert!(
        matches!(
            verify_artefact(&art, &TrustAnchors::new()),
            Err(VerifyError::NoTrustAnchors)
        ),
        "absent anchors are a refusal, never a bypass"
    );
}

#[test]
fn verification_refuses_a_valid_signature_from_an_untrusted_key() {
    let s = Scratch::new("untrusted");
    let art = s.artefact("mg-sbom", b"deployable bytes");
    sign(&art, keystore("rogue-key")).expect("sign");

    // Anchors that trust a different key entirely.
    let anchors = anchors_for(&keystore("release-key-1"));
    assert!(
        matches!(
            verify_artefact(&art, &anchors),
            Err(VerifyError::UntrustedKey { .. })
        ),
        "a cryptographically valid signature from an unknown key is still refused"
    );
}

#[test]
fn verification_refuses_when_a_sidecar_is_absent() {
    for (tag, victim) in [
        ("sig", sig_path as fn(&Path) -> PathBuf),
        ("att", att_path),
        ("sbom", attest::sbom::sbom_path),
    ] {
        let s = Scratch::new(&format!("sidecar-{tag}"));
        let art = s.artefact("mg-sbom", b"deployable bytes");
        let k = keystore("release-key-1");
        let anchors = anchors_for(&k);
        sign(&art, k).expect("sign");

        std::fs::remove_file(victim(&art)).expect("remove sidecar");
        assert!(
            matches!(
                verify_artefact(&art, &anchors),
                Err(VerifyError::SidecarMissing { .. })
            ),
            "{tag}: a missing sidecar must be a refusal"
        );
    }
}

#[test]
fn trust_anchor_parsing_rejects_malformed_lines() {
    assert!(TrustAnchors::parse("ML-DSA-87 key deadbeef").is_ok());
    assert!(TrustAnchors::parse("# comment only\n\n")
        .expect("parses")
        .is_empty());
    assert!(
        TrustAnchors::parse("ML-DSA-87 key").is_err(),
        "too few fields"
    );
    assert!(
        TrustAnchors::parse("SLH-DSA-SHA2-128s key deadbeef").is_err(),
        "not an approved release algorithm"
    );
    assert!(
        TrustAnchors::parse("ML-DSA-87 key nothex!!").is_err(),
        "key must be hex"
    );
}

#[test]
fn trust_anchors_refuse_one_key_id_bound_to_two_keys() {
    // Within one file: listed twice is an error even when the lines agree,
    // because the operator's intent is no longer readable from the file.
    assert!(
        TrustAnchors::parse("ML-DSA-87 key aabb\nML-DSA-87 key ccdd\n").is_err(),
        "a repeated key id in one file must be refused"
    );

    // Across files: the same anchor twice is harmless, a conflicting one is not.
    let mut anchors = TrustAnchors::parse("ML-DSA-87 key aabb\n").unwrap();
    anchors
        .merge(TrustAnchors::parse("ML-DSA-87 key aabb\n").unwrap())
        .expect("an identical anchor merges");
    anchors
        .merge(TrustAnchors::parse("ML-DSA-87 other eeff\n").unwrap())
        .expect("a different key id merges");
    assert_eq!(anchors.len(), 2);

    for conflicting in ["ML-DSA-87 key ccdd\n", "ML-DSA-65 key aabb\n"] {
        let e = anchors
            .merge(TrustAnchors::parse(conflicting).unwrap())
            .expect_err("which key is trusted must not depend on merge order");
        assert!(e.contains("two different keys"), "{e}");
    }
    assert_eq!(anchors.len(), 2, "a refused merge adds nothing for that id");
}

#[test]
fn the_verdict_line_holds_each_field_exactly_once() {
    let verdict = attest::Verdict {
        artefact: "app key=evil alg=none".into(),
        artefact_sha256: [0xab; 32],
        key_id: "release key".into(),
        alg: SigId::MlDsa87,
        components: 3,
    };
    let line = verdict.line();
    assert_eq!(
        line,
        format!(
            "ok artefact=app%20key%3Devil%20alg%3Dnone digest={} key=release%20key \
             alg=ML-DSA-87 components=3",
            "ab".repeat(32)
        )
    );
    assert_eq!(line.split_whitespace().count(), 6);
}
