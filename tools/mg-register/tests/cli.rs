//! RF-SC-5 — admission to the Bifrost registry, through the **actual binary**.
//! AC-RF-SC-5c / 5d.
//!
//! The claim under test: the registry records a released artefact with its
//! digest, its SBOM digest, the release key that signed it and its provenance —
//! and it records nothing that did not pass the release gate, whoever holds the
//! admission key. Everything here runs offline. `[NFR-PRV-1]`

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use attest::{Keystore, KmsSigner, Provenance};
use pqc_agility::SigId;

const EXE: &str = env!("CARGO_BIN_EXE_mg-register");

const SBOM: &str = r#"{
  "bomFormat": "CycloneDX",
  "specVersion": "1.5",
  "version": 1,
  "components": [
    { "type": "library", "name": "aes-gcm", "version": "0.10.3", "purl": "pkg:cargo/aes-gcm@0.10.3" }
  ]
}"#;

/// A scratch directory with a release key, an admission key, their anchor files,
/// and one gated artefact.
struct Fixture {
    dir: PathBuf,
    artefact: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("mg-register-cli-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let f = Self {
            artefact: dir.join("http-gateway"),
            dir,
        };

        f.keystore("release", "release-key-1", SigId::MlDsa87);
        f.keystore("admission", "bifrost-admission-1", SigId::MlDsa65);

