//! RF-SC-2 — the real verifier. Acceptance criteria AC-RF-SC-2a … 2e.
//!
//! Before this, `mg-verify` printed a stub message and returned success, so the
//! pre-publish gate accepted anything. These tests run the **actual binary** and
//! assert its exit code, because that is what CI and the deploy path consume —
//! a library that refuses correctly while the CLI exits 0 would be no better
//! than the stub.
//!
//! The table below has one row per rejection reason. Every row must exit
//! non-zero; there is exactly one row that exits 0.
//!
//! Everything here runs offline with no network. `[NFR-PRV-1]`

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use attest::{att_path, sig_path, Keystore, KmsSigner, Provenance, SigningBackend};

const EXE: &str = env!("CARGO_BIN_EXE_mg-verify");

const SBOM: &str = r#"{
  "bomFormat": "CycloneDX",
  "specVersion": "1.5",
  "version": 1,
  "components": [
    { "type": "library", "name": "aes-gcm", "version": "0.10.3", "purl": "pkg:cargo/aes-gcm@0.10.3" },
    { "type": "library", "name": "fips204", "version": "0.4.6", "purl": "pkg:cargo/fips204@0.4.6" }
  ]
}"#;

/// A scratch directory holding one signed artefact bundle plus its anchor file.
struct Bundle {
    dir: PathBuf,
    artefact: PathBuf,
    anchors: PathBuf,
}

impl Bundle {
    /// Build a valid, signed, verifiable bundle on disk.
    fn good(tag: &str) -> Self {
        Self::signed_by(tag, "release-key-1", "release-key-1")
    }

    /// Build a bundle signed by `signing_key_id` but whose anchor file trusts
    /// only `trusted_key_id` — equal for the happy path, different for AC-2c.
    fn signed_by(tag: &str, signing_key_id: &str, trusted_key_id: &str) -> Self {
        Self::build(tag, "mg-sbom", signing_key_id, trusted_key_id)
    }

    /// A fresh scratch directory holding an artefact called `name` and nothing else.
    fn scratch(tag: &str, name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("mg-verify-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");

        let artefact = dir.join(name);
        std::fs::write(&artefact, b"deployable bytes").expect("write artefact");
        let anchors = dir.join("anchors.txt");
        Self {
            dir,
            artefact,
            anchors,
        }
    }

    fn build(tag: &str, name: &str, signing_key_id: &str, trusted_key_id: &str) -> Self {
        let b = Self::scratch(tag, name);
        b.sign(signing_key_id, trusted_key_id);
        b
    }

    /// A scratch directory standing in for an image's app directory: the artefact is
    /// a copy of this mg-verify binary called `svc`, so a test can see whether it was
    /// started (it prints its usage text when given `--help`). Signed or not.
    fn service(tag: &str, signed: bool) -> Self {
        Self::service_from(tag, Path::new(EXE), signed)
    }

