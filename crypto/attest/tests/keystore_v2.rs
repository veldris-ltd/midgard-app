//! WP-50: keystore format version 3: the ML-KEM-1024 custody root is wrapped under
//! an operator passphrase or an external trust root, its AAD binding the whole header;
//! version 1 (root in the clear) and version 2 (header not authenticated) are refused
//! by every reader and migrated once; and key files and directories are private from
//! creation. Acceptance criteria AC-50.1 … AC-50.5.
//! `[VLD-ANB-MIDGARD-0007]`
//!
//! Everything here runs offline with no network. `[NFR-PRV-1]`

#![cfg(feature = "custody")]

use std::path::PathBuf;

use attest::keystore::test_support::{encode_v1, encode_v2, root_secret_bytes, test_passphrase};
use attest::keystore::{create_private_dir, format_version, migrate, VERSION};
use attest::{Keystore, RootWrap, SignError};
use kms::{HardwareTrustRoot, SigningRoot, SoftHsmToken};
use pqc_agility::OsRng;

/// A scratch directory that cleans itself up. No external crates, so no tempfile.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("attest-keystore-v2-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn keystore(key_id: &str) -> Keystore {
    Keystore::generate(key_id, kms::RELEASE_SIG_ALG, &mut OsRng).expect("generate keystore")
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// AC-50.1: the root secret is nowhere in the file, and the version byte is 3.
// [AC-50.1] [VLD-ANB-MIDGARD-0007]
#[test]
fn root_secret_is_not_present_in_the_clear() {
    let s = Scratch::new("root-clear");
    let k = keystore("release-key-1");
    let path = s.path("k.keystore");
    let passphrase = test_passphrase();
    k.write(&path, &RootWrap::Passphrase(&passphrase), &mut OsRng)
        .expect("write");

    let file = std::fs::read(&path).unwrap();
    assert_eq!(format_version(&file), Some(3));
    assert_eq!(VERSION, 3);
    assert!(
        !contains(&file, &root_secret_bytes(&k)),
        "no window of the keystore equals the custody root"
    );
}

/// AC-50.2: the wrong passphrase is `KeystoreLocked` and yields no root; the right
/// one unseals a custodian that signs.
// [AC-50.2] [VLD-ANB-MIDGARD-0007]
#[test]
fn a_keystore_opened_without_the_passphrase_fails() {
    let s = Scratch::new("locked");
    let k = keystore("release-key-1");
    let path = s.path("k.keystore");
    let passphrase = test_passphrase();
    k.write(&path, &RootWrap::Passphrase(&passphrase), &mut OsRng)
        .expect("write");

    let wrong = pqc_agility::SecretBytes::new(b"wrong-passphrase-0000".to_vec());
    match Keystore::read(&path, &RootWrap::Passphrase(&wrong)) {
        Err(SignError::KeystoreLocked(reason)) => {
            assert!(!reason.contains("corrupt"), "{reason}")
        }
        Err(e) => panic!("expected KeystoreLocked, got {e}"),
        Ok(_) => panic!("a wrong passphrase produced a signing root"),
    }

    // A passphrase-wrapped file offered an external root is locked as well.
    let token = SoftHsmToken::provision("hsm-0", "root-0", &mut OsRng).unwrap();
    let hsm = HardwareTrustRoot::new(Box::new(token));
    assert!(matches!(
        Keystore::read(&path, &RootWrap::External(&hsm)),
        Err(SignError::KeystoreLocked(_))
    ));

    let opened = Keystore::read(&path, &RootWrap::Passphrase(&passphrase)).expect("unlocks");
    let root = opened.into_signing_root().expect("unseals");
    let sig = root.sign(b"payload").expect("signs");
    assert!(!sig.as_bytes().is_empty());
    assert_eq!(root.verifying_key(), &k.verifying_key);
}

