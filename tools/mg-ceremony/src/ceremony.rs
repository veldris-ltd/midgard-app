//! The ceremony itself: the checks before anything is generated, the keys, the
//! published files, the checks the release will make of them, and the record.
//!
//! Every refusal happens before a key exists. Once the keys are generated, they
//! are written to the custody directory first and read back, so a published
//! anchor always pins a key that is held.

use std::path::{Component, Path, PathBuf};

use attest::{hex::hex, Keystore};
use pqc_agility::{sha256, SigId};

use crate::clock::Now;
use crate::ssh::Signer;

/// Where the published files go, relative to the checkout.
pub const TRUST: &str = "deploy/trust";
/// Where the ceremony record goes, relative to the checkout.
pub const RECORDS: &str = "docs/ops/ceremonies";
pub const RELEASE_ANCHOR: &str = "release-trust.pub";
pub const BIFROST_ANCHOR: &str = "bifrost-trust.pub";
pub const ALLOWED_SIGNERS: &str = "allowed_signers";

/// The Bifrost admission algorithm, `bifrost_artefact::BIFROST_SIG_ALG`. Named here
/// rather than imported so the ceremony does not pull in the registry; the release
/// preflight and `mg-register` both refuse an admission key of any other algorithm.
const BIFROST_ALG: SigId = SigId::MlDsa65;

/// What the officers decided, before anything is generated.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The Midgard checkout the files are published into.
    pub repo: PathBuf,
    /// A fresh directory outside the checkout, for the keystores.
    pub custody: PathBuf,
    /// The year in the key ids: `midgard-release-<year>`.
    pub year: String,
    pub officer: String,
    pub witness: String,
    pub signers: Vec<Signer>,
}

/// What a ceremony produced.
#[derive(Debug)]
pub struct Outcome {
    pub release_keystore: PathBuf,
    pub bifrost_keystore: PathBuf,
    pub release_id: String,
    pub bifrost_id: String,
    pub published: Published,
    /// The record, relative to the checkout.
    pub record: PathBuf,
}

/// The three published files, as the release reads them.
#[derive(Debug)]
pub struct Published {
    pub anchors: Vec<Anchor>,
    pub signers: Vec<Signer>,
    pub allowed_signers_sha256: String,
}

#[derive(Debug)]
pub struct Anchor {
    /// Relative to the checkout, with forward slashes.
    pub file: String,
    pub alg: &'static str,
    pub key_ids: Vec<String>,
    pub sha256: String,
}

/// A path as a person reads it. Canonical Windows paths carry the `\\?\` prefix,
/// which is correct but alarming on a ceremony screen, so it is left out.
pub fn shown(path: impl AsRef<Path>) -> String {
    let s = path.as_ref().display().to_string();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s
    }
}

/// The checkout to publish into: it must hold `deploy/trust`.
pub fn find_repo(path: &Path) -> Result<PathBuf, String> {
    let repo = std::fs::canonicalize(path)
        .map_err(|e| format!("cannot open the checkout {}: {e}", shown(path)))?;
    if !repo.join(TRUST).join("README.md").is_file() {
        return Err(format!(
            "{} is not a Midgard checkout: it has no {TRUST}/README.md",
            shown(&repo)
        ));
    }
    Ok(repo)
}

/// Look for a Midgard checkout at, or above, each starting point.
pub fn discover_repo(starts: &[PathBuf]) -> Option<PathBuf> {
    starts
        .iter()
        .flat_map(|s| s.ancestors())
        .find_map(|dir| find_repo(dir).ok())
}

/// A ceremony publishes new keys once. If any published file exists, this is a
/// rotation, which appends beside the old key (`deploy/trust/README.md`).
pub fn check_not_held(repo: &Path) -> Result<(), String> {
    for f in [RELEASE_ANCHOR, BIFROST_ANCHOR, ALLOWED_SIGNERS] {
        let p = repo.join(TRUST).join(f);
        if p.exists() {
            return Err(format!(
                "{TRUST}/{f} already exists. A ceremony publishes new keys once; \
                 to rotate, append (deploy/trust/README.md)"
            ));
        }
    }
    Ok(())
}

/// Check a custody directory without creating it: it must be outside the
/// checkout, where git cannot pick the keystores up, and empty or not yet there.
/// Returns its absolute path.
pub fn check_custody(repo: &Path, custody: &Path) -> Result<PathBuf, String> {
    if custody.as_os_str().is_empty() {
        return Err("name the custody directory".into());
    }
    let abs = resolve(custody)?;
    if abs.starts_with(repo) {
        return Err(format!(
            "{} is inside this checkout; keep the keystores outside it, where git cannot pick them up",
            shown(&abs)
        ));
    }
    if abs.exists() {
        if !abs.is_dir() {
            return Err(format!("{} is a file, not a directory", shown(&abs)));
        }
        let mut entries =
            std::fs::read_dir(&abs).map_err(|e| format!("cannot read {}: {e}", shown(&abs)))?;
        if entries.next().is_some() {
            return Err(format!(
                "{} is not empty; give each ceremony a fresh directory",
                shown(&abs)
            ));
        }
    }
    Ok(abs)
}