    /// [`Bundle::service`], the artefact a copy of `binary`.
    fn service_from(tag: &str, binary: &Path, signed: bool) -> Self {
        let b = Self::scratch(tag, "svc");
        std::fs::copy(binary, &b.artefact).expect("copy the binary in as svc");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&b.artefact, std::fs::Permissions::from_mode(0o755))
                .expect("make svc executable");
        }
        if signed {
            b.sign("release-key-1", "release-key-1");
        } else {
            std::fs::write(b.sbom(), SBOM).expect("write sbom");
            std::fs::write(&b.anchors, "ML-DSA-87 release-key-1 aabb\n").expect("write anchors");
        }
        b
    }

    /// Sign the artefact (with an SBOM written beside it) by `signing_key_id`, and
    /// write an anchor file trusting `trusted_key_id`.
    fn sign(&self, signing_key_id: &str, trusted_key_id: &str) {
        let b = self;
        std::fs::write(b.sbom(), SBOM).expect("write sbom");

        let mut rng = pqc_agility::OsRng;
        let signing = Keystore::generate(signing_key_id, kms::RELEASE_SIG_ALG, &mut rng)
            .expect("generate signing keystore");

        // The anchor file trusts `trusted_key_id`: the same keystore when they
        // match, an unrelated one when they do not.
        if signing_key_id == trusted_key_id {
            std::fs::write(&b.anchors, signing.export_verifying_key()).expect("write anchors");
        } else {
            let other = Keystore::generate(trusted_key_id, kms::RELEASE_SIG_ALG, &mut rng)
                .expect("generate other keystore");
            std::fs::write(&b.anchors, other.export_verifying_key()).expect("write anchors");
        }

        let signer = KmsSigner::new(signing.into_signing_root().expect("unseal"));
        attest::sign_artefact(
            &b.artefact,
            None,
            &signer,
            &Provenance {
                source_revision: "71e2a58".into(),
                toolchain_sha256: "ab".repeat(32),
                rustc_version: "rustc 1.82.0".into(),
                repro: Vec::new(),
            },
        )
        .expect("sign");
    }

    /// `mg-verify --trust-anchor <anchors> --run-service <args>`, as a release image's
    /// entrypoint runs it, with this bundle's directory as the app directory and
    /// `envs` set (nothing else is inherited but PATH).
    fn run_service(&self, envs: &[(&str, &str)], args: &[&str]) -> Output {
        let mut cmd = Command::new(EXE);
        cmd.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("MIDGARD_APP_DIR", &self.dir)
            .arg("--trust-anchor")
            .arg(&self.anchors)
            .arg("--run-service")
            .args(args);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        cmd.output().expect("run mg-verify --run-service")
    }

    /// A bundle with a **genuine signature over whatever SBOM it is given**.
    ///
    /// `sign_artefact` refuses to sign an unusable SBOM, so this assembles the
    /// bundle from the format's own parts instead. It is what lets a test show
    /// that the verifier's SBOM gate refuses on its own account — that a valid
    /// signature does not vouch for a bill of materials nobody could scan.
    fn signed_over_sbom(tag: &str, sbom: &str) -> Self {
        let b = Self::scratch(tag, "mg-sbom");
        std::fs::write(b.sbom(), sbom).expect("write sbom");
        let att = b"{}\n";
        std::fs::write(att_path(&b.artefact), att).expect("write att");

        let mut rng = pqc_agility::OsRng;
        let keystore = Keystore::generate("release-key-1", kms::RELEASE_SIG_ALG, &mut rng)
            .expect("generate keystore");
        std::fs::write(&b.anchors, keystore.export_verifying_key()).expect("write anchors");
        let signer = KmsSigner::new(keystore.into_signing_root().expect("unseal"));

        let digest = attest::preimage::digest;
        let signature = signer
            .sign(&attest::preimage::preimage(
                &digest(b"deployable bytes"),
                &digest(sbom.as_bytes()),
                &digest(att),
            ))
            .expect("sign");
        let env = attest::envelope::SigEnvelope {
            domain: attest::DOMAIN_LABEL.to_vec(),
            key_id: "release-key-1".into(),
            signature,
        };
        std::fs::write(sig_path(&b.artefact), env.encode()).expect("write sig");
        b
    }

    fn verify(&self) -> Output {
        Command::new(EXE)
            .arg(&self.artefact)
            .arg("--trust-anchor")
            .arg(&self.anchors)
            .output()
            .expect("run mg-verify")
    }

    fn sbom(&self) -> PathBuf {
        attest::sbom::sbom_path(&self.artefact)
    }
}