        std::fs::write(&f.artefact, b"the gated binary").unwrap();
        std::fs::write(attest::sbom::sbom_path(&f.artefact), SBOM).unwrap();
        f.sign_with("release");
        f
    }

    /// Generate `<name>.keystore` and its exported anchor `<name>.pub`.
    fn keystore(&self, name: &str, key_id: &str, alg: SigId) {
        let mut rng = pqc_agility::OsRng;
        let ks = Keystore::generate(key_id, alg, &mut rng).expect("generate keystore");
        ks.write(&self.path(&format!("{name}.keystore"))).unwrap();
        std::fs::write(self.path(&format!("{name}.pub")), ks.export_verifying_key()).unwrap();
    }

    /// Run the signing stage of the gate over the artefact with a named keystore.
    fn sign_with(&self, keystore: &str) {
        let ks = Keystore::read(&self.path(&format!("{keystore}.keystore"))).unwrap();
        let signer = KmsSigner::new(ks.into_signing_root().expect("unseal"));
        attest::sign_artefact(
            &self.artefact,
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

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn registry(&self) -> PathBuf {
        self.path("registry")
    }

    /// `mg-register` with the standard arguments, any of which `with` overrides.
    fn register(&self, with: &[(&str, &str)]) -> Output {
        let mut args: Vec<(String, String)> = [
            ("registry", self.registry()),
            ("artefact", self.artefact.clone()),
            ("keystore", self.path("admission.keystore")),
            ("release-anchor", self.path("release.pub")),
            ("admission-anchor", self.path("admission.pub")),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string_lossy().into_owned()))
        .collect();
        args.push(("id".into(), "http-gateway".into()));
        args.push(("version".into(), "1.4.0".into()));
        for (k, v) in with {
            args.retain(|(key, _)| key != k);
            args.push((k.to_string(), v.to_string()));
        }
        let mut cmd = Command::new(EXE);
        for (k, v) in args {
            cmd.arg(format!("--{k}")).arg(v);
        }
        cmd.output().expect("run mg-register")
    }

    fn list(&self) -> Output {
        Command::new(EXE)
            .arg("list")
            .arg("--registry")
            .arg(self.registry())
            .arg("--admission-anchor")
            .arg(self.path("admission.pub"))
            .output()
            .expect("run mg-register list")
    }

    /// How many artefacts the registry directory holds on disk.
    fn stored(&self) -> usize {
        match std::fs::read_dir(self.registry()) {
            Ok(entries) => entries
                .filter(|e| {
                    e.as_ref()
                        .unwrap()
                        .path()
                        .extension()
                        .is_some_and(|x| x == "artefact")
                })
                .count(),
            Err(_) => 0,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

// --- AC-RF-SC-5d — what the registry records ---------------------------------

#[test]
fn ac_rf_sc_5d_a_gated_artefact_is_recorded_with_its_digests_key_and_provenance() {
    let f = Fixture::new("record");
    std::fs::write(f.path("repro.json"), b"{ \"packages\": [] }\n").unwrap();

    let out = f.register(&[
        ("repro", s(&f.path("repro.json"))),
        ("component", "service"),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));

    // One machine-readable line: digest, SBOM digest, the release key id.
    let line = stdout(&out);
    let fields: std::collections::BTreeMap<&str, &str> = line
        .trim()
        .strip_prefix("registered ")
        .expect("verdict line")
        .split_whitespace()
        .filter_map(|kv| kv.split_once('='))
        .collect();
    let digest = attest::hex::hex(&attest::preimage::digest(b"the gated binary"));
    assert_eq!(fields["id"], "http-gateway");
    assert_eq!(fields["version"], "1.4.0");
    assert_eq!(fields["component"], "service");
    assert_eq!(fields["digest"], digest);
    assert_eq!(
        fields["sbom"],
        attest::hex::hex(&attest::preimage::digest(SBOM.as_bytes()))
    );
    assert_eq!(fields["release-key"], "release-key-1");
    assert_eq!(fields["attestations"], "3");

    // The registry itself answers "what was published": the binary, and beside
    // it the release signature, the provenance and the reproducibility report.
    let out = f.list();
    assert!(out.status.success(), "{}", stderr(&out));
    let listed = stdout(&out);
    let rows: Vec<Vec<&str>> = listed
        .lines()
        .map(|l| l.split_whitespace().collect())
        .collect();
    let names: Vec<(&str, &str, &str)> = rows.iter().map(|r| (r[0], r[1], r[2])).collect();
    assert_eq!(
        names,
        [
            ("http-gateway", "1.4.0", "service"),
            ("http-gateway.att", "1.4.0", "attestation"),
            ("http-gateway.repro.json", "1.4.0", "attestation"),
            ("http-gateway.sig", "1.4.0", "attestation"),
        ]
    );
    assert_eq!(rows[0][3], digest, "the recorded digest is the binary's");
    // The recorded signature is byte-for-byte the gate's.
    let sig = std::fs::read(attest::sig_path(&f.artefact)).unwrap();
    assert_eq!(
        rows[3][3],
        attest::hex::hex(&attest::preimage::digest(&sig))
    );
}

// --- AC-RF-SC-5c — nothing ungated is admitted --------------------------------

#[test]
fn an_artefact_that_did_not_pass_the_gate_is_never_admitted() {
    type Break = fn(&Fixture);
    let cases: [(&str, Break, &str); 5] = [
        (
            "never signed",
            |f| {
                std::fs::remove_file(attest::sig_path(&f.artefact)).unwrap();
                std::fs::remove_file(attest::att_path(&f.artefact)).unwrap();
            },
            "missing required sidecar",
        ),
        (
            "changed after signing",
            |f| std::fs::write(&f.artefact, b"a different binary").unwrap(),
            "signature does not verify",
        ),
        (
            "its SBOM swapped",
            |f| {
                std::fs::write(
                    attest::sbom::sbom_path(&f.artefact),
                    SBOM.replace("0.10.3", "0.10.4"),
                )
                .unwrap()
            },
            "signature does not verify",
        ),
        (
            "its provenance swapped",
            |f| std::fs::write(attest::att_path(&f.artefact), b"{}\n").unwrap(),
            "signature does not verify",
        ),
        (
            // Signed, validly — by the admission key, which is not a release key.
            // Holding the key that admits is not holding the key that releases.
            "signed by the wrong key",
            |f| f.sign_with("admission"),
            "not a trusted release key",
        ),
    ];
    for (tag, break_it, expected) in cases {
        let f = Fixture::new(&format!("ungated-{}", tag.replace(' ', "-")));
        break_it(&f);

        let out = f.register(&[]);
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(1), "{tag}: must be refused: {err}");
        assert!(
            err.contains("has not passed the release gate") && err.contains(expected),
            "{tag}: {err}"
        );
        assert!(stdout(&out).is_empty(), "{tag}: nothing on stdout");
        assert_eq!(f.stored(), 0, "{tag}: nothing may reach the registry");
    }
}

#[test]
fn the_admission_key_must_be_the_pinned_one() {
    let f = Fixture::new("admission");
    f.keystore("rogue", "bifrost-admission-1", SigId::MlDsa65);
    f.keystore("stranger", "somebody-else", SigId::MlDsa65);

    // Same key id, different key: a keystore cannot claim the pinned identity.
    let out = f.register(&[("keystore", s(&f.path("rogue.keystore")))]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("holds a different key"),
        "{}",
        stderr(&out)
    );

    // A key the anchor file does not pin at all.
    let out = f.register(&[("keystore", s(&f.path("stranger.keystore")))]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("is not the admission key pinned"),
        "{}",
        stderr(&out)
    );

    // The release keystore: the wrong root, and the wrong algorithm.
    let out = f.register(&[
        ("keystore", s(&f.path("release.keystore"))),
        ("admission-anchor", s(&f.path("release.pub"))),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("admits only ML-DSA-65"),
        "{}",
        stderr(&out)
    );

    assert_eq!(f.stored(), 0, "none of these may start a registry");
}

#[test]
fn a_registry_admitted_under_one_key_does_not_open_under_another() {
    let f = Fixture::new("reopen");
    assert!(f.register(&[]).status.success());
    assert_eq!(f.stored(), 3);

    // Listing against an anchor file that pins some other key fails closed:
    // every resident artefact is re-verified on open.
    f.keystore("other", "bifrost-admission-1", SigId::MlDsa65);
    let out = Command::new(EXE)
        .arg("list")
        .arg("--registry")
        .arg(f.registry())
        .arg("--admission-anchor")
        .arg(f.path("other.pub"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(stdout(&out).is_empty());

    // A directory that is not there is an error, and listing does not create it.
    let out = Command::new(EXE)
        .arg("list")
        .arg("--registry")
        .arg(f.path("no-such-registry"))
        .arg("--admission-anchor")
        .arg(f.path("admission.pub"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(!f.path("no-such-registry").exists());
}

// --- exit 0 means registered --------------------------------------------------

#[test]
fn help_cannot_turn_a_registration_into_a_success() {
    let f = Fixture::new("help");
    let out = Command::new(EXE)
        .arg("--registry")
        .arg(f.registry())
        .arg("--help")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "--help beside arguments is a usage error"
    );
    assert!(stdout(&out).is_empty());
    assert_eq!(f.stored(), 0);

    let out = Command::new(EXE).arg("--help").output().unwrap();
    assert!(out.status.success());
    assert!(stdout(&out).contains("USAGE:"));
}
