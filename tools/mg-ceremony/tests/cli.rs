//! RT-35: the key ceremony, through the **actual binary**. [AC-RF-CA-6a] [NFR-PQ-3]
//!
//! The claims under test: the ceremony publishes exactly the three files the
//! release checks, in the forms it checks, each anchor pinning the key held in its
//! keystore; it never puts a keystore inside the checkout or names where the
//! keystores are in the record; and it refuses, before generating anything, a
//! private key offered as a signer, an existing published file, or a custody
//! directory that is not fresh. Everything here runs offline. `[NFR-PRV-1]`

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use attest::Keystore;

const EXE: &str = env!("CARGO_BIN_EXE_mg-ceremony");

// Real ed25519 public keys (public material only), with the fingerprints
// `ssh-keygen -lf` prints for them.
const ALICE: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINyw+6K7GLdHy8Z8lAxVMz5rVCCiWQhotLU9107+qSZW alice";
const ALICE_FP: &str = "SHA256:JcqaqS1Mgl8TGWjTR0WomD/A/NTvNdkS4iEPMk/24q8";
const BOB: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGbKjr8492Z+kJQPt0zuy3z47pZ0h+VRv7GwrMx2/Mq1 bob";

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("mg-ceremony-cli-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("repo/deploy/trust")).unwrap();
        std::fs::write(dir.join("repo/deploy/trust/README.md"), "anchors\n").unwrap();
        std::fs::write(dir.join("alice.pub"), ALICE).unwrap();
        std::fs::write(dir.join("bob.pub"), BOB).unwrap();
        Self(dir)
    }
    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
    fn trust(&self, file: &str) -> PathBuf {
        self.0.join("repo/deploy/trust").join(file)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(EXE)
            .arg("run")
            .arg("--repo")
            .arg(self.path("repo"))
            .args(args)
            .output()
            .expect("run mg-ceremony")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn sha256_hex(path: &Path) -> String {
    attest::hex::hex(&pqc_agility::sha256(&std::fs::read(path).unwrap()))
}

#[test]
fn a_ceremony_publishes_what_the_release_checks() {
    let s = Scratch::new("good");
    let custody = s.path("usb/keys");
    let alice = format!("alice@example.invalid={}", s.path("alice.pub").display());
    let bob = format!("bob@example.invalid={}", s.path("bob.pub").display());
    let o = s.run(&[
        "--custody",
        custody.to_str().unwrap(),
        "--year",
        "2026",
        "--officer",
        "A. Officer",
        "--witness",
        "W. Itness",
        "--signer",
        &alice,
        "--signer",
        &bob,
    ]);
    let out = text(&o);
    assert!(o.status.success(), "{out}");

    // Each anchor pins the key its keystore holds, under the right algorithm.
    let release = std::fs::read_to_string(s.trust("release-trust.pub")).unwrap();
    let bifrost = std::fs::read_to_string(s.trust("bifrost-trust.pub")).unwrap();
    assert!(
        release.starts_with("ML-DSA-87 midgard-release-2026 "),
        "{release}"
    );
    assert!(
        bifrost.starts_with("ML-DSA-65 midgard-bifrost-2026 "),
        "{bifrost}"
    );
    let held = Keystore::read(&custody.join("release.keystore")).unwrap();
    assert_eq!(held.export_verifying_key(), release);
    let held = Keystore::read(&custody.join("bifrost.keystore")).unwrap();
    assert_eq!(held.export_verifying_key(), bifrost);
    assert!(!s.path("repo/release.keystore").exists());

    let signers = std::fs::read_to_string(s.trust("allowed_signers")).unwrap();
    for who in ["alice", "bob"] {
        let prefix = format!("{who}@example.invalid namespaces=\"git\" ssh-ed25519 ");
        assert_eq!(
            signers.lines().filter(|l| l.starts_with(&prefix)).count(),
            1,
            "{signers}"
        );
    }
    assert!(
        !signers.contains(" alice\n") && !signers.contains(" bob\n"),
        "key comments kept"
    );

    // The record carries the digests and fingerprints, and not where the keys are.
    let records: Vec<PathBuf> = std::fs::read_dir(s.path("repo/docs/ops/ceremonies"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(records.len(), 1);
    let record = std::fs::read_to_string(&records[0]).unwrap();
    assert!(record.contains(ALICE_FP), "{record}");
    assert!(
        record.contains(&sha256_hex(&s.trust("release-trust.pub"))),
        "{record}"
    );
    assert!(
        record.contains(&sha256_hex(&s.trust("allowed_signers"))),
        "{record}"
    );
    assert!(record.contains("| Witness | W. Itness |"), "{record}");
    assert!(!record.contains(custody.to_str().unwrap()), "{record}");

    // The fingerprints and digests are printed for the second channel.
    assert!(out.contains(ALICE_FP), "{out}");
    assert!(
        out.contains(&sha256_hex(&s.trust("bifrost-trust.pub"))),
        "{out}"
    );

    // `check` reads the published files back and agrees.
    let c = Command::new(EXE)
        .args(["check", "--repo"])
        .arg(s.path("repo"))
        .output()
        .unwrap();
    assert!(c.status.success(), "{}", text(&c));
    assert!(text(&c).contains(&sha256_hex(&s.trust("allowed_signers"))));
}

/// Run a refused ceremony and check it refused for the stated reason, before
/// any key was generated or published.
fn refused(s: &Scratch, args: &[&str], reason: &str) {
    let o = s.run(args);
    let out = text(&o);
    assert!(!o.status.success(), "it ran: {out}");
    assert!(
        out.contains(reason),
        "refused, but not for '{reason}': {out}"
    );
    assert!(
        !s.trust("allowed_signers").exists(),
        "refused, but published something"
    );
}

#[test]
fn a_private_key_offered_as_a_signer_is_refused() {
    let s = Scratch::new("priv");
    std::fs::write(
        s.path("alice"),
        "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaA==\n-----END OPENSSH PRIVATE KEY-----\n",
    )
    .unwrap();
    let signer = format!("alice@example.invalid={}", s.path("alice").display());
    let custody = s.path("keys");
    refused(
        &s,
        &["--custody", custody.to_str().unwrap(), "--signer", &signer],
        "private key",
    );
    assert!(!custody.exists());
}

#[test]
fn a_custody_directory_inside_the_checkout_or_not_empty_is_refused() {
    let s = Scratch::new("custody");
    let signer = format!("alice@example.invalid={}", s.path("alice.pub").display());
    let inside = s.path("repo/keys");
    refused(
        &s,
        &["--custody", inside.to_str().unwrap(), "--signer", &signer],
        "inside this checkout",
    );
    let used = s.path("used");
    std::fs::create_dir_all(&used).unwrap();
    std::fs::write(used.join("old"), "x").unwrap();
    refused(
        &s,
        &["--custody", used.to_str().unwrap(), "--signer", &signer],
        "not empty",
    );
}

#[test]
fn an_existing_anchor_is_never_overwritten() {
    let s = Scratch::new("again");
    std::fs::write(s.trust("release-trust.pub"), "ML-DSA-87 old 00\n").unwrap();
    let signer = format!("alice@example.invalid={}", s.path("alice.pub").display());
    let custody = s.path("keys");
    refused(
        &s,
        &["--custody", custody.to_str().unwrap(), "--signer", &signer],
        "already exists",
    );
    assert_eq!(
        std::fs::read_to_string(s.trust("release-trust.pub")).unwrap(),
        "ML-DSA-87 old 00\n"
    );
}

#[test]
fn bad_signers_are_refused() {
    let s = Scratch::new("signers");
    let custody = s.path("keys");
    let c = custody.to_str().unwrap();
    std::fs::write(s.path("junk.pub"), "not a key\n").unwrap();
    let junk = format!("alice@example.invalid={}", s.path("junk.pub").display());
    refused(
        &s,
        &["--custody", c, "--signer", &junk],
        "not an SSH public key",
    );
    let bare = s.path("alice.pub").display().to_string();
    refused(
        &s,
        &["--custody", c, "--signer", &bare],
        "is not PRINCIPAL=KEY.pub",
    );
    refused(&s, &["--custody", c], "at least one tag signer");
    let a = format!("alice@example.invalid={}", s.path("alice.pub").display());
    refused(
        &s,
        &["--custody", c, "--signer", &a, "--signer", &a],
        "listed once",
    );
}

#[test]
fn check_refuses_a_checkout_with_no_ceremony() {
    let s = Scratch::new("nocheck");
    let c = Command::new(EXE)
        .args(["check", "--repo"])
        .arg(s.path("repo"))
        .output()
        .unwrap();
    assert!(!c.status.success());
    assert!(text(&c).contains("release-trust.pub"), "{}", text(&c));
}