/// An absolute, symlink-free form of a path that may not exist yet: the nearest
/// existing ancestor, canonicalised, with the rest appended.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cannot read the current directory: {e}"))?
            .join(path)
    };
    let mut rest = Vec::new();
    let mut base = abs.as_path();
    loop {
        if let Ok(c) = std::fs::canonicalize(base) {
            let mut out = c;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return Ok(out);
        }
        match (base.parent(), base.components().next_back()) {
            (Some(parent), Some(Component::Normal(name))) => {
                rest.push(name.to_os_string());
                base = parent;
            }
            _ => return Err(format!("cannot resolve {}", shown(path))),
        }
    }
}

/// Everything that can be refused is refused here, before a key is generated.
pub fn check_plan(plan: &Plan) -> Result<(PathBuf, PathBuf), String> {
    let repo = find_repo(&plan.repo)?;
    check_not_held(&repo)?;
    let custody = check_custody(&repo, &plan.custody)?;
    if plan.year.len() != 4 || !plan.year.bytes().all(|b| b.is_ascii_digit()) {
        return Err("the year must be four digits".into());
    }
    if plan.signers.is_empty() {
        return Err("name at least one release tag signer".into());
    }
    let mut principals: Vec<&str> = plan.signers.iter().map(|s| s.principal.as_str()).collect();
    principals.sort_unstable();
    if principals.windows(2).any(|w| w[0] == w[1]) {
        return Err("each tag signer is listed once".into());
    }
    Ok((repo, custody))
}

/// Hold the ceremony.
pub fn hold(plan: &Plan, now: &Now, commit: Option<&str>) -> Result<Outcome, String> {
    let (repo, custody) = check_plan(plan)?;

    let release_id = format!("midgard-release-{}", plan.year);
    let bifrost_id = format!("midgard-bifrost-{}", plan.year);
    let mut rng = pqc_agility::OsRng;
    let release = Keystore::generate(&release_id, kms::RELEASE_SIG_ALG, &mut rng)
        .map_err(|e| format!("release key generation failed: {e}"))?;
    let bifrost = Keystore::generate(&bifrost_id, BIFROST_ALG, &mut rng)
        .map_err(|e| format!("Bifrost key generation failed: {e}"))?;

    // The keys first, read back, so nothing is published for a key not held.
    std::fs::create_dir_all(&custody)
        .map_err(|e| format!("cannot create {}: {e}", shown(&custody)))?;
    let release_keystore = custody.join("release.keystore");
    let bifrost_keystore = custody.join("bifrost.keystore");
    for (ks, path) in [(&release, &release_keystore), (&bifrost, &bifrost_keystore)] {
        ks.write(path).map_err(|e| e.to_string())?;
        let back = Keystore::read(path).map_err(|e| e.to_string())?;
        if back.export_verifying_key() != ks.export_verifying_key() {
            return Err(format!("{} did not read back as written", shown(path)));
        }
    }

    let trust = repo.join(TRUST);
    write(&trust.join(RELEASE_ANCHOR), &release.export_verifying_key())?;
    write(&trust.join(BIFROST_ANCHOR), &bifrost.export_verifying_key())?;
    let mut signers = String::from(
        "# The SSH keys that may sign a Midgard release tag. Written by the key ceremony\n\
         # (tools/mg-ceremony); checked by tools/ci/verify-tag.sh. [AC-RF-CA-6a]\n",
    );
    for s in &plan.signers {
        signers.push_str(&s.allowed_signers_line());
        signers.push('\n');
    }
    write(&trust.join(ALLOWED_SIGNERS), &signers)?;

    let published = check_published(&repo)?;

    std::fs::create_dir_all(repo.join(RECORDS))
        .map_err(|e| format!("cannot create {RECORDS}: {e}"))?;
    let mut record = PathBuf::from(RECORDS).join(format!("{}-release-keys.md", now.iso_date()));
    if repo.join(&record).exists() {
        record = PathBuf::from(RECORDS).join(format!(
            "{}-release-keys-{}.md",
            now.iso_date(),
            now.compact_time()
        ));
    }
    let text = record_text(plan, now, commit, &release_id, &bifrost_id, &published);
    write(&repo.join(&record), &text)?;

    Ok(Outcome {
        release_keystore,
        bifrost_keystore,
        release_id,
        bifrost_id,
        published,
        record,
    })
}

/// The commit the checkout is at, for the record. `None` when git is not
/// installed or the folder is not a git checkout.
pub fn git_head(repo: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    let head = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !head.is_empty()).then_some(head)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", shown(path)))
}

