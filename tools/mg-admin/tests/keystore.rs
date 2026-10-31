//! WP-50 — `mg-admin keystore migrate|verify`, driving the real binary. A version 1
//! keystore (custody root in the clear), as an earlier release wrote, is refused by
//! every reader with the migrate hint; `keystore migrate` converts it in place to
//! version 2 under the passphrase, keeping the key, and `keystore verify` then unlocks
//! it. An enclosure upgraded from an earlier build is migrated the same way before
//! `mesh upgrade` (the compose `init` does this). With no passphrase source, mg-admin
//! refuses and names both. AC-50.2, AC-50.3, AC-50.4. `[VLD-ANB-MIDGARD-0007]`

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use attest::keystore::test_support::{encode_v1, root_secret_bytes, test_passphrase};
use attest::keystore::{format_version, VERSION};
use attest::{Keystore, RootWrap};
use http_gateway::state_dir::StateDir;
use kms::SigningRoot;
use pqc_agility::OsRng;

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!(
        "mg-admin-keystore-{tag}-{}-{}",
        std::process::id(),
        uniq
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run `mg-admin` with the test passphrase file in `MIDGARD_KEYSTORE_PASSPHRASE_FILE`.
fn mg_admin(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mg-admin"))
        .env(
            "MIDGARD_KEYSTORE_PASSPHRASE_FILE",
            attest::keystore::test_support::shared_test_passphrase_file(),
        )
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("mg-admin runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Rewrite a version 2 keystore on disk as the version 1 file an earlier release
/// would have written for the same keys.
fn downgrade_to_v1(path: &Path) -> Vec<u8> {
    let keystore = Keystore::read(path, &RootWrap::Passphrase(&test_passphrase())).unwrap();
    let v1 = encode_v1(&keystore);
    std::fs::write(path, &v1).unwrap();
    root_secret_bytes(&keystore)
}

/// AC-50.3: a version 1 file migrates in place, keeps its key and loses its root
/// bytes; verify then unlocks it, and migrating again changes nothing.
// [AC-50.3] [VLD-ANB-MIDGARD-0007]
#[test]
fn migrate_then_verify_round_trip() {
    let dir = scratch("migrate");
    let path = dir.join("release.keystore");
    let original = Keystore::generate("release-key-1", kms::RELEASE_SIG_ALG, &mut OsRng).unwrap();
    let root = root_secret_bytes(&original);
    std::fs::write(&path, encode_v1(&original)).unwrap();
    let path_arg = path.display().to_string();

    // Before: every reader refuses it, with the way out.
    let refused = mg_admin(&["keystore", "verify", "--keystore", &path_arg]);
    assert_eq!(refused.status.code(), Some(1), "{}", stderr(&refused));
    assert!(
        stderr(&refused).contains("keystore migrate"),
        "{}",
        stderr(&refused)
    );

    let migrated = mg_admin(&["keystore", "migrate", "--keystore", &path_arg]);
    assert!(migrated.status.success(), "{}", stderr(&migrated));
    assert!(stdout(&migrated).contains("destroy every other copy"));
    assert!(!dir.join("release.keystore.v2").exists());

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(format_version(&bytes), Some(VERSION));
    assert!(
        !contains(&bytes, &root),
        "the migrated file holds no root bytes"
    );
    let reread = Keystore::read(&path, &RootWrap::Passphrase(&test_passphrase())).unwrap();
    assert_eq!(
        reread.into_signing_root().unwrap().verifying_key(),
        &original.verifying_key,
        "the same signing key"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    let verified = mg_admin(&["keystore", "verify", "--keystore", &path_arg]);
    assert!(verified.status.success(), "{}", stderr(&verified));
    let out = stdout(&verified);
    assert!(out.contains("release-key-1"), "{out}");
    assert!(out.contains("ML-DSA-87"), "{out}");
    assert!(out.contains("format version: 2"), "{out}");
    assert!(out.contains("unlocks: yes"), "{out}");

    let again = mg_admin(&["keystore", "migrate", "--keystore", &path_arg]);
    assert!(again.status.success(), "{}", stderr(&again));
    assert!(stdout(&again).contains("nothing changed"));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);

    let _ = std::fs::remove_dir_all(&dir);
}

/// AC-50.2: no passphrase source is a usage error naming both; a wrong passphrase is
/// "locked", never "corrupt".
// [AC-50.2] [VLD-ANB-MIDGARD-0007]
#[test]
fn services_refuse_to_start_without_a_passphrase_source() {
    let dir = scratch("no-source");
    let state = dir.join("state").display().to_string();
    let out = Command::new(env!("CARGO_BIN_EXE_mg-admin"))
        .env_remove("MIDGARD_KEYSTORE_PASSPHRASE_FILE")
        .args(["init", "--state-dir", &state])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("--keystore-passphrase-file"));
    assert!(stderr(&out).contains("MIDGARD_KEYSTORE_PASSPHRASE_FILE"));
    assert!(
        !dir.join("state/keys").exists(),
        "nothing is written without a passphrase"
    );

    assert!(mg_admin(&["init", "--state-dir", &state]).status.success());
    let wrong = dir.join("wrong.passphrase");
    attest::keystore::write_restricted(&wrong, b"wrong-passphrase-0000").unwrap();
    let locked = mg_admin(&[
        "keystore",
        "verify",
        "--keystore",
        &StateDir::new(&state).idp_keystore().display().to_string(),
        "--keystore-passphrase-file",
        &wrong.display().to_string(),
    ]);
    assert_eq!(locked.status.code(), Some(1));
    assert!(stderr(&locked).contains("locked"), "{}", stderr(&locked));
    assert!(!stderr(&locked).contains("corrupt"), "{}", stderr(&locked));

    let _ = std::fs::remove_dir_all(&dir);
}

/// AC-50.4: `init` creates keys/ 0700 and the keystores 0600; `mesh issue` creates
/// --out 0700.
// [AC-50.4] [VLD-ANB-MIDGARD-0007]
#[cfg(unix)]
#[test]
fn init_and_mesh_issue_create_private_key_directories() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir = scratch("modes");
    let state = StateDir::new(dir.join("state"));
    let state_arg = state.root().display().to_string();
    assert!(mg_admin(&["init", "--state-dir", &state_arg])
        .status
        .success());
    assert_eq!(mode(&state.keys()), 0o700);
    assert_eq!(mode(&state.idp_keystore()), 0o600);
    assert_eq!(mode(&state.audit_keystore()), 0o600);

    let out = dir.join("svc");
    let issued = mg_admin(&[
        "mesh",
        "issue",
        "--state-dir",
        &state_arg,
        "--service",
        "svc",
        "--roles",
        "inference-server",
        "--out",
        &out.display().to_string(),
    ]);
    assert!(issued.status.success(), "{}", stderr(&issued));
    assert_eq!(mode(&out), 0o700);
    assert_eq!(mode(&out.join("svc.keystore")), 0o600);

    let _ = std::fs::remove_dir_all(&dir);
}

/// The in-place upgrade path (6a9b34e) meets version 1 keystores: an enclosure and a
/// service directory an earlier build wrote are refused until migrated, and once every
/// keystore is migrated, `mesh upgrade` and the state directory work as before, with
/// the same keys. `[VLD-ANB-MIDGARD-0007]` [MG-SEC-02]
#[test]
fn an_enclosure_from_an_earlier_build_is_migrated_then_upgraded() {
    let dir = scratch("upgrade");
    let state = StateDir::new(dir.join("state"));
    let state_arg = state.root().display().to_string();
    let out = dir.join("svc");
    let out_arg = out.display().to_string();
    assert!(mg_admin(&["init", "--state-dir", &state_arg])
        .status
        .success());
    let issue = mg_admin(&[
        "mesh",
        "issue",
        "--state-dir",
        &state_arg,
        "--service",
        "svc",
        "--roles",
        "inference-server",
        "--out",
        &out_arg,
    ]);
    assert!(issue.status.success(), "{}", stderr(&issue));

    // What an earlier build left: every keystore in version 1, root in the clear.
    let keystores = [
        state.idp_keystore(),
        state.audit_keystore(),
        out.join("svc.keystore"),
        out.join("svc.audit.keystore"),
    ];
    let roots: Vec<Vec<u8>> = keystores.iter().map(|k| downgrade_to_v1(k)).collect();
    let identity_vk = Keystore::decode(
        &attest::keystore::migrate_v1(
            &std::fs::read(&keystores[2]).unwrap(),
            &RootWrap::Passphrase(&test_passphrase()),
            &mut OsRng,
        )
        .unwrap(),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .unwrap()
    .verifying_key;

    let upgrade = || {
        mg_admin(&[
            "mesh",
            "upgrade",
            "--state-dir",
            &state_arg,
            "--service",
            "svc",
            "--roles",
            "inference-server",
            "--out",
            &out_arg,
        ])
    };
    let refused = upgrade();
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        stderr(&refused).contains("keystore migrate"),
        "{}",
        stderr(&refused)
    );

    // The audit keystore first, on its own: recording a migration in the ledger needs
    // it. Then the rest, each recorded in the enclosure's audit ledger.
    let migrate = |keystore: &Path, record: bool| {
        let keystore = keystore.display().to_string();
        let mut args = vec!["keystore", "migrate", "--keystore", &keystore];
        if record {
            args.extend(["--state-dir", &state_arg]);
        }
        let migrated = mg_admin(&args);
        assert!(migrated.status.success(), "{}", stderr(&migrated));
    };
    migrate(&keystores[1], false);
    for keystore in [&keystores[0], &keystores[2], &keystores[3]] {
        migrate(keystore, true);
    }
    for (keystore, root) in keystores.iter().zip(&roots) {
        assert!(!contains(&std::fs::read(keystore).unwrap(), root));
    }

    let upgraded = upgrade();
    assert!(upgraded.status.success(), "{}", stderr(&upgraded));
    let identity =
        Keystore::read(&keystores[2], &RootWrap::Passphrase(&test_passphrase())).unwrap();
    assert_eq!(
        identity.verifying_key, identity_vk,
        "the key it already had"
    );
    let listed = mg_admin(&["user", "list", "--state-dir", &state_arg]);
    assert!(listed.status.success(), "{}", stderr(&listed));

    let _ = std::fs::remove_dir_all(&dir);
}