impl Drop for Bundle {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Flip the last byte of a file.
fn flip_last_byte(p: &Path) {
    let mut bytes = std::fs::read(p).expect("read");
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    std::fs::write(p, &bytes).expect("write");
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

// --- AC-RF-SC-2e — the positive case, and the verdict is machine-readable ----

#[test]
fn ac_rf_sc_2e_valid_bundle_exits_zero_with_a_parseable_verdict() {
    let b = Bundle::good("ok");
    let out = b.verify();

    assert!(
        out.status.success(),
        "a valid bundle must verify: {}",
        stderr(&out)
    );

    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(line.starts_with("ok "), "verdict line: {line:?}");

    // Parse the named fields, the way a CI script would.
    let fields: std::collections::BTreeMap<&str, &str> = line["ok ".len()..]
        .split_whitespace()
        .filter_map(|kv| kv.split_once('='))
        .collect();

    assert_eq!(fields.get("artefact"), Some(&"mg-sbom"));
    assert_eq!(fields.get("key"), Some(&"release-key-1"));
    assert_eq!(fields.get("components"), Some(&"2"));
    assert_eq!(fields.get("alg"), Some(&"ML-DSA-87"));
    let digest = fields.get("digest").expect("digest field");
    assert_eq!(digest.len(), 64, "sha256 as hex");
    assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn a_file_name_cannot_plant_a_field_in_the_verdict() {
    // The artefact's name is whatever the file is called. Unencoded, this one
    // would put a second `key=` in the line a script parses.
    let b = Bundle::build(
        "spoof",
        "mg-sbom key=attacker-key",
        "release-key-1",
        "release-key-1",
    );
    let out = b.verify();
    assert!(out.status.success(), "{}", stderr(&out));

    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let fields: Vec<(&str, &str)> = line["ok ".len()..]
        .split_whitespace()
        .filter_map(|kv| kv.split_once('='))
        .collect();
    let names: Vec<&str> = fields.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        names,
        ["artefact", "digest", "key", "alg", "components"],
        "each field exactly once, in order: {line:?}"
    );
    assert_eq!(fields[0].1, "mg-sbom%20key%3Dattacker-key");
    assert_eq!(fields[2].1, "release-key-1");
}

// --- exit 0 means verified, and nothing else ---------------------------------

#[test]
fn help_cannot_turn_a_verification_into_a_success() {
    // Unsigned: there is nothing here that could verify.
    let b = Bundle::scratch("help", "mg-sbom");
    for position in [0usize, 3] {
        let mut args = vec![
            b.artefact.to_string_lossy().into_owned(),
            "--trust-anchor".to_string(),
            b.anchors.to_string_lossy().into_owned(),
        ];
        args.insert(position, "--help".to_string());

        let out = Command::new(EXE)
            .args(&args)
            .output()
            .expect("run mg-verify");
        assert_eq!(
            out.status.code(),
            Some(2),
            "--help beside an artefact is a usage error, not a pass: {args:?}"
        );
        assert!(out.stdout.is_empty(), "nothing on stdout: {args:?}");
    }

    // On its own it is still help.
    let out = Command::new(EXE)
        .arg("--help")
        .output()
        .expect("run mg-verify");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("usage:"));
}

#[test]
fn an_artefact_that_was_never_signed_is_refused() {
    // The state the stub let through: a binary and its SBOM, straight from the
    // build, with no signing stage ever having run.
    let b = Bundle::scratch("unsigned", "mg-sbom");
    std::fs::write(b.sbom(), SBOM).unwrap();
    std::fs::write(&b.anchors, "ML-DSA-87 release-key-1 aabb\n").unwrap();

    let out = b.verify();
    assert!(
        !out.status.success(),
        "an unsigned artefact must not verify"
    );
    assert!(
        stderr(&out).contains("missing required sidecar"),
        "{}",
        stderr(&out)
    );
    assert!(out.stdout.is_empty());
}

// --- the rejection table -----------------------------------------------------

/// One row per reason a verification must be refused. Each mutates an otherwise
/// valid bundle, and each must exit non-zero.
#[test]
fn every_rejection_reason_exits_non_zero() {
    type Mutate = fn(&Bundle);

    let rows: Vec<(&str, Mutate, &str)> = vec![
        // AC-RF-SC-2b — absence is rejection.
        (
            "missing-sig",
            |b| std::fs::remove_file(sig_path(&b.artefact)).unwrap(),
            "missing required sidecar",
        ),
        (
            "missing-att",
            |b| std::fs::remove_file(att_path(&b.artefact)).unwrap(),
            "missing required sidecar",
        ),
        (
            "missing-sbom",
            |b| std::fs::remove_file(b.sbom()).unwrap(),
            "missing required sidecar",
        ),
        // AC-RF-SC-2a — every tamper is caught.
        (
            "tampered-artefact",
            |b| flip_last_byte(&b.artefact),
            "signature does not verify",
        ),
        (
            "tampered-sbom",
            |b| {
                // Change a byte inside a component version, keeping the document
                // structurally valid so the failure is the signature, not the parse.
                let s = std::fs::read_to_string(b.sbom()).unwrap();
                std::fs::write(b.sbom(), s.replace("0.10.3", "0.10.4")).unwrap();
            },
            "signature does not verify",
        ),
        (
            "tampered-att-subject",
            |b| {
                let s = std::fs::read_to_string(att_path(&b.artefact)).unwrap();
                // Rewrite the subject digest — the classic provenance swap.
                let doctored = s.replace("\"sha256\": \"", "\"sha256\": \"0");
                std::fs::write(att_path(&b.artefact), doctored).unwrap();
            },
            "signature does not verify",
        ),
        (
            "truncated-sig",
            |b| {
                let mut v = std::fs::read(sig_path(&b.artefact)).unwrap();
                v.truncate(v.len() / 2);
                std::fs::write(sig_path(&b.artefact), v).unwrap();
            },
            "malformed signature envelope",
        ),
        (
            "trailing-bytes-in-sig",
            |b| {
                let mut v = std::fs::read(sig_path(&b.artefact)).unwrap();
                v.push(0);
                std::fs::write(sig_path(&b.artefact), v).unwrap();
            },
            "malformed signature envelope",
        ),
        // Wrong domain label — a cross-protocol replay.
        (
            "wrong-domain",
            |b| {
                let bytes = std::fs::read(sig_path(&b.artefact)).unwrap();
                let env = attest::envelope::SigEnvelope::parse(&bytes).unwrap();
                let forged = attest::envelope::SigEnvelope {
                    domain: b"midgard-bifrost-artefact-v1".to_vec(),
                    ..env
                };
                std::fs::write(sig_path(&b.artefact), forged.encode()).unwrap();
            },
            "is not the artefact domain",
        ),
        // An algorithm that is not approved for release signing. The anchor file
        // cannot name one, but the envelope's own algorithm tag can.
        (
            "unapproved-algorithm",
            |b| {
                let bytes = std::fs::read(sig_path(&b.artefact)).unwrap();
                let env = attest::envelope::SigEnvelope::parse(&bytes).unwrap();
                let forged = attest::envelope::SigEnvelope {
                    signature: pqc_agility::Signature::new(
                        pqc_agility::SigId::MlDsa44,
                        env.signature.as_bytes().to_vec(),
                    ),
                    ..env
                };
                std::fs::write(sig_path(&b.artefact), forged.encode()).unwrap();
            },
            "not an approved release-signing algorithm",
        ),
        // An SBOM swapped after signing is a signature failure whatever it
        // contains: the verifier does not parse bytes it has not authenticated.
        (
            "sbom-replaced-after-signing",
            |b| std::fs::write(b.sbom(), r#"{"hello":"world"}"#).unwrap(),
            "signature does not verify",
        ),
    ];

    for (tag, mutate, expected) in rows {
        let b = Bundle::good(tag);
        assert!(
            b.verify().status.success(),
            "{tag}: the bundle must be valid before mutation"
        );

        mutate(&b);
        let out = b.verify();
        let err = stderr(&out);

        assert!(
            !out.status.success(),
            "{tag}: MUST exit non-zero, but it passed. stdout: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(
            err.contains(expected),
            "{tag}: expected stderr to contain {expected:?}, got: {err}"
        );
        assert!(
            out.stdout.is_empty(),
            "{tag}: nothing may be printed to stdout on refusal"
        );
    }
}

// --- the SBOM gate refuses on its own account --------------------------------

#[test]
fn a_valid_signature_does_not_vouch_for_an_unusable_sbom() {
    // Control: the hand-assembled bundle is a real one.
    let ok = Bundle::signed_over_sbom("sbom-control", SBOM);
    let out = ok.verify();
    assert!(
        out.status.success(),
        "control must verify: {}",
        stderr(&out)
    );

    let rows: [(&str, String); 6] = [
        ("not-cyclonedx", r#"{"hello":"world"}"#.into()),
        (
            "empty-component-list",
            r#"{"bomFormat":"CycloneDX","specVersion":"1.5","components":[]}"#.into(),
        ),
        (
            "component-without-a-purl",
            r#"{"bomFormat":"CycloneDX","specVersion":"1.5","components":[
                {"name":"a","purl":"pkg:cargo/a@1"},{"name":"b"}]}"#
                .into(),
        ),
        ("not-cyclonedx-1.5", SBOM.replace("\"1.5\"", "\"1.4\"")),
        ("truncated", SBOM[..SBOM.len() - 1].to_string()),
        ("trailing-data", format!("{SBOM} {{}}")),
    ];
    for (tag, sbom) in rows {
        let b = Bundle::signed_over_sbom(&format!("sbom-{tag}"), &sbom);
        let out = b.verify();
        let err = stderr(&out);
        assert!(!out.status.success(), "{tag}: MUST exit non-zero");
        assert!(
            err.contains("invalid CycloneDX SBOM"),
            "{tag}: the signature is genuine, so the refusal must be the SBOM's: {err}"
        );
        assert!(out.stdout.is_empty(), "{tag}: nothing on stdout on refusal");
    }
}

// --- AC-RF-SC-2c — an untrusted key is refused even when the signature is valid

#[test]
fn ac_rf_sc_2c_valid_signature_from_an_untrusted_key_is_refused() {
    let b = Bundle::signed_by("untrusted", "rogue-key", "release-key-1");
    let out = b.verify();

    assert!(!out.status.success(), "an untrusted key must be refused");
    let err = stderr(&out);
    assert!(
        err.contains("rogue-key") && err.contains("not a trusted release key"),
        "the refusal must name the key: {err}"
    );
}

// --- trust anchors are mandatory and explicit --------------------------------

#[test]
fn no_trust_anchor_is_a_refusal_not_a_bypass() {
    let b = Bundle::good("no-anchors");
    let out = Command::new(EXE)
        .arg(&b.artefact)
        .output()
        .expect("run mg-verify");

    assert!(!out.status.success(), "no anchors must not pass");
    assert_eq!(
        out.status.code(),
        Some(2),
        "missing anchors is a usage error"
    );
    assert!(
        stderr(&out).contains("requires an explicit trust anchor set"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_malformed_anchor_file_is_a_refusal() {
    let b = Bundle::good("bad-anchors");
    std::fs::write(&b.anchors, "not an anchor line").unwrap();
    let out = b.verify();
    assert!(!out.status.success(), "a malformed anchor file must refuse");
}

#[test]
fn an_anchor_for_an_unapproved_algorithm_is_refused_at_parse_time() {
    // SLH-DSA is reserved for the offline root (RF-SC-8) and is not an approved
    // release-signing algorithm, so it cannot even be loaded as an anchor —
    // defence in depth ahead of the acceptor's own algorithm check.
    let b = Bundle::good("unapproved-anchor-alg");
    std::fs::write(&b.anchors, "SLH-DSA-SHA2-128s k aabb\n").unwrap();
    let out = b.verify();
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("not an approved release algorithm"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_missing_artefact_is_a_refusal() {
    let b = Bundle::good("missing-artefact");
    std::fs::remove_file(&b.artefact).unwrap();
    let out = b.verify();
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("cannot read artefact"),
        "{}",
        stderr(&out)
    );
}

// --- AC-RF-SC-2d — one implementation ---------------------------------------

/// Every `.rs` file under `dir`, recursively.
fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn ac_rf_sc_2d_subsystems_hold_no_independent_acceptance_decision() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    // 1. Every place Bifrost and Mimir accept a signed thing routes the decision
    //    through `attest::accept`.
    for site in [
        "bifrost/artefact/src/signed.rs",
        "bifrost/bundle/src/bundle.rs",
        "bifrost/bundle/src/stream.rs",
        "mimir/model-server/src/artifact.rs",
    ] {
        let src =
            std::fs::read_to_string(root.join(site)).unwrap_or_else(|e| panic!("{site}: {e}"));
        assert!(
            src.contains("attest::accept::accept("),
            "{site} must delegate its acceptance decision to attest::accept"
        );
    }

    // 2. And nowhere in either subsystem's shipped source — not just in those
    //    files — is a signature verified any other way. A raw scheme reached via
    //    `sig_for(` or `FipsSig`, or a direct `.verify(` call, would be a second
    //    place for "acceptable" to be defined, so a new one fails here by name.
    let mut sources = Vec::new();
    for subsystem in ["bifrost", "mimir"] {
        let entries = std::fs::read_dir(root.join(subsystem)).expect("subsystem dir");
        for entry in entries {
            let src = entry.expect("dir entry").path().join("src");
            if src.is_dir() {
                rust_sources(&src, &mut sources);
            }
        }
    }
    assert!(
        sources.len() > 20,
        "the walk found only {} files — it is not looking where the code is",
        sources.len()
    );
    for path in sources {
        let src = std::fs::read_to_string(&path).expect("read source");
        for needle in [".verify(", "sig_for(", "FipsSig"] {
            assert!(
                !src.contains(needle),
                "{} contains `{needle}` — signature acceptance must go through attest::accept",
                path.display()
            );
        }
    }
}

// --- verify-before-run: `--run-service`, the release image's entrypoint -------
//
// The release image's runtime stage is distroless, with no shell, so mg-verify is
// its entrypoint: it verifies the service binary against the pinned anchor and only
// then becomes it. [AC-B-1] [NFR-PQ-3] [VLD-ANB-MIDGARD-0068]

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn run_service_refuses_an_unverified_binary_and_does_not_exec() {
    // [AC-B-1] AC-36.3
    let b = Bundle::service("run-unsigned", false);
    let out = b.run_service(&[("MIDGARD_SERVICE", "svc")], &["--help"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("refusing to start"),
        "{}",
        stderr(&out)
    );
    assert!(
        !stdout(&out).contains("usage:") && !stdout(&out).contains("verify-before-run: OK"),
        "the service must not have been started: {}",
        stdout(&out)
    );
}

#[cfg(unix)]
#[test]
fn run_service_execs_a_verified_binary_with_the_given_arguments() {
    // [AC-B-1] AC-36.4
    let b = Bundle::service("run-signed", true);
    let out = b.run_service(&[("MIDGARD_SERVICE", "svc")], &["--help"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    // The exact line deploy/images/build.sh looks for.
    let ok = text
        .find("verify-before-run: OK — starting svc")
        .unwrap_or_else(|| panic!("no OK line: {text}"));
    // Then the service's own output: svc is mg-verify, given --help by exec.
    let usage = text
        .find("usage:")
        .unwrap_or_else(|| panic!("the service did not run with its argument: {text}"));
    assert!(ok < usage, "verified first, then started: {text}");
    assert!(text.contains(&format!(
        "verify-before-run: checking {} against {}",
        b.artefact.display(),
        b.anchors.display()
    )));

    // A leading `--` is a separator, not an argument for the service.
    let out = b.run_service(&[("MIDGARD_SERVICE", "svc")], &["--", "--help"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("usage:"));
}

/// The bytes `verify_artefact_bytes` is given are what it verifies: the artefact's
/// path is only where its sidecars are found, and is never reopened. Replaced after
/// it was read, the path no longer verifies, while the bytes read before still do.
#[test]
fn verify_artefact_bytes_checks_the_bytes_given_not_the_path() {
    // [AC-B-1] [AC-RF-SC-2d] [VLD-ANB-MIDGARD-0068]
    let b = Bundle::good("bytes-not-path");
    let anchors = attest::TrustAnchors::parse(&std::fs::read_to_string(&b.anchors).unwrap())
        .expect("anchors");
    let verified = std::fs::read(&b.artefact).unwrap();
    std::fs::write(&b.artefact, b"replaced after it was read").unwrap();

    let verdict = attest::verify_artefact_bytes(&verified, &b.artefact, &anchors)
        .expect("the bytes read before the swap still verify");
    assert_eq!(verdict.artefact, "mg-sbom");
    assert_eq!(verdict.artefact_sha256, attest::preimage::digest(&verified));
    assert!(
        attest::verify_artefact(&b.artefact, &anchors).is_err(),
        "the replaced file does not"
    );
    assert!(attest::verify_artefact_bytes(b"other bytes", &b.artefact, &anchors).is_err());

    // Nor is the path needed at all: removed, its bytes still verify beside its sidecars.
    std::fs::remove_file(&b.artefact).unwrap();
    attest::verify_artefact_bytes(&verified, &b.artefact, &anchors)
        .expect("verified without the artefact's file");
}

/// Prints the executable this process image was loaded from, for
/// `run_service_execs_the_bytes_it_verified_not_the_path`, which starts this test
/// binary as a service. Ignored in a normal run.
#[test]
#[ignore = "a helper run as a service by another test"]
fn print_own_executable() {
    let exe = std::fs::read_link("/proc/self/exe").expect("readlink /proc/self/exe");
    println!("own-executable={}", exe.display());
}

/// On Linux `--run-service` executes the bytes it verified, from a sealed in-memory
/// file, never the path again: whatever is at the path after verification is not
/// what runs. The service here is this test binary, signed, asked to print the file
/// its process image came from; that is the in-memory file, not the artefact path. A
/// swap of the path between verifying and starting cannot be timed from outside, so
/// this shows the path is not reopened instead, and
/// `verify_artefact_bytes_checks_the_bytes_given_not_the_path` that what is verified
/// is the bytes read once.
#[cfg(target_os = "linux")]
#[test]
fn run_service_execs_the_bytes_it_verified_not_the_path() {
    // [AC-B-1] [NFR-PQ-3] [VLD-ANB-MIDGARD-0068]
    let me = std::env::current_exe().expect("this test binary");
    let b = Bundle::service_from("run-bytes", &me, true);
    let out = b.run_service(
        &[("MIDGARD_SERVICE", "svc")],
        &[
            "--exact",
            "print_own_executable",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("verify-before-run: OK — starting svc"),
        "{text}"
    );
    let exe = text
        .lines()
        .find_map(|l| l.split_once("own-executable=").map(|(_, exe)| exe.trim()))
        .unwrap_or_else(|| panic!("the service did not run: {text}"));
    assert!(
        exe.starts_with("/memfd:"),
        "the service ran from {exe}, not from the bytes mg-verify verified"
    );
    assert!(
        !exe.contains(&*b.dir.to_string_lossy()),
        "the artefact path was reopened: {exe}"
    );
}

#[test]
fn run_service_without_a_service_name_is_a_usage_error() {
    // [AC-B-1]
    let b = Bundle::service("run-no-name", true);
    for envs in [vec![], vec![("MIDGARD_SERVICE", "")]] {
        let out = b.run_service(&envs, &["--help"]);
        assert_eq!(out.status.code(), Some(2), "{envs:?}: {}", stderr(&out));
        assert!(
            !stdout(&out).contains("usage:"),
            "nothing started: {envs:?}"
        );
    }
    // A path is not a service name: the binary is always one the app directory holds.
    let out = b.run_service(&[("MIDGARD_SERVICE", "../svc")], &["--help"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
}

#[test]
fn run_service_with_an_artefact_path_is_a_usage_error() {
    // [AC-B-1]
    let b = Bundle::service("run-artefact", true);
    let out = b.run_service(&[("MIDGARD_SERVICE", "svc")], &["some/path"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(!stdout(&out).contains("verify-before-run: OK"));

    // Nor before the flag.
    let out = Command::new(EXE)
        .env("MIDGARD_SERVICE", "svc")
        .env("MIDGARD_APP_DIR", &b.dir)
        .arg(&b.artefact)
        .arg("--trust-anchor")
        .arg(&b.anchors)
        .args(["--run-service", "--help"])
        .output()
        .expect("run mg-verify");
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(out.stdout.is_empty());
}

#[test]
fn run_service_with_no_service_options_refuses_to_start() {
    // [AC-B-1] A container given no options is refused, not started on a default
    // address, even with a binary that verifies. [MG-SEC-06]
    let b = Bundle::service("run-no-options", true);
    for args in [&[][..], &["--"][..]] {
        let out = b.run_service(&[("MIDGARD_SERVICE", "svc")], args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("refusing to start"),
            "{}",
            stderr(&out)
        );
        assert!(!stdout(&out).contains("verify-before-run: OK"), "{args:?}");
    }
}

#[test]
fn run_service_trust_anchor_comes_only_from_its_own_command_line() {
    // [AC-B-1] The image names /trust/release-trust.pub before --run-service. Neither
    // the environment nor the container's arguments can point verification at
    // another anchor. [MG-SEC-14]
    let b = Bundle::service("run-anchor", true);
    let genuine = b.dir.join("genuine.pub");
    std::fs::copy(&b.anchors, &genuine).expect("keep the anchor that would verify");
    let other = Keystore::generate("other-key", kms::RELEASE_SIG_ALG, &mut pqc_agility::OsRng)
        .expect("generate keystore");
    std::fs::write(&b.anchors, other.export_verifying_key()).expect("write anchors");

    let g = genuine.to_string_lossy().into_owned();
    let out = b.run_service(
        &[
            ("MIDGARD_SERVICE", "svc"),
            ("MIDGARD_RELEASE_TRUST_KEY", &g),
            ("TRUST", &g),
        ],
        &["--trust-anchor", &g, "--help"],
    );
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("refusing to start"),
        "{}",
        stderr(&out)
    );
    assert!(
        !stdout(&out).contains("usage:"),
        "the service must not start"
    );
    assert!(
        !stdout(&out).contains("genuine.pub") && !stderr(&out).contains("genuine.pub"),
        "the other anchor path is never used"
    );
}