/// Read the three published files as the release does (`release-preflight.sh`,
/// `verify-tag.sh`), and return what they pin. A file that the release would
/// refuse is refused here.
pub fn check_published(repo: &Path) -> Result<Published, String> {
    let anchors = vec![
        check_anchor(repo, RELEASE_ANCHOR, kms::RELEASE_SIG_ALG.as_str())?,
        check_anchor(repo, BIFROST_ANCHOR, BIFROST_ALG.as_str())?,
    ];
    let path = repo.join(TRUST).join(ALLOWED_SIGNERS);
    let text = read(&path)?;
    if text.contains("PRIVATE KEY") {
        return Err(format!("{TRUST}/{ALLOWED_SIGNERS} holds a private key"));
    }
    let mut signers = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut f = line.splitn(3, ' ');
        let (principal, ns, key) = (
            f.next().unwrap_or_default(),
            f.next().unwrap_or_default(),
            f.next().unwrap_or_default(),
        );
        if ns != "namespaces=\"git\"" {
            return Err(format!(
                "{TRUST}/{ALLOWED_SIGNERS}: '{principal}' is not listed for the namespace \"git\""
            ));
        }
        signers.push(
            Signer::parse(principal, key)
                .map_err(|e| format!("{TRUST}/{ALLOWED_SIGNERS}: '{principal}': {e}"))?,
        );
    }
    if signers.is_empty() {
        return Err(format!("{TRUST}/{ALLOWED_SIGNERS} lists no tag signer"));
    }
    Ok(Published {
        anchors,
        signers,
        allowed_signers_sha256: hex(&sha256(text.as_bytes())),
    })
}

fn check_anchor(repo: &Path, file: &str, alg: &'static str) -> Result<Anchor, String> {
    let rel = format!("{TRUST}/{file}");
    let text = read(&repo.join(TRUST).join(file))?;
    let lower = text.to_ascii_lowercase();
    if lower.contains("sk=") || lower.contains("private") {
        return Err(format!(
            "{rel} holds private key material; nothing may be committed"
        ));
    }
    let mut key_ids = Vec::new();
    for line in text.lines().map(|l| l.trim_end_matches('\r')) {
        let f: Vec<&str> = line.split(' ').collect();
        if let [a, id, key] = f[..] {
            if a == alg
                && !id.is_empty()
                && !key.is_empty()
                && key.bytes().all(|b| b.is_ascii_hexdigit())
            {
                key_ids.push(id.to_string());
            }
        }
    }
    if key_ids.is_empty() {
        return Err(format!("{rel} has no \"{alg} <key-id> <hex>\" line"));
    }
    Ok(Anchor {
        file: rel,
        alg,
        key_ids,
        sha256: hex(&sha256(text.as_bytes())),
    })
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", shown(path)))
}

/// The ceremony record. It carries every value the officers check, and never where
/// the keystores are held.
fn record_text(
    plan: &Plan,
    now: &Now,
    commit: Option<&str>,
    release_id: &str,
    bifrost_id: &str,
    p: &Published,
) -> String {
    let anchor = |i: usize| &p.anchors[i];
    let mut s = format!(
        "# Key ceremony, {date}\n\
         \n\
         Release signing and Bifrost admission keys, and the release tag signers, per\n\
         `docs/ops/KEY_CEREMONY.md`. Generated by `mg-ceremony` at {stamp} (UTC)\n\
         from commit `{commit}`.\n\
         \n\
         ## Keys\n\
         \n\
         | Key | Algorithm | Key id | Published as | SHA-256 of the published file |\n\
         |---|---|---|---|---|\n\
         | Release signing | {ra} | `{release_id}` | `{rf}` | `{rs}` |\n\
         | Bifrost admission | {ba} | `{bifrost_id}` | `{bf}` | `{bs}` |\n\
         \n\
         The private halves are sealed keystores held outside the repository. Where they are\n\
         held is not recorded here.\n\
         \n\
         ## Release tag signers\n\
         \n\
         `{TRUST}/{ALLOWED_SIGNERS}`, SHA-256 `{ss}`:\n\
         \n\
         | Principal | SSH key fingerprint |\n\
         |---|---|\n",
        date = now.long_date(),
        stamp = now.iso_timestamp(),
        commit = commit.unwrap_or("unknown"),
        ra = anchor(0).alg,
        rf = anchor(0).file,
        rs = anchor(0).sha256,
        ba = anchor(1).alg,
        bf = anchor(1).file,
        bs = anchor(1).sha256,
        ss = p.allowed_signers_sha256,
    );
    for signer in &p.signers {
        s.push_str(&format!(
            "| {} | `{}` |\n",
            signer.principal, signer.fingerprint
        ));
    }
    s.push_str(&format!(
        "\n\
         ## Second channel\n\
         \n\
         The fingerprints and SHA-256 values above were also published at:\n\
         \n\
         - _where, and when_\n\
         \n\
         ## Sign-off\n\
         \n\
         Each person checked the values above against the second channel and the\n\
         screen of the ceremony machine.\n\
         \n\
         | Role | Name | Confirmed | Date |\n\
         |---|---|---|---|\n\
         | Security Officer | {} | | |\n\
         | Witness | {} | | |\n",
        plan.officer, plan.witness
    ));
    s
}

