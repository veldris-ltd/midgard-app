//! `mg-bundle` end to end: a bundle packed from a sealed keystore imports offline against
//! the anchor that keystore exports, and writes out exactly what it verified; another
//! key's anchor, a changed byte, a key of the wrong algorithm and an unknown component
//! are all refused, and a refused import writes nothing. [FR-B-5] [AC-B-3] [RC-19]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use attest::Keystore;
use pqc_agility::{OsRng, SigId};

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    let dir = std::env::temp_dir().join(format!("mg-bundle-{tag}-{}-{uniq}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn mg_bundle(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mg-bundle"))
        .args(args)
        .output()
        .unwrap()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A sealed keystore of `alg` and the anchor file it exports.
fn keystore(dir: &Path, name: &str, alg: SigId) -> (PathBuf, PathBuf) {
    let key = Keystore::generate(name, alg, &mut OsRng).unwrap();
    let keystore = dir.join(format!("{name}.keystore"));
    let anchor = dir.join(format!("{name}.pub"));
    key.write(&keystore).unwrap();
    std::fs::write(&anchor, key.export_verifying_key()).unwrap();
    (keystore, anchor)
}

#[test]
fn a_bundle_from_a_sealed_keystore_imports_offline_and_refuses_everything_else() {
    let dir = scratch("flow");
    let (admission, anchor) = keystore(&dir, "bifrost-admission", SigId::MlDsa65);

    let service = dir.join("http-gateway");
    let payload = b"\x7fELF not really a service".to_vec();
    let sbom = r#"{"bomFormat": "CycloneDX", "specVersion": "1.5", "components": [
        {"type": "library", "name": "http-gateway", "version": "0.1.0-rc.1", "purl": "pkg:cargo/http-gateway@0.1.0-rc.1"}
    ]}"#;
    std::fs::write(&service, &payload).unwrap();
    std::fs::write(dir.join("http-gateway.sbom.cdx.json"), sbom).unwrap();
    let signature = dir.join("http-gateway.sig");
    std::fs::write(&signature, b"a detached release signature").unwrap();

    let bundle = dir.join("midgard.mgbundle");
    let spec_service = format!("http-gateway:0.1.0-rc.1:{}", s(&service));
    let spec_signature = format!("attestation=http-gateway.sig:0.1.0-rc.1:{}", s(&signature));
    let pack = |keystore: &Path, specs: &[&str]| {
        let mut args = vec![
            "pack",
            "--out",
            s(&bundle),
            "--id",
            "midgard-0.1.0-rc.1",
            "--label",
            "v0.1.0-rc.1",
            "--keystore",
            s(keystore),
        ];
        args.extend_from_slice(specs);
        mg_bundle(&args)
    };
    let out = pack(&admission, &[&spec_service, &spec_signature]);
    assert!(out.status.success(), "{}", stderr(&out));

    let verified = dir.join("verified");
    let out = mg_bundle(&[
        "import",
        "--trust-anchor",
        s(&anchor),
        "--out-dir",
        s(&verified),
        s(&bundle),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let refs = String::from_utf8_lossy(&out.stdout);
    assert!(refs.contains("http-gateway@0.1.0-rc.1"), "{refs}");
    assert!(refs.contains("http-gateway.sig@0.1.0-rc.1"), "{refs}");
    assert_eq!(
        std::fs::read(verified.join("http-gateway")).unwrap(),
        payload
    );
    assert_eq!(
        std::fs::read_to_string(verified.join("http-gateway.sbom.cdx.json")).unwrap(),
        sbom
    );
    assert_eq!(
        std::fs::read(verified.join("http-gateway.sig")).unwrap(),
        b"a detached release signature"
    );

    // Importing into the same directory again never overwrites.
    let again = mg_bundle(&[
        "import",
        "--trust-anchor",
        s(&anchor),
        "--out-dir",
        s(&verified),
        s(&bundle),
    ]);
    assert_eq!(again.status.code(), Some(1));

    // Another key's anchor: refused, and nothing written.
    let (_, other) = keystore(&dir, "someone-else", SigId::MlDsa65);
    let refused = dir.join("refused");
    let out = mg_bundle(&[
        "import",
        "--trust-anchor",
        s(&other),
        "--out-dir",
        s(&refused),
        s(&bundle),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("REJECTED"), "{}", stderr(&out));
    assert!(!refused.exists(), "a refused import wrote files");

    // One changed byte anywhere in the bundle.
    let mut bytes = std::fs::read(&bundle).unwrap();
    let at = bytes.len() / 2;
    bytes[at] ^= 0x01;
    let tampered = dir.join("tampered.mgbundle");
    std::fs::write(&tampered, bytes).unwrap();
    let out = mg_bundle(&["import", "--trust-anchor", s(&anchor), s(&tampered)]);
    assert_eq!(out.status.code(), Some(1), "a tampered bundle imported");

    // A release key (ML-DSA-87) cannot sign a bundle import would accept.
    let (release, _) = keystore(&dir, "release", SigId::MlDsa87);
    let out = pack(&release, &[&spec_service]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("ML-DSA-65"), "{}", stderr(&out));

    // An unknown component.
    let out = pack(
        &admission,
        &[&format!("binary=http-gateway:1:{}", s(&service))],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("unknown component"),
        "{}",
        stderr(&out)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pack_refuses_an_sbom_that_import_would_refuse() {
    let dir = scratch("bad-sbom");
    let (admission, _anchor) = keystore(&dir, "bifrost-admission", SigId::MlDsa65);

    let service = dir.join("svc");
    std::fs::write(&service, b"bytes").unwrap();
    // CycloneDX in name only: it lists no components.
    std::fs::write(
        dir.join("svc.sbom.cdx.json"),
        r#"{"bomFormat": "CycloneDX", "specVersion": "1.5"}"#,
    )
    .unwrap();

    let bundle = dir.join("out.mgbundle");
    let spec = format!("svc:1.0:{}", s(&service));
    let out = mg_bundle(&[
        "pack",
        "--out",
        s(&bundle),
        "--id",
        "x",
        "--label",
        "x",
        "--keystore",
        s(&admission),
        &spec,
    ]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("SBOM"), "{}", stderr(&out));
    assert!(!bundle.exists(), "a refused pack wrote a bundle");
}