/// AC-50.3: a version 1 file is refused with the migrate hint; migrating it yields a
/// version 3 file that unseals the same verifying key and holds no root bytes.
// [AC-50.3] [VLD-ANB-MIDGARD-0007]
#[test]
fn version_1_is_refused_and_migrates() {
    let s = Scratch::new("migrate");
    let k = keystore("release-key-1");
    let root = root_secret_bytes(&k);
    let v1 = encode_v1(&k);
    assert_eq!(format_version(&v1), Some(1));
    assert!(
        contains(&v1, &root),
        "version 1 holds the root in the clear"
    );

    let passphrase = test_passphrase();
    let wrap = RootWrap::Passphrase(&passphrase);
    let path = s.path("v1.keystore");
    std::fs::write(&path, &v1).unwrap();
    match Keystore::read(&path, &wrap) {
        Err(SignError::KeyUnreadable { reason, .. }) => {
            assert!(reason.contains("keystore migrate"), "{reason}")
        }
        Err(e) => panic!("expected the migrate refusal, got {e}"),
        Ok(_) => panic!("a version 1 keystore was read"),
    }

    let v3 = migrate(&v1, &wrap, &mut OsRng).expect("migrate");
    assert_eq!(format_version(&v3), Some(VERSION));
    assert!(
        !contains(&v3, &root),
        "the migrated file holds no root bytes"
    );
    let migrated = Keystore::decode(&v3, &wrap).expect("the migrated file decodes");
    assert_eq!(migrated.key_id, "release-key-1");
    let signing = migrated.into_signing_root().expect("unseals");
    assert_eq!(signing.verifying_key(), &k.verifying_key);
    assert_eq!(
        root_secret_bytes(&Keystore::decode(&v3, &wrap).unwrap()),
        root
    );

    // Migration takes only version 1 or 2 input, never a current file.
    assert!(migrate(&v3, &wrap, &mut OsRng).is_err());
}

/// A version 2 file (root wrapped, header not authenticated), as the previous release
/// wrote, is refused with the migrate hint and still migrates: to version 3, the same
/// root and the same signing key, its header now bound to the wrapped root.
// [AC-50.3] [VLD-ANB-MIDGARD-0007]
#[test]
fn version_2_is_refused_and_still_migrates() {
    let s = Scratch::new("migrate-v2");
    let k = keystore("release-key-1");
    let v2 = encode_v2(&k);
    assert_eq!(format_version(&v2), Some(2));

    let passphrase = test_passphrase();
    let wrap = RootWrap::Passphrase(&passphrase);
    let path = s.path("v2.keystore");
    std::fs::write(&path, &v2).unwrap();
    match Keystore::read(&path, &wrap) {
        Err(SignError::KeyUnreadable { reason, .. }) => {
            assert!(reason.contains("keystore migrate"), "{reason}")
        }
        Err(e) => panic!("expected the migrate refusal, got {e}"),
        Ok(_) => panic!("a version 2 keystore was read"),
    }

    let v3 = migrate(&v2, &wrap, &mut OsRng).expect("migrate");
    assert_eq!(format_version(&v3), Some(VERSION));
    let migrated = Keystore::decode(&v3, &wrap).expect("the migrated file decodes");
    assert_eq!(root_secret_bytes(&migrated), root_secret_bytes(&k));
    assert_eq!(
        migrated
            .into_signing_root()
            .expect("unseals")
            .verifying_key(),
        &k.verifying_key
    );

    // A wrong passphrase migrates nothing.
    let wrong = pqc_agility::SecretBytes::new(b"wrong-passphrase-0000".to_vec());
    assert!(matches!(
        migrate(&v2, &RootWrap::Passphrase(&wrong), &mut OsRng),
        Err(SignError::KeystoreLocked(_))
    ));
}

