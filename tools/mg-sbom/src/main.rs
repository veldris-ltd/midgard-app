//! mg-sbom — emit a deterministic CycloneDX SBOM for an artefact. [NFR-OSS-1]
//!
//! ```text
//! mg-sbom [--out <path>] <artefact>                       a Rust artefact, from Cargo.lock
//! mg-sbom --pnpm-lock <pnpm-lock.yaml> --importer <dir>
//!         [--out <path>] <artefact>                       a JS artefact, from pnpm-lock.yaml
//! ```
//!
//! Renders a CycloneDX 1.5 document listing every resolved component with its
//! purl — `pkg:cargo/…` from `Cargo.lock`, or `pkg:npm/…` for the production
//! closure of one pnpm importer (see [`pnpm`]). Output is **sorted and
//! timestamp-free**, so the SBOM is itself bit-reproducible alongside the artefact
//! it describes (AC-M-6, AC-B-4). No external crates — the sovereign posture.

mod pnpm;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pqc_agility::sha256;

fn main() -> ExitCode {
    // `--out <path>` writes the SBOM where the caller wants it. The release gate
    // needs it *beside the artefact* (`<artefact>.sbom.cdx.json`), because that
    // is where mg-sign and mg-verify look for the bill of materials they bind
    // into the signature. Without it, the sanitised-name-in-CWD default below
    // puts the SBOM somewhere the rest of the chain cannot find. [AC-B-4]
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut artefact: Option<String> = None;
    let mut out_override: Option<String> = None;
    let mut pnpm_lock: Option<String> = None;
    let mut importer: Option<String> = None;
    let mut list_packages = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => match args.get(i + 1) {
                Some(v) => {
                    out_override = Some(v.clone());
                    i += 2;
                }
                None => {
                    eprintln!("mg-sbom: --out requires a value");
                    return ExitCode::from(2);
                }
            },
            "--list-packages" => {
                list_packages = true;
                i += 1;
            }
            flag @ ("--pnpm-lock" | "--importer") => match args.get(i + 1) {
                Some(v) if !v.starts_with("--") => {
                    if flag == "--pnpm-lock" {
                        pnpm_lock = Some(v.clone());
                    } else {
                        importer = Some(v.clone());
                    }
                    i += 2;
                }
                _ => {
                    eprintln!("mg-sbom: {flag} requires a value");
                    return ExitCode::from(2);
                }
            },
            other if other.starts_with("--") => {
                eprintln!("mg-sbom: unknown flag {other}");
                return ExitCode::from(2);
            }
            other => {
                artefact = Some(other.to_string());
                i += 1;
            }
        }
    }
    // `--pnpm-lock <file> --list-packages`: every registry package the lockfile
    // pins, one per line. The npm advisory snapshot is filtered to exactly this
    // set, so it has to come from the same reader the SBOM does.
    if list_packages {
        let Some(lock_path) = &pnpm_lock else {
            eprintln!("mg-sbom: --list-packages needs --pnpm-lock <file>");
            return ExitCode::from(2);
        };
        let names = std::fs::read_to_string(lock_path)
            .map_err(|e| format!("cannot read {lock_path}: {e}"))
            .and_then(|text| pnpm::Lock::parse(&text))
            .map(|lock| lock.package_names());
        return match names {
            Ok(names) => {
                for n in names {
                    println!("{n}");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("mg-sbom: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let artefact = match artefact {
        Some(a) => a,
        None => {
            eprintln!(
                "usage: mg-sbom [--pnpm-lock <pnpm-lock.yaml> --importer <dir>] [--out <path>] <artefact-name>"
            );
            return ExitCode::from(2);
        }
    };
    let components = match (pnpm_lock, importer) {
        (None, None) => {
            let lock = match std::fs::read_to_string("Cargo.lock") {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("mg-sbom: cannot read Cargo.lock: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let packages = parse_lockfile(&lock);
            let root = Path::new(".");
            let (members, workspace_licence) = workspace_members(root);
            resolve_licences(
                &packages,
                &root.join("vendor"),
                &members,
                workspace_licence.as_deref(),
            )
        }
        (Some(lock_path), Some(importer)) => {
            let text = match std::fs::read_to_string(&lock_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("mg-sbom: cannot read {lock_path}: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let root = match Path::new(&lock_path).parent() {
                Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
                _ => PathBuf::from("."),
            };
            let resolved =
                pnpm::Lock::parse(&text).and_then(|lock| pnpm::components(&lock, &importer, &root));
            match resolved {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("mg-sbom: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
        _ => {
            eprintln!("mg-sbom: --pnpm-lock and --importer go together");
            return ExitCode::from(2);
        }
    };
    let unlicensed = components.iter().filter(|c| c.licence.is_none()).count();
    if unlicensed > 0 {
        // Not fatal here — mg-scan is the gate. But say so, because a silently
        // licence-less SBOM is how an unscannable component reaches a release.
        eprintln!(
            "mg-sbom: WARNING — {unlicensed} component(s) have no resolvable licence; mg-scan will refuse them"
        );
    }
    // Self-hash: when the positional artefact is a real file (the release gate
    // passes `target/release/<name>`), hash it so the SBOM records the exact
    // content it describes. A synthetic name that is not a file yields no
    // self-hash (tests, doc runs). [RT-41]
    let artefact_hash = match std::fs::read(&artefact) {
        Ok(bytes) => Some(sha256(&bytes)),
        Err(_) => None,
    };
    let hashed = components
        .iter()
        .filter(|c| c.checksum.is_some() || c.sha512.is_some())
        .count();
    let sbom = render_cyclonedx(&artefact, &components, artefact_hash.as_ref());
    let out = out_override.unwrap_or_else(|| format!("{}.sbom.cdx.json", sanitize(&artefact)));
    match std::fs::write(&out, &sbom) {
        Ok(()) => {
            eprintln!(
                "mg-sbom: wrote {} ({} components, {} with integrity hashes{})",
                out,
                components.len(),
                hashed,
                if artefact_hash.is_some() {
                    ", + artefact self-hash"
                } else {
                    ""
                }
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("mg-sbom: cannot write {out}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// One resolved SBOM component.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Component {
    /// Crate name.
    pub name: String,
    /// Resolved version.
    pub version: String,
    /// SPDX licence expression, or `None` when it could not be resolved.
    ///
    /// `None` is emitted as an explicit absence, never as a guess: `mg-scan`
    /// treats an unknown licence as a failure, and inventing a plausible one
    /// here would turn that gate off silently. [AC-RF-SC-4c]
    pub licence: Option<String>,
    /// The SHA-256 integrity hash Cargo pins for this crate in `Cargo.lock` (the
    /// digest of the downloaded `.crate`). `None` for path/workspace members, which
    /// are built from source, not fetched. Emitted as `components[].hashes`. [RT-41]
    pub checksum: Option<String>,
    /// The SHA-512 integrity hash pnpm pins for a registry package, as hex.
    /// `None` for Cargo packages and for workspace packages.
    pub sha512: Option<String>,
    /// Where Cargo resolved this package from, as recorded in `Cargo.lock`.
    /// Emitted as the `midgard:source` property so `mg-scan` can tell our own
    /// code from a registry crate that merely shares its name — on a positive
    /// statement, not by inferring it from a missing hash. [RT-41]
    pub source: Source,
    /// Which package ecosystem this component belongs to. It decides the purl
    /// type, and through that which advisory database `mg-scan` matches it
    /// against — a crate and an npm package can share a name.
    pub ecosystem: Ecosystem,
}

/// The package ecosystem a component comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ecosystem {
    /// crates.io packages, from `Cargo.lock`.
    Cargo,
    /// npm packages, from `pnpm-lock.yaml`.
    Npm,
}

impl Ecosystem {
    /// The package URL for a component of this ecosystem. An npm scope's `@` is
    /// percent-encoded, as the purl specification requires.
    fn purl(self, name: &str, version: &str) -> String {
        match self {
            Ecosystem::Cargo => format!("pkg:cargo/{name}@{version}"),
            Ecosystem::Npm => format!("pkg:npm/{}@{version}", name.replacen('@', "%40", 1)),
        }
    }
}

/// A locked package's origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// No `source` line: a path dependency, i.e. a workspace member.
    Workspace,
    /// A registry (crates.io or a mirror of it).
    Registry,
    /// A git repository.
    Git,
    /// Anything else Cargo may record.
    Other,
}

impl Source {
    fn from_lock(source: Option<&str>) -> Self {
        match source {
            None => Source::Workspace,
            Some(s) if s.starts_with("registry+") || s.starts_with("sparse+") => Source::Registry,
            Some(s) if s.starts_with("git+") => Source::Git,
            Some(_) => Source::Other,
        }
    }

    /// The value written to the SBOM.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Workspace => "workspace",
            Source::Registry => "registry",
            Source::Git => "git",
            Source::Other => "other",
        }
    }
}

/// One `[[package]]` of a Cargo.lock: name, version, integrity checksum, origin.
type Locked = (String, String, Option<String>, Source);

/// Extract every `[[package]]` in a Cargo.lock, sorted and deduplicated for
/// determinism.
fn parse_lockfile(s: &str) -> Vec<Locked> {
    let mut out = Vec::new();
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut checksum: Option<String> = None;
    let mut source: Option<String> = None;
    // Flush the accumulated package at each new `[[package]]` and at EOF, so a
    // `checksum` line that follows the version is captured with it. [RT-41]
    let flush = |out: &mut Vec<Locked>,
                 name: &mut Option<String>,
                 version: &mut Option<String>,
                 checksum: &mut Option<String>,
                 source: &mut Option<String>| {
        if let (Some(n), Some(v)) = (name.take(), version.take()) {
            out.push((n, v, checksum.take(), Source::from_lock(source.as_deref())));
        }
        *checksum = None;
        *source = None;
    };
    for line in s.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            flush(
                &mut out,
                &mut name,
                &mut version,
                &mut checksum,
                &mut source,
            );
        } else if let Some(v) = line.strip_prefix("name = \"") {
            name = Some(v.trim_end_matches('"').to_string());
        } else if let Some(v) = line.strip_prefix("version = \"") {
            version = Some(v.trim_end_matches('"').to_string());
        } else if let Some(v) = line.strip_prefix("source = \"") {
            source = Some(v.trim_end_matches('"').to_string());
        } else if let Some(v) = line.strip_prefix("checksum = \"") {
            let c = v.trim_end_matches('"').to_string();
            // Only accept a well-formed 64-hex SHA-256; ignore anything else.
            if c.len() == 64 && c.bytes().all(|b| b.is_ascii_hexdigit()) {
                checksum = Some(c);
            }
        }
    }
    flush(
        &mut out,
        &mut name,
        &mut version,
        &mut checksum,
        &mut source,
    );
    out.sort();
    out.dedup();
    out
}

/// Read `license = "..."` from a manifest, resolving `license.workspace = true`
/// against the workspace root.
fn licence_from_manifest(text: &str, workspace_licence: Option<&str>) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("license = \"") {
            return Some(v.trim_end_matches('"').to_string());
        }
        if line.starts_with("license.workspace") || line.starts_with("license = { workspace") {
            return workspace_licence.map(|s| s.to_string());
        }
    }
    None
}

/// Resolve a licence for every locked package.
///
/// Two sources, both local so the SBOM can be produced offline: the vendored
/// crate's own manifest (`vendor/<name>-<version>/Cargo.toml`) for third-party
/// code, and the workspace member's manifest for first-party code. A package
/// found in neither gets `None` rather than a default.
fn resolve_licences(
    packages: &[Locked],
    vendor_dir: &Path,
    members: &BTreeMap<String, PathBuf>,
    workspace_licence: Option<&str>,
) -> Vec<Component> {
    packages
        .iter()
        .map(|(name, version, checksum, source)| {
            let vendored = vendor_dir
                .join(format!("{name}-{version}"))
                .join("Cargo.toml");
            let licence = std::fs::read_to_string(&vendored)
                .ok()
                .and_then(|t| licence_from_manifest(&t, workspace_licence))
                .or_else(|| {
                    members
                        .get(name)
                        .and_then(|p| std::fs::read_to_string(p).ok())
                        .and_then(|t| licence_from_manifest(&t, workspace_licence))
                });
            Component {
                name: name.clone(),
                version: version.clone(),
                licence,
                checksum: checksum.clone(),
                sha512: None,
                source: *source,
                ecosystem: Ecosystem::Cargo,
            }
        })
        .collect()
}

/// Map workspace member package name -> its Cargo.toml path.
fn workspace_members(root: &Path) -> (BTreeMap<String, PathBuf>, Option<String>) {
    let mut members = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(root.join("Cargo.toml")) else {
        return (members, None);
    };
    let workspace_licence = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("license = \""))
        .map(|v| v.trim_end_matches('"').to_string());

    if let Some(start) = text
        .find("members")
        .and_then(|i| text[i..].find('[').map(|j| i + j + 1))
    {
        if let Some(len) = text[start..].find(']') {
            for raw in text[start..start + len].split(',') {
                let dir = raw.trim().trim_matches('"').trim();
                if dir.is_empty() {
                    continue;
                }
                let manifest = root.join(dir).join("Cargo.toml");
                if let Ok(m) = std::fs::read_to_string(&manifest) {
                    if let Some(name) = m
                        .lines()
                        .find_map(|l| l.trim().strip_prefix("name = \""))
                        .map(|v| v.trim_end_matches('"').to_string())
                    {
                        members.insert(name, manifest);
                    }
                }
            }
        }
    }
    (members, workspace_licence)
}

fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            _ => o.push(c),
        }
    }
    o
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Render a deterministic CycloneDX 1.5 document. No `metadata.timestamp` (it
/// would break reproducibility); components are pre-sorted by the caller.
fn render_cyclonedx(
    artefact: &str,
    components: &[Component],
    artefact_hash: Option<&[u8; 32]>,
) -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str("  \"bomFormat\": \"CycloneDX\",\n");
    s.push_str("  \"specVersion\": \"1.5\",\n");
    s.push_str("  \"version\": 1,\n");
    s.push_str("  \"metadata\": {\n");
    // The described artefact. When its concrete file was hashed, carry a SHA-256
    // self-hash so the SBOM records the exact content it describes (the signature
    // also binds this digest, via attest; this makes it visible in the document).
    // [RT-41]
    match artefact_hash {
        Some(h) => s.push_str(&format!(
            "    \"component\": {{ \"type\": \"application\", \"name\": \"{}\", \"version\": \"0.0.0\", \"hashes\": [ {{ \"alg\": \"SHA-256\", \"content\": \"{}\" }} ] }}\n",
            json_escape(artefact),
            hex(h)
        )),
        None => s.push_str(&format!(
            "    \"component\": {{ \"type\": \"application\", \"name\": \"{}\", \"version\": \"0.0.0\" }}\n",
            json_escape(artefact)
        )),
    }
    s.push_str("  },\n");
    s.push_str("  \"components\": [\n");
    for (i, c) in components.iter().enumerate() {
        let (n, v) = (json_escape(&c.name), json_escape(&c.version));
        // CycloneDX models an SPDX expression as `licenses: [{ expression }]`.
        // A component with no resolvable licence emits an EMPTY array rather
        // than a guess or an omitted key, so "we did not find one" is
        // distinguishable from "nobody looked". [AC-RF-SC-4c]
        let licences = match &c.licence {
            Some(l) => format!("[ {{ \"expression\": \"{}\" }} ]", json_escape(l)),
            None => "[]".to_string(),
        };
        let purl = json_escape(&c.ecosystem.purl(&c.name, &c.version));
        s.push_str(&format!(
            "    {{ \"type\": \"library\", \"name\": \"{n}\", \"version\": \"{v}\", \"purl\": \"{purl}\", \"licenses\": {licences}"
        ));
        // The registry SHA-256 Cargo pins — real, verifiable per-component
        // integrity data the whole-SBOM digest does not provide. [RT-41]
        if let Some(cs) = &c.checksum {
            s.push_str(&format!(
                ", \"hashes\": [ {{ \"alg\": \"SHA-256\", \"content\": \"{cs}\" }} ]"
            ));
        } else if let Some(cs) = &c.sha512 {
            // The registry SHA-512 pnpm pins for an npm package.
            s.push_str(&format!(
                ", \"hashes\": [ {{ \"alg\": \"SHA-512\", \"content\": \"{cs}\" }} ]"
            ));
        }
        // Where the package came from, so the scanner need not guess. [RT-41]
        s.push_str(&format!(
            ", \"properties\": [ {{ \"name\": \"midgard:source\", \"value\": \"{}\" }} ]",
            c.source.as_str()
        ));
        s.push_str(" }");
        if i + 1 < components.len() {
            s.push(',');
        }
        s.push('\n');
    }
    s.push_str("  ]\n");
    s.push_str("}\n");
    s
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const CK: &str = "831dc8000c9e1f27f5ab7bcaf07ad3d573a4d95f21b7bb13ec00b9d2c5e1c8c0";
    const SAMPLE: &str = r#"
# This file is automatically @generated by Cargo.
version = 3

[[package]]
name = "fabric"
version = "0.0.0"

[[package]]
name = "aes-gcm"
version = "0.10.3"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "831dc8000c9e1f27f5ab7bcaf07ad3d573a4d95f21b7bb13ec00b9d2c5e1c8c0"
dependencies = [
 "aead",
]
"#;

    #[test]
    fn parses_sorts_and_captures_checksums() {
        let comps = parse_lockfile(SAMPLE);
        assert_eq!(
            comps,
            vec![
                (
                    "aes-gcm".to_string(),
                    "0.10.3".to_string(),
                    Some(CK.to_string()),
                    Source::Registry
                ),
                (
                    "fabric".to_string(),
                    "0.0.0".to_string(),
                    None,
                    Source::Workspace
                ),
            ]
        );
    }

    #[test]
    fn a_packages_origin_comes_from_its_lockfile_source() {
        let lock = "[[package]]\nname = \"ours\"\nversion = \"0.1.0\"\n\n\
                    [[package]]\nname = \"from-git\"\nversion = \"1.0.0\"\n\
                    source = \"git+https://example.invalid/repo#abc\"\n\n\
                    [[package]]\nname = \"odd\"\nversion = \"1.0.0\"\nsource = \"path+file:///x\"\n";
        let got: Vec<(String, Source)> = parse_lockfile(lock)
            .into_iter()
            .map(|(n, _, _, s)| (n, s))
            .collect();
        assert_eq!(
            got,
            [
                ("from-git".to_string(), Source::Git),
                ("odd".to_string(), Source::Other),
                ("ours".to_string(), Source::Workspace),
            ]
        );
        // A source line must not leak from one package into the next.
        assert_eq!(parse_lockfile(SAMPLE)[1].3, Source::Workspace);
    }

    #[test]
    fn every_component_states_its_origin() {
        let sbom = render_cyclonedx(
            "x",
            &[
                comp_ck("aes-gcm", "0.10.3", Some("MIT"), CK),
                comp("fabric", "0.0.0", Some("Apache-2.0")),
            ],
            None,
        );
        assert!(sbom.contains(
            "\"properties\": [ { \"name\": \"midgard:source\", \"value\": \"registry\" } ]"
        ));
        assert!(sbom.contains(
            "\"properties\": [ { \"name\": \"midgard:source\", \"value\": \"workspace\" } ]"
        ));
    }

    #[test]
    fn rejects_a_malformed_checksum() {
        // The old sample used a non-hex `abc123`; such a value is dropped, never
        // emitted as a bogus integrity hash. [RT-41]
        let comps =
            parse_lockfile("[[package]]\nname = \"x\"\nversion = \"1\"\nchecksum = \"abc123\"\n");
        assert_eq!(comps[0].2, None);
    }

    fn comp(name: &str, version: &str, licence: Option<&str>) -> Component {
        Component {
            name: name.into(),
            version: version.into(),
            licence: licence.map(|s| s.to_string()),
            checksum: None,
            sha512: None,
            source: Source::Workspace,
            ecosystem: Ecosystem::Cargo,
        }
    }
    fn comp_ck(name: &str, version: &str, licence: Option<&str>, checksum: &str) -> Component {
        Component {
            checksum: Some(checksum.into()),
            source: Source::Registry,
            ..comp(name, version, licence)
        }
    }

    #[test]
    fn renders_deterministic_cyclonedx_with_purls_and_component_hashes() {
        let comps = vec![
            comp_ck("aes-gcm", "0.10.3", Some("Apache-2.0 OR MIT"), CK),
            comp("fabric", "0.0.0", Some("Apache-2.0")), // path member: no hash
        ];
        let sbom = render_cyclonedx("midgard-core", &comps, None);
        assert!(sbom.contains("\"bomFormat\": \"CycloneDX\""));
        assert!(sbom.contains("\"purl\": \"pkg:cargo/aes-gcm@0.10.3\""));
        // Registry component carries its SHA-256 integrity hash; path member does not.
        assert!(sbom.contains(&format!(
            "\"hashes\": [ {{ \"alg\": \"SHA-256\", \"content\": \"{CK}\" }} ]"
        )));
        assert!(!sbom.contains("timestamp"), "no timestamp → reproducible");
        // Rendering is a pure function of the inputs → identical twice.
        assert_eq!(sbom, render_cyclonedx("midgard-core", &comps, None));
    }

    #[test]
    fn self_hash_binds_the_artefact_when_hashed() {
        let content = b"the concrete artefact bytes";
        let h = sha256(content);
        let sbom = render_cyclonedx("midgard-core", &[comp("fabric", "0.0.0", None)], Some(&h));
        // metadata.component carries the artefact self-hash.
        assert!(sbom
            .split("\"components\"")
            .next()
            .unwrap()
            .contains(&format!("\"content\": \"{}\"", hex(&h))));
        assert_eq!(
            sbom,
            render_cyclonedx("midgard-core", &[comp("fabric", "0.0.0", None)], Some(&h))
        );
    }

    #[test]
    fn emits_licences_as_spdx_expressions() {
        let sbom = render_cyclonedx(
            "x",
            &[comp("aes-gcm", "0.10.3", Some("Apache-2.0 OR MIT"))],
            None,
        );
        assert!(sbom.contains("\"licenses\": [ { \"expression\": \"Apache-2.0 OR MIT\" } ]"));
    }

    #[test]
    fn an_unresolvable_licence_is_an_empty_array_not_a_guess() {
        // "we looked and found nothing" must be distinguishable from "nobody
        // looked" — mg-scan refuses the former. [AC-RF-SC-4c]
        let sbom = render_cyclonedx("x", &[comp("mystery", "1.0.0", None)], None);
        assert!(sbom.contains("\"licenses\": []"));
        assert!(!sbom.contains("NOASSERTION"));
        assert!(!sbom.contains("Apache"));
    }

    #[test]
    fn licence_resolution_reads_manifests_and_the_workspace_default() {
        assert_eq!(
            licence_from_manifest("name = \"x\"\nlicense = \"MIT\"\n", None),
            Some("MIT".into())
        );
        assert_eq!(
            licence_from_manifest("license.workspace = true\n", Some("Apache-2.0")),
            Some("Apache-2.0".into())
        );
        // Inheriting from a workspace that declares nothing yields nothing.
        assert_eq!(
            licence_from_manifest("license.workspace = true\n", None),
            None
        );
        assert_eq!(
            licence_from_manifest("name = \"x\"\n", Some("Apache-2.0")),
            None
        );
    }
}