/// The message to publish through the second channel: everything a reader needs
/// to check the committed files were not swapped.
pub fn second_channel_text(p: &Published, now: &Now) -> String {
    let mut s = format!(
        "Midgard key ceremony, {}. The committed trust files must have these values.\n\n",
        now.long_date()
    );
    for a in &p.anchors {
        s.push_str(&format!(
            "{}  {}  {}\n  sha256 {}\n",
            a.file,
            a.alg,
            a.key_ids.join(", "),
            a.sha256
        ));
    }
    s.push_str(&format!(
        "{TRUST}/{ALLOWED_SIGNERS}\n  sha256 {}\n",
        p.allowed_signers_sha256
    ));
    for signer in &p.signers {
        s.push_str(&format!("  {}  {}\n", signer.principal, signer.fingerprint));
    }
    s.push_str("\nCheck on your own machine with: mg-ceremony check --repo <your checkout>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mg-ceremony-unit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("repo").join(TRUST)).unwrap();
        std::fs::write(dir.join("repo").join(TRUST).join("README.md"), "anchors\n").unwrap();
        dir
    }

    #[test]
    fn shows_windows_paths_without_the_verbatim_prefix() {
        assert_eq!(shown(r"\\?\E:\midgard-keys"), r"E:\midgard-keys");
        assert_eq!(shown(r"\\?\UNC\host\share\k"), r"\\host\share\k");
        assert_eq!(shown("/media/keys"), "/media/keys");
    }

    #[test]
    fn resolves_a_path_that_does_not_exist_yet() {
        let dir = scratch("resolve");
        let p = resolve(&dir.join("a").join("b")).unwrap();
        assert!(p.is_absolute());
        assert!(p.ends_with(Path::new("a").join("b")));
        assert!(!p.exists());
    }

    #[test]
    fn a_custody_directory_is_outside_the_checkout_and_fresh() {
        let dir = scratch("custody");
        let repo = find_repo(&dir.join("repo")).unwrap();
        assert!(check_custody(&repo, &dir.join("keys")).is_ok());
        let inside = check_custody(&repo, &dir.join("repo").join("keys")).unwrap_err();
        assert!(inside.contains("inside this checkout"), "{inside}");
        std::fs::create_dir_all(dir.join("used")).unwrap();
        std::fs::write(dir.join("used").join("old"), "x").unwrap();
        let used = check_custody(&repo, &dir.join("used")).unwrap_err();
        assert!(used.contains("not empty"), "{used}");
        std::fs::write(dir.join("afile"), "x").unwrap();
        assert!(check_custody(&repo, &dir.join("afile")).is_err());
        assert!(check_custody(&repo, Path::new("")).is_err());
    }

    #[test]
    fn a_directory_without_deploy_trust_is_not_a_checkout() {
        let dir = scratch("notrepo");
        assert!(find_repo(&dir).is_err());
        assert_eq!(
            discover_repo(&[dir.join("repo").join(TRUST)]),
            Some(find_repo(&dir.join("repo")).unwrap())
        );
    }

    #[test]
    fn an_anchor_the_release_would_refuse_is_refused() {
        let dir = scratch("anchor");
        let repo = dir.join("repo");
        let t = repo.join(TRUST);
        std::fs::write(t.join(RELEASE_ANCHOR), "ML-DSA-65 k 00ff\n").unwrap();
        assert!(check_anchor(&repo, RELEASE_ANCHOR, "ML-DSA-87").is_err());
        std::fs::write(t.join(RELEASE_ANCHOR), "ML-DSA-87 k 00ff sk=00\n").unwrap();
        assert!(check_anchor(&repo, RELEASE_ANCHOR, "ML-DSA-87").is_err());
        std::fs::write(t.join(RELEASE_ANCHOR), "ML-DSA-87 k xyz\n").unwrap();
        assert!(check_anchor(&repo, RELEASE_ANCHOR, "ML-DSA-87").is_err());
        std::fs::write(
            t.join(RELEASE_ANCHOR),
            "# old\nML-DSA-87 a 00\r\nML-DSA-87 b ff\n",
        )
        .unwrap();
        let a = check_anchor(&repo, RELEASE_ANCHOR, "ML-DSA-87").unwrap();
        assert_eq!(a.key_ids, ["a", "b"]);
    }
}