/// AC-50.5: the root sealed to a hardware trust root (SoftHSM stand-in) reads back
/// with the same token; another token's root does not unlock it.
// [AC-50.5] [VLD-ANB-MIDGARD-0007]
#[test]
fn external_root_wrap_round_trips() {
    let s = Scratch::new("external");
    let k = keystore("release-key-1");
    let path = s.path("k.keystore");
    let token = SoftHsmToken::provision("hsm-1", "custody-root", &mut OsRng).unwrap();
    let hsm = HardwareTrustRoot::new(Box::new(token));
    k.write(&path, &RootWrap::External(&hsm), &mut OsRng)
        .expect("write");
    let file = std::fs::read(&path).unwrap();
    assert!(!contains(&file, &root_secret_bytes(&k)));

    let opened = Keystore::read(&path, &RootWrap::External(&hsm)).expect("same token unlocks");
    assert_eq!(
        opened.into_signing_root().unwrap().verifying_key(),
        &k.verifying_key
    );

    // Another device, even one claiming the same key id, cannot unwrap the root.
    for (device, key_id) in [("hsm-2", "custody-root"), ("hsm-3", "other-root")] {
        let other = SoftHsmToken::provision(device, key_id, &mut OsRng).unwrap();
        let other = HardwareTrustRoot::new(Box::new(other));
        assert!(
            matches!(
                Keystore::read(&path, &RootWrap::External(&other)),
                Err(SignError::KeystoreLocked(_))
            ),
            "{device}"
        );
    }
    // Nor can a passphrase.
    let passphrase = test_passphrase();
    assert!(matches!(
        Keystore::read(&path, &RootWrap::Passphrase(&passphrase)),
        Err(SignError::KeystoreLocked(_))
    ));
}

/// AC-50.4: key files are created 0600, never overwritten, and key directories 0700.
// [AC-50.4] [VLD-ANB-MIDGARD-0007]
#[cfg(unix)]
#[test]
fn write_restricted_creates_0600_and_refuses_overwrite() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("hygiene");
    let keys = s.path("state/keys");
    create_private_dir(&keys).expect("create keys/");
    assert_eq!(
        std::fs::metadata(&keys).unwrap().permissions().mode() & 0o777,
        0o700
    );
    // An existing, wider directory is narrowed.
    std::fs::set_permissions(&keys, std::fs::Permissions::from_mode(0o755)).unwrap();
    create_private_dir(&keys).unwrap();
    assert_eq!(
        std::fs::metadata(&keys).unwrap().permissions().mode() & 0o777,
        0o700
    );

    let path = keys.join("k.keystore");
    let passphrase = test_passphrase();
    let wrap = RootWrap::Passphrase(&passphrase);
    keystore("a")
        .write(&path, &wrap, &mut OsRng)
        .expect("write");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(
        !keys.join("k.keystore.tmp").exists(),
        "the temporary name is renamed away"
    );
    let first = std::fs::read(&path).unwrap();
    assert!(
        keystore("b").write(&path, &wrap, &mut OsRng).is_err(),
        "a second write to the same path is an error"
    );
    assert_eq!(std::fs::read(&path).unwrap(), first, "and changes nothing");
}

/// The passphrase file rules: empty, shorter than 16 bytes and (on Unix) readable by
/// others are all refused; one trailing newline is not part of the passphrase.
// [AC-50.2] [VLD-ANB-MIDGARD-0007]
#[test]
fn passphrase_file_rules() {
    let s = Scratch::new("passphrase");
    let write = |name: &str, bytes: &[u8]| {
        let p = s.path(name);
        attest::keystore::write_restricted(&p, bytes).unwrap();
        p
    };

    assert!(RootWrap::passphrase_from_file(&write("empty", b"")).is_err());
    assert!(RootWrap::passphrase_from_file(&write("newline-only", b"\n")).is_err());
    assert!(RootWrap::passphrase_from_file(&write("short", b"fifteen-bytes!!")).is_err());
    assert!(RootWrap::passphrase_from_file(&s.path("absent")).is_err());

    let good = write("good", b"sixteen-bytes-ok\n");
    let passphrase = RootWrap::passphrase_from_file(&good).expect("accepted");
    assert_eq!(passphrase.expose(), b"sixteen-bytes-ok");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let wide = write("wide", b"a-long-enough-passphrase");
        std::fs::set_permissions(&wide, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(RootWrap::passphrase_from_file(&wide).is_err());
    }
}
