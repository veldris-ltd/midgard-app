//! The bill of materials for a JavaScript artefact, from `pnpm-lock.yaml`.
//! [NFR-OSS-1] [AC-B-4] [AC-RF-SC-5b]
//!
//! A UI bundle or an npm package tarball ships its *production* dependency
//! closure: what the importer declares under `dependencies` and
//! `optionalDependencies`, and everything those pull in. Build tooling
//! (`devDependencies` — the bundler, the compiler, type packages) is not in the
//! artefact and is not listed; it is pinned by the same lockfile's integrity
//! hashes, which is what makes the build repeatable.
//!
//! Workspace packages the importer links to are followed and listed too, marked
//! `workspace`, exactly as `Cargo.lock` path members are: they are our own code,
//! and the scanner must not confuse them with a registry package of the same
//! name.
//!
//! ## Reading the lockfile
//! Lockfile version 9 only, and only the shape pnpm writes — this is a reader
//! for one generated format, not a YAML parser. Anything it does not recognise
//! is an error rather than a skipped entry: a dependency this reader cannot
//! follow is a dependency missing from the bill of materials.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component as PathPart, Path, PathBuf};

use attest::json::{self, Json};

use crate::{Component, Ecosystem, Source};

/// The lockfile version this reader understands.
const LOCKFILE_VERSION: &str = "9.0";

/// A parsed `pnpm-lock.yaml`.
#[derive(Debug, Default)]
pub struct Lock {
    /// importer path → its production dependencies, as `(name, version)`.
    importers: BTreeMap<String, Vec<(String, String)>>,
    /// `name@version` → the registry integrity string (`sha512-…`).
    integrity: BTreeMap<String, String>,
    /// snapshot key (`name@version(peers)`) → its dependencies.
    snapshots: BTreeMap<String, Vec<(String, String)>>,
}

/// Strip a YAML single-quoted scalar's quotes.
fn unquote(s: &str) -> &str {
    s.strip_prefix('\'')
        .and_then(|v| v.strip_suffix('\''))
        .unwrap_or(s)
}

/// Split a `key: value` line, where the key may be single-quoted and may itself
/// contain a colon (`'@scope/name': 1.0.0`).
fn key_value(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix('\'') {
        let end = rest.find('\'')?;
        let value = rest[end + 1..].strip_prefix(':')?.trim();
        return Some((&rest[..end], value));
    }
    let (k, v) = line.split_once(':')?;
    Some((k.trim(), v.trim()))
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

impl Lock {
    /// Parse the lockfile text.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lock = Lock::default();
        let mut section = "";
        let mut entry = String::new();
        let mut sub = String::new();
        let mut dep = String::new();
        let mut version_seen = false;

        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim_end();
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let err = |what: &str| format!("pnpm-lock.yaml line {}: {what}", n + 1);
            match indent(line) {
                0 => {
                    let (k, v) = key_value(line).ok_or_else(|| err("expected `key:`"))?;
                    if k == "lockfileVersion" {
                        version_seen = true;
                        if unquote(v) != LOCKFILE_VERSION {
                            return Err(err(&format!(
                                "lockfile version {v} is not {LOCKFILE_VERSION}"
                            )));
                        }
                    }
                    section = match k {
                        "importers" => "importers",
                        "packages" => "packages",
                        "snapshots" => "snapshots",
                        _ => "",
                    };
                    entry.clear();
                    sub.clear();
                }
                2 if !section.is_empty() => {
                    let (k, _) = key_value(line).ok_or_else(|| err("expected an entry"))?;
                    entry = k.to_string();
                    sub.clear();
                    match section {
                        "importers" => {
                            lock.importers.entry(entry.clone()).or_default();
                        }
                        "snapshots" => {
                            lock.snapshots.entry(entry.clone()).or_default();
                        }
                        _ => {}
                    }
                }
                4 if !section.is_empty() => {
                    let (k, v) = key_value(line).ok_or_else(|| err("expected a field"))?;
                    sub = k.to_string();
                    if section == "packages" && k == "resolution" {
                        let integrity = v
                            .split("integrity:")
                            .nth(1)
                            .map(|s| s.trim().trim_end_matches('}').trim().trim_end_matches(','))
                            .map(|s| s.split(',').next().unwrap_or(s).trim());
                        match integrity {
                            Some(i) if !i.is_empty() => {
                                lock.integrity.insert(entry.clone(), i.to_string());
                            }
                            // A tarball or git resolution has no registry
                            // integrity; the closure walk refuses those.
                            _ => {}
                        }
                    }
                }
                6 if section == "importers" => {
                    let (k, _) = key_value(line).ok_or_else(|| err("expected a dependency"))?;
                    dep = k.to_string();
                }
                8 if section == "importers" => {
                    let (k, v) = key_value(line).ok_or_else(|| err("expected a field"))?;
                    if k == "version" && is_shipped(&sub) {
                        lock.importers
                            .entry(entry.clone())
                            .or_default()
                            .push((dep.clone(), unquote(v).to_string()));
                    }
                }
                6 if section == "snapshots" => {
                    if is_shipped(&sub) {
                        let (k, v) =
                            key_value(line).ok_or_else(|| err("expected `name: version`"))?;
                        lock.snapshots
                            .entry(entry.clone())
                            .or_default()
                            .push((k.to_string(), unquote(v).to_string()));
                    }
                }
                _ => {}
            }
        }
        if !version_seen {
            return Err("pnpm-lock.yaml declares no lockfileVersion".into());
        }
        Ok(lock)
    }

    /// Every registry package the lockfile pins, by name. The npm advisory
    /// snapshot is filtered to these.
    pub fn package_names(&self) -> BTreeSet<String> {
        self.integrity
            .keys()
            .filter_map(|k| split_spec(k).map(|(name, _)| name.to_string()))
            .collect()
    }
}

