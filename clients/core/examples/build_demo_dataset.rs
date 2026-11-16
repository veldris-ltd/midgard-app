//! Build and sign the demo dataset for a release. [AC-C-13] [NFR-PQ-3] [NFR-PRV-1]
//!
//! ```text
//! cargo run --offline --locked -p midgard-client-core --features backend-fips \
//!   --example build_demo_dataset -- --fixtures <dir> --tag <tag> --keystore <path> \
//!   --keystore-passphrase-file <path> --anchors <pub> --out <file>
//! ```
//!
//! Refuses unless every manifest input is a plain path under `sdk/ts/src/mock/` (and
//! byte-identical to the tagged release's copy) or under `clients/demo/` (and the
//! committed, clean copy), the fixture directory itself is committed and clean, the
//! body's numbers have one spelling and the local-suite guard passes over the whole
//! body, and the signed file verifies under the pinned anchor before it is written.
//! Every check is a refusal; there is no warn-and-continue.
//!
//! The unsealed demo key is wrapped in [`dataset::DemoDatasetOnly`] at once, so in
//! this process it can sign `preimage(DEMO_DATASET, …)` messages only. It is never
//! handed to a general signer.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use attest::json::{parse, Json};
use attest::{Keystore, KmsSigner, RootWrap, TrustAnchors};
use midgard_client_core::demo::build::{self, InputOrigin};
use midgard_client_core::demo::dataset;

/// ML-DSA signing in an unoptimised build holds large arrays on the stack; run the
/// work on a thread with room, as `mg-sign` does.
const STACK_SIZE: usize = 64 * 1024 * 1024;

fn arg(args: &[String], name: &str) -> Result<String, String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or(format!("{name} is required"))
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Every manifest input comes from the tag's mocks or the committed demo sources, and
/// the fixture directory is the committed one.
fn check_inputs(root: &Path, fixtures: &Path, tag: &str) -> Result<(), String> {
    let manifest =
        std::fs::read_to_string(fixtures.join("manifest.json")).map_err(|e| e.to_string())?;
    let v = parse(&manifest)?;
    let Some(Json::Obj(inputs)) = v.get("inputs") else {
        return Err("manifest has no inputs".into());
    };
    if inputs.is_empty() {
        return Err("manifest has no inputs".into());
    }
    for (path, blob) in inputs {
        let blob = blob
            .as_str()
            .ok_or(format!("input {path}: blob is not a string"))?;
        let origin = build::input_origin(path).map_err(|e| e.to_string())?;
        let want = match origin {
            InputOrigin::Tagged => git(root, &["rev-parse", &format!("{tag}:{path}")])?,
            InputOrigin::Committed => {
                if !git(root, &["status", "--porcelain", "--", path])?.is_empty() {
                    return Err(format!("input {path} has uncommitted changes"));
                }
                git(root, &["rev-parse", &format!("HEAD:{path}")])?
            }
        };
        if want != blob {
            return Err(format!(
                "input {path} is not the {} copy",
                match origin {
                    InputOrigin::Tagged => tag,
                    InputOrigin::Committed => "committed",
                }
            ));
        }
    }
    let rel = fixtures
        .canonicalize()
        .map_err(|e| format!("{}: {e}", fixtures.display()))?;
    let rel = rel
        .strip_prefix(root)
        .map_err(|_| format!("{} is outside the repository", fixtures.display()))?
        .to_string_lossy()
        .into_owned();
    if git(root, &["ls-files", "--", &rel])?.is_empty() {
        return Err(format!("the fixtures in {rel} are not committed"));
    }
    if !git(
        root,
        &["status", "--porcelain", "--untracked-files=all", "--", &rel],
    )?
    .is_empty()
    {
        return Err(format!("the fixtures in {rel} have uncommitted changes"));
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = PathBuf::from(git(Path::new("."), &["rev-parse", "--show-toplevel"])?)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let fixtures = PathBuf::from(arg(&args, "--fixtures")?);
    let tag = arg(&args, "--tag")?;
    let out = PathBuf::from(arg(&args, "--out")?);
    let release = tag
        .strip_prefix('v')
        .ok_or("--tag must look like v<release>")?
        .to_string();
    let anchors = TrustAnchors::parse(
        &std::fs::read_to_string(arg(&args, "--anchors")?).map_err(|e| e.to_string())?,
    )?;

    check_inputs(&root, &fixtures, &tag)?;
    let body = build::canonical_body(&fixtures).map_err(|e| e.to_string())?;
    let built = parse(std::str::from_utf8(&body).map_err(|e| e.to_string())?)?;
    if built.get("release").and_then(Json::as_str) != Some(release.as_str()) {
        return Err(format!("the fixtures are not for {release}"));
    }

    // Already built from these fixtures and signed under the pinned key: keep it, so
    // a rebuild does not churn the committed file (ML-DSA signing is randomised).
    if let Ok(existing) = std::fs::read(&out) {
        if let Ok(ds) = dataset::verify(&existing, &anchors, &release) {
            if ds.body_bytes == body {
                println!(
                    "build_demo_dataset: {} is up to date ({} bytes)",
                    out.display(),
                    existing.len()
                );
                return Ok(());
            }
        }
    }

    let pass =
        RootWrap::passphrase_from_file(Path::new(&arg(&args, "--keystore-passphrase-file")?))
            .map_err(|e| e.to_string())?;
    let ks = Keystore::read(
        Path::new(&arg(&args, "--keystore")?),
        &RootWrap::Passphrase(&pass),
    )
    .map_err(|e| e.to_string())?;
    drop(pass);
    let signer = dataset::DemoDatasetOnly::new(KmsSigner::new(
        ks.into_signing_root().map_err(|e| e.to_string())?,
    ));
    let file = dataset::sign(&release, &body, &signer).map_err(|e| e.to_string())?;
    drop(signer);

    dataset::verify(&file, &anchors, &release).map_err(|e| format!("self-check failed: {e}"))?;
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&out, &file).map_err(|e| e.to_string())?;
    println!(
        "build_demo_dataset: {} ({} bytes) signed and verified",
        out.display(),
        file.len()
    );
    Ok(())
}

fn main() -> ExitCode {
    let worker = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(run)
        .map_err(|e| e.to_string());
    match worker.and_then(|h| {
        h.join()
            .map_err(|_| "the build thread panicked".to_string())
    }) {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(e)) | Err(e) => {
            eprintln!("build_demo_dataset: {e}");
            ExitCode::FAILURE
        }
    }
}