/// Whether a dependency group is part of what ships.
fn is_shipped(group: &str) -> bool {
    matches!(group, "dependencies" | "optionalDependencies")
}

/// Split `name@version` at the version's `@`, leaving a scope's `@` alone.
fn split_spec(spec: &str) -> Option<(&str, &str)> {
    let at = spec.char_indices().skip(1).find(|(_, c)| *c == '@')?.0;
    Some((&spec[..at], &spec[at + 1..]))
}

/// A version without its peer-dependency suffix: `6.30.4(react@18.3.1)` → `6.30.4`.
fn base_version(version: &str) -> &str {
    version.split('(').next().unwrap_or(version)
}

/// Resolve `link:../../x` against an importer path, without touching the disk.
fn resolve_link(importer: &str, link: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for p in Path::new(importer).join(link).components() {
        match p {
            PathPart::ParentDir => {
                parts.pop();
            }
            PathPart::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            _ => {}
        }
    }
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

/// `sha512-<base64>` → lowercase hex, or `None` for any other integrity form.
fn sha512_hex(integrity: &str) -> Option<String> {
    let bytes = base64_decode(integrity.strip_prefix("sha512-")?)?;
    (bytes.len() == 64).then(|| crate::hex(&bytes))
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A `package.json`'s licence as one SPDX expression: the modern `license`
/// string, the object form, or the legacy `licenses` array (alternatives).
fn licence_of(manifest: &Json) -> Option<String> {
    let one = |v: &Json| -> Option<String> {
        v.as_str()
            .or_else(|| v.get("type").and_then(Json::as_str))
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty())
    };
    if let Some(l) = manifest.get("license").and_then(one) {
        return Some(l);
    }
    let many: Vec<String> = manifest
        .get("licenses")?
        .as_array()?
        .iter()
        .filter_map(one)
        .collect();
    (!many.is_empty()).then(|| many.join(" OR "))
}

fn read_manifest(path: &Path) -> Option<Json> {
    json::parse(std::fs::read_to_string(path).ok()?.trim()).ok()
}

/// Where pnpm installed a registry package, if it is installed.
///
/// The virtual store names a directory `<name>@<version>` with `/` written as
/// `+`, followed for some packages by a peer suffix. Looked up by prefix rather
/// than reconstructed, so this does not have to mirror pnpm's suffix encoding.
fn installed_manifest(store: &[String], root: &Path, name: &str, version: &str) -> Option<PathBuf> {
    let stem = format!("{}@{}", name.replace('/', "+"), version);
    let dir = store
        .iter()
        .find(|d| *d == &stem || d.starts_with(&format!("{stem}_")))?;
    Some(
        root.join("node_modules/.pnpm")
            .join(dir)
            .join("node_modules")
            .join(name)
            .join("package.json"),
    )
}

/// The components an importer ships: itself, the workspace packages it links,
/// and its production registry closure.
///
/// `root` is the directory holding the lockfile. Licences come from each
/// package's own `package.json` — in `node_modules` for registry packages, so an
/// uninstalled workspace yields components with no licence, which the scan gate
/// refuses rather than assumes.
pub fn components(lock: &Lock, importer: &str, root: &Path) -> Result<Vec<Component>, String> {
    let importer = importer.trim_end_matches('/').replace('\\', "/");
    if !lock.importers.contains_key(&importer) {
        return Err(format!(
            "pnpm-lock.yaml has no importer {importer:?} (known: {})",
            lock.importers
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let root_licence = read_manifest(&root.join("package.json")).and_then(|m| licence_of(&m));
    let store: Vec<String> = std::fs::read_dir(root.join("node_modules/.pnpm"))
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();

    let mut out: BTreeMap<(String, String), Component> = BTreeMap::new();
    let mut seen_importers: BTreeSet<String> = BTreeSet::new();
    let mut seen_snapshots: BTreeSet<String> = BTreeSet::new();
    // (dependencies to follow, the importer they are relative to)
    let mut queue: Vec<(Vec<(String, String)>, String)> = Vec::new();

    let mut visit_importer = |path: &str,
                              out: &mut BTreeMap<(String, String), Component>,
                              queue: &mut Vec<(Vec<(String, String)>, String)>|
     -> Result<(), String> {
        if !seen_importers.insert(path.to_string()) {
            return Ok(());
        }
        let deps = lock.importers.get(path).ok_or_else(|| {
            format!("a workspace link points at {path:?}, which is not an importer")
        })?;
        let manifest = read_manifest(&root.join(path).join("package.json"))
            .ok_or_else(|| format!("cannot read {path}/package.json"))?;
        let field = |k: &str| manifest.get(k).and_then(Json::as_str).map(str::to_string);
        let name = field("name").ok_or_else(|| format!("{path}/package.json has no name"))?;
        let version = field("version").unwrap_or_else(|| "0.0.0".into());
        out.insert(
            (name.clone(), version.clone()),
            Component {
                name,
                version,
                licence: licence_of(&manifest).or_else(|| root_licence.clone()),
                checksum: None,
                sha512: None,
                source: Source::Workspace,
                ecosystem: Ecosystem::Npm,
            },
        );
        queue.push((deps.clone(), path.to_string()));
        Ok(())
    };

    visit_importer(&importer, &mut out, &mut queue)?;

    while let Some((deps, from)) = queue.pop() {
        for (name, version) in deps {
            if let Some(link) = version.strip_prefix("link:") {
                visit_importer(&resolve_link(&from, link), &mut out, &mut queue)?;
                continue;
            }
            // A registry package is `name@<semver>[(peers)]`. Aliases, tarballs
            // and git sources resolve to something this reader cannot pin to an
            // integrity hash, so they are refused rather than half-described.
            let base = base_version(&version);
            if base.contains(':') || base.contains('/') || base.is_empty() {
                return Err(format!(
                    "dependency {name} resolves to {version:?}, which is not a registry version"
                ));
            }
            let key = format!("{name}@{version}");
            if !seen_snapshots.insert(key.clone()) {
                continue;
            }
            let next = lock
                .snapshots
                .get(&key)
                .ok_or_else(|| format!("pnpm-lock.yaml has no snapshot for {key}"))?;
            let integrity = lock
                .integrity
                .get(&format!("{name}@{base}"))
                .ok_or_else(|| format!("pnpm-lock.yaml pins no integrity for {name}@{base}"))?;
            let sha512 = sha512_hex(integrity)
                .ok_or_else(|| format!("{name}@{base}: integrity {integrity:?} is not sha512"))?;
            let licence = installed_manifest(&store, root, &name, base)
                .and_then(|p| read_manifest(&p))
                .and_then(|m| licence_of(&m));
            out.insert(
                (name.clone(), base.to_string()),
                Component {
                    name,
                    version: base.to_string(),
                    licence,
                    checksum: None,
                    sha512: Some(sha512),
                    source: Source::Registry,
                    ecosystem: Ecosystem::Npm,
                },
            );
            // Registry packages never link back into the workspace.
            queue.push((next.clone(), from.clone()));
        }
    }
    Ok(out.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 64 zero bytes, base64.
    const ZERO: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

    fn lock_text() -> String {
        format!(
            r#"lockfileVersion: '9.0'

settings:
  autoInstallPeers: true

importers:

  .: {{}}

  apps/console:
    dependencies:
      '@acme/kit':
        specifier: workspace:*
        version: link:../../libs/kit
      react-dom:
        specifier: ^18.3.1
        version: 18.3.1(react@18.3.1)
    devDependencies:
      vite:
        specifier: ^5.4.11
        version: 5.4.21

  libs/kit:
    dependencies:
      react:
        specifier: ^18.3.1
        version: 18.3.1

packages:

  '@scope/leaf@1.0.0':
    resolution: {{integrity: {ZERO}}}

  react-dom@18.3.1:
    resolution: {{integrity: {ZERO}}}
    peerDependencies:
      react: ^18.3.1

  react@18.3.1:
    resolution: {{integrity: {ZERO}}}
    engines: {{node: '>=0.10.0'}}

  vite@5.4.21:
    resolution: {{integrity: {ZERO}}}

snapshots:

  '@scope/leaf@1.0.0': {{}}

  react-dom@18.3.1(react@18.3.1):
    dependencies:
      '@scope/leaf': 1.0.0
      react: 18.3.1

  react@18.3.1: {{}}

  vite@5.4.21:
    dependencies:
      react: 18.3.1
"#
        )
    }

    /// A workspace on disk matching `lock_text`, optionally "installed".
    fn workspace(tag: &str, installed: bool) -> PathBuf {
        let root = std::env::temp_dir().join(format!("mg-sbom-pnpm-{tag}"));
        let _ = std::fs::remove_dir_all(&root);
        let write = |rel: &str, text: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        write("package.json", r#"{"name":"root","license":"Apache-2.0"}"#);
        write(
            "apps/console/package.json",
            r#"{"name":"@acme/console","version":"2.0.0"}"#,
        );
        write(
            "libs/kit/package.json",
            r#"{"name":"@acme/kit","version":"1.1.0","license":"MIT"}"#,
        );
        if installed {
            write(
                "node_modules/.pnpm/react@18.3.1/node_modules/react/package.json",
                r#"{"name":"react","license":"MIT"}"#,
            );
            write(
                "node_modules/.pnpm/react-dom@18.3.1_react@18.3.1/node_modules/react-dom/package.json",
                r#"{"name":"react-dom","license":{"type":"MIT"}}"#,
            );
            write(
                "node_modules/.pnpm/@scope+leaf@1.0.0/node_modules/@scope/leaf/package.json",
                r#"{"name":"@scope/leaf","licenses":[{"type":"MIT"},{"type":"Apache-2.0"}]}"#,
            );
        }
        root
    }

    fn names(c: &[Component]) -> Vec<(&str, &str, Source)> {
        c.iter()
            .map(|c| (c.name.as_str(), c.version.as_str(), c.source))
            .collect()
    }

    #[test]
    fn the_production_closure_is_listed_and_build_tooling_is_not() {
        let root = workspace("closure", true);
        let lock = Lock::parse(&lock_text()).expect("parses");
        let c = components(&lock, "apps/console", &root).expect("resolves");

        assert_eq!(
            names(&c),
            [
                ("@acme/console", "2.0.0", Source::Workspace),
                ("@acme/kit", "1.1.0", Source::Workspace),
                ("@scope/leaf", "1.0.0", Source::Registry),
                ("react", "18.3.1", Source::Registry),
                ("react-dom", "18.3.1", Source::Registry),
            ],
            "the importer, the workspace package it links, and the registry closure — no vite"
        );
        assert!(c.iter().all(|c| c.ecosystem == Ecosystem::Npm));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn licences_and_integrity_come_from_the_packages_themselves() {
        let root = workspace("licences", true);
        let lock = Lock::parse(&lock_text()).unwrap();
        let c = components(&lock, "apps/console", &root).unwrap();
        let get = |n: &str| c.iter().find(|c| c.name == n).unwrap();

        // A workspace package with no licence of its own inherits the root's.
        assert_eq!(get("@acme/console").licence.as_deref(), Some("Apache-2.0"));
        assert_eq!(get("@acme/kit").licence.as_deref(), Some("MIT"));
        assert_eq!(get("react").licence.as_deref(), Some("MIT"));
        assert_eq!(
            get("react-dom").licence.as_deref(),
            Some("MIT"),
            "object form"
        );
        assert_eq!(
            get("@scope/leaf").licence.as_deref(),
            Some("MIT OR Apache-2.0"),
            "the legacy array lists alternatives"
        );
        assert_eq!(
            get("react").sha512.as_deref(),
            Some("00".repeat(64).as_str())
        );
        assert!(
            get("@acme/kit").sha512.is_none(),
            "our own code has no registry hash"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_uninstalled_workspace_yields_no_licence_rather_than_a_guess() {
        let root = workspace("uninstalled", false);
        let lock = Lock::parse(&lock_text()).unwrap();
        let c = components(&lock, "apps/console", &root).unwrap();
        let react = c.iter().find(|c| c.name == "react").unwrap();
        assert_eq!(
            react.licence, None,
            "mg-scan refuses unknown; it must stay unknown"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn what_the_reader_cannot_follow_is_an_error() {
        let root = workspace("errors", true);
        let lock = Lock::parse(&lock_text()).unwrap();
        assert!(
            components(&lock, "apps/nowhere", &root).is_err(),
            "unknown importer"
        );

        for (tag, from, to) in [
            (
                "an alias",
                "version: 18.3.1(react@18.3.1)",
                "version: npm:other@1.0.0",
            ),
            (
                "a tarball",
                "version: 18.3.1(react@18.3.1)",
                "version: file:../x.tgz",
            ),
            ("a missing snapshot", "  react@18.3.1: {}\n", ""),
            (
                "a link to nowhere",
                "version: link:../../libs/kit",
                "version: link:../../libs/gone",
            ),
        ] {
            let text = lock_text().replace(from, to);
            let lock = Lock::parse(&text).expect("still parses");
            assert!(
                components(&lock, "apps/console", &root).is_err(),
                "{tag} must be refused"
            );
        }
        assert!(
            Lock::parse("lockfileVersion: '6.0'\n").is_err(),
            "another version"
        );
        assert!(Lock::parse("importers:\n").is_err(), "no version at all");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn package_names_cover_every_pinned_registry_package() {
        let lock = Lock::parse(&lock_text()).unwrap();
        let names: Vec<String> = lock.package_names().into_iter().collect();
        assert_eq!(names, ["@scope/leaf", "react", "react-dom", "vite"]);
    }

    #[test]
    fn small_helpers() {
        assert_eq!(
            split_spec("@scope/leaf@1.0.0"),
            Some(("@scope/leaf", "1.0.0"))
        );
        assert_eq!(split_spec("react@18.3.1"), Some(("react", "18.3.1")));
        assert_eq!(base_version("6.30.4(react@18.3.1)"), "6.30.4");
        assert_eq!(resolve_link("apps/console", "../../libs/kit"), "libs/kit");
        assert_eq!(
            key_value("'@scope/name': 1.0.0"),
            Some(("@scope/name", "1.0.0"))
        );
        assert_eq!(base64_decode("TWFu"), Some(b"Man".to_vec()));
        assert_eq!(base64_decode("TQ=="), Some(b"M".to_vec()));
        assert!(base64_decode("not base64!").is_none());
        assert!(sha512_hex("sha1-AAAA").is_none());
    }
}
