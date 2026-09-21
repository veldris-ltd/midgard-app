//! mg-scan — vulnerability and licence gate over a CycloneDX SBOM. [FR-MS-6]
//!
//! Runs between `sbom` and `sign` in the release gate, so an artefact that fails
//! the scan is never signed. Fails closed on:
//!
//!   * any CRITICAL, HIGH or **unscored** advisory affecting a component, with
//!     no unexpired exception;
//!   * any component whose licence is not satisfiable by the allow-list;
//!   * any component with no licence metadata at all;
//!   * an expired exception of either kind;
//!   * an advisory database that is absent, empty, or older than its own
//!     staleness limit.
//!
//! ```text
//! mg-scan <sbom.cdx.json> [--advisory-db <dir>] [--policy-dir <dir>]
//!         [--today YYYY-MM-DD] [--advisory-only] [--out findings.json]
//! ```
//!
//! `--advisory-only` reports without failing on anything below CRITICAL — the PR
//! mode, so exposure is discovered at review time rather than at release time.
//! It still fails on CRITICAL, because a critical advisory is not news anyone
//! should merge past.
//!
//! Exit codes: `0` clean, `2` usage error, `1` refused.
//!
//! ## Offline
//! Everything is local: the SBOM, `supply-chain/*.toml`, and the vendored
//! `vendor/advisory-db`. The scan never resolves a name over the network, which
//! is why the database's staleness is enforced rather than assumed.

mod advisory;
mod licence;
mod semver;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use advisory::Database;
use licence::Policy;
use semver::Version;

const USAGE: &str = "\
usage:
  mg-scan <sbom.cdx.json> [--advisory-db <dir>] [--policy-dir <dir>]
          [--today YYYY-MM-DD] [--advisory-only] [--out <findings.json>]

Refuses unless every component in the SBOM has an allowed licence and no
blocking advisory. Exceptions live in supply-chain/{licences,advisories}.toml
and must carry an owner and an expiry; an expired exception is a refusal.
";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(Failure::Usage(m)) => {
            eprintln!("mg-scan: {m}\n");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Refused(m)) => {
            eprintln!("mg-scan: REFUSED — {m}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
enum Failure {
    Usage(String),
    Refused(String),
}

/// One component read out of the SBOM.
#[derive(Debug, Clone)]
struct Component {
    name: String,
    version: String,
    purl: String,
    /// SPDX expression; `None` when the SBOM recorded no licence.
    licence: Option<String>,
    /// Whether this component was fetched from the registry (crates.io). Detected
    /// from the presence of an integrity `hashes` block, which the SBOM carries only
    /// for crates with a `Cargo.lock` checksum. A path/workspace member has none —
    /// and RustSec advisories, which are keyed to crates.io package names, do NOT
    /// apply to a local crate that merely shares a name. [RT-41] [FR-MS-6]
    from_registry: bool,
}

/// One reason a component was flagged.
#[derive(Debug)]
struct Finding {
    component: String,
    purl: String,
    kind: String,
    detail: String,
    blocking: bool,
}

fn run() -> Result<ExitCode, Failure> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }

    let mut sbom_path: Option<PathBuf> = None;
    let mut db_dir = PathBuf::from("vendor/advisory-db");
    let mut policy_dir = PathBuf::from("supply-chain");
    let mut today: Option<String> = None;
    let mut advisory_only = false;
    let mut out: Option<PathBuf> = None;

    let mut i = 0;
    while i < argv.len() {
        let need = |i: usize, what: &str| -> Result<String, Failure> {
            argv.get(i + 1)
                .filter(|v| !v.starts_with("--"))
                .cloned()
                .ok_or_else(|| Failure::Usage(format!("{what} requires a value")))
        };
        match argv[i].as_str() {
            "--advisory-db" => {
                db_dir = PathBuf::from(need(i, "--advisory-db")?);
                i += 2;
            }
            "--policy-dir" => {
                policy_dir = PathBuf::from(need(i, "--policy-dir")?);
                i += 2;
            }
            "--today" => {
                today = Some(need(i, "--today")?);
                i += 2;
            }
            "--out" => {
                out = Some(PathBuf::from(need(i, "--out")?));
                i += 2;
            }
            "--advisory-only" => {
                advisory_only = true;
                i += 1;
            }
            other if other.starts_with("--") => {
                return Err(Failure::Usage(format!("unknown flag {other}")))
            }
            other => {
                if sbom_path.is_some() {
                    return Err(Failure::Usage("takes exactly one SBOM path".into()));
                }
                sbom_path = Some(PathBuf::from(other));
                i += 1;
            }
        }
    }

    let sbom_path = sbom_path.ok_or_else(|| Failure::Usage("no SBOM given".into()))?;
    let today = today.unwrap_or_else(build_date);

    // --- inputs, each of which fails closed if unusable ---------------------
    let sbom = std::fs::read_to_string(&sbom_path)
        .map_err(|e| Failure::Refused(format!("cannot read SBOM {}: {e}", sbom_path.display())))?;
    let components =
        parse_sbom(&sbom).map_err(|e| Failure::Refused(format!("{}: {e}", sbom_path.display())))?;
    if components.is_empty() {
        return Err(Failure::Refused(
            "the SBOM lists no components — nothing was scanned".into(),
        ));
    }

    let policy_text = std::fs::read_to_string(policy_dir.join("licences.toml"))
        .map_err(|e| Failure::Refused(format!("cannot read licences.toml: {e}")))?;
    let policy =
        Policy::parse(&policy_text).map_err(|e| Failure::Refused(format!("licences.toml: {e}")))?;
    if policy.allowed_count() == 0 {
        // An empty allow-list would refuse everything, which looks like a
        // working gate right up until someone "fixes" it with a blanket
        // exception. Treat it as misconfiguration.
        return Err(Failure::Refused(
            "licences.toml allows nothing — that is a misconfiguration, not a policy".into(),
        ));
    }

    let advisory_exceptions =
        parse_advisory_exceptions(&policy_dir.join("advisories.toml")).map_err(Failure::Refused)?;

    let expected_advisories = check_db_freshness(&db_dir, &today).map_err(Failure::Refused)?;
    let db = Database::load(&db_dir).map_err(Failure::Refused)?;

    // The snapshot manifest records how many advisories it vendored. If fewer
    // load, something is dropping them — a parser that silently skips what it
    // cannot read is indistinguishable from a clean scan, and that is precisely
    // the failure this gate exists to prevent. (It happened once already: CVSS
    // 4.0 vectors were being discarded whole.)
    if db.count < expected_advisories {
        return Err(Failure::Refused(format!(
            "advisory database declares {expected_advisories} advisories but only {} loaded — \
             {} were silently dropped; the parser is at fault, not the data",
            db.count,
            expected_advisories - db.count
        )));
    }

    eprintln!(
        "mg-scan: {} component(s), {} advisories, {} allowed licence id(s)",
        components.len(),
        db.count,
        policy.allowed_count()
    );

    let mut findings: Vec<Finding> = Vec::new();

    // --- expired exceptions are themselves failures -------------------------
    for (crate_name, e) in policy.exceptions() {
        if expired(&e.expires, &today) {
            findings.push(Finding {
                component: crate_name.clone(),
                purl: String::new(),
                kind: "expired-licence-exception".into(),
                detail: format!(
                    "exception for {} expired {} (owner {})",
                    e.licence, e.expires, e.owner
                ),
                blocking: true,
            });
        }
    }
    for e in &advisory_exceptions {
        if expired(&e.expires, &today) {
            findings.push(Finding {
                component: e.krate.clone(),
                purl: String::new(),
                kind: "expired-advisory-exception".into(),
                detail: format!(
                    "exception for {} expired {} (owner {})",
                    e.id, e.expires, e.owner
                ),
                blocking: true,
            });
        }
    }

    // --- per component ------------------------------------------------------
    for c in &components {
        match &c.licence {
            None => findings.push(Finding {
                component: format!("{}@{}", c.name, c.version),
                purl: c.purl.clone(),
                kind: "licence-unknown".into(),
                detail: "no licence metadata; unknown is not permitted".into(),
                blocking: true,
            }),
            Some(expr) if !policy.allows(expr) => {
                let excused = policy
                    .exception(&c.name)
                    .filter(|e| e.licence == *expr && !expired(&e.expires, &today))
                    .is_some();
                if !excused {
                    findings.push(Finding {
                        component: format!("{}@{}", c.name, c.version),
                        purl: c.purl.clone(),
                        kind: "licence-disallowed".into(),
                        detail: format!("licence {expr:?} is not on the allow-list"),
                        blocking: true,
                    });
                }
            }
            Some(_) => {}
        }

        // RustSec advisories are keyed to crates.io package names. A path/workspace
        // member is our own code, not the registry crate of the same name, so it is
        // never matched against the advisory database — that would flag a name
        // collision (e.g. our `telemetry` vs the abandoned crates.io `telemetry`) as
        // a phantom vulnerability. Licence checks above still apply to every
        // component. [RT-41] [FR-MS-6]
        if !c.from_registry {
            continue;
        }

        let Some(version) = Version::parse(&c.version) else {
            // A version we cannot parse cannot be matched against advisory
            // ranges, so it cannot be cleared either.
            findings.push(Finding {
                component: format!("{}@{}", c.name, c.version),
                purl: c.purl.clone(),
                kind: "version-unparseable".into(),
                detail: "cannot compare this version against advisory ranges".into(),
                blocking: true,
            });
            continue;
        };

        for a in db.for_crate(&c.name) {
            if !a.affects(&version) {
                continue;
            }
            let excused = advisory_exceptions
                .iter()
                .any(|e| e.id == a.id && e.krate == c.name && !expired(&e.expires, &today));
            let informational = a.informational.is_some();
            let blocks = !informational && a.severity.blocks_release() && !excused;

            findings.push(Finding {
                component: format!("{}@{}", c.name, c.version),
                purl: c.purl.clone(),
                kind: if informational {
                    format!(
                        "advisory-informational-{}",
                        a.informational.as_deref().unwrap_or("")
                    )
                } else {
                    format!("advisory-{}", a.severity.as_str().to_lowercase())
                },
                detail: format!(
                    "{} {}{}",
                    a.id,
                    a.url.as_deref().unwrap_or("(no url)"),
                    if excused { " [exception recorded]" } else { "" }
                ),
                blocking: blocks,
            });
        }
    }

    findings.sort_by(|a, b| {
        b.blocking
            .cmp(&a.blocking)
            .then_with(|| a.component.cmp(&b.component))
            .then_with(|| a.kind.cmp(&b.kind))
    });

    for f in &findings {
        println!(
            "{}  {}  {}  {}",
            if f.blocking { "BLOCK" } else { "note " },
            f.component,
            f.kind,
            f.detail
        );
    }

    if let Some(path) = &out {
        write_findings(path, &findings).map_err(Failure::Refused)?;
        eprintln!("mg-scan: wrote {}", path.display());
    }

    // In advisory-only (PR) mode, only CRITICAL still blocks.
    let blocking: Vec<&Finding> = findings
        .iter()
        .filter(|f| {
            if advisory_only {
                f.kind == "advisory-critical"
            } else {
                f.blocking
            }
        })
        .collect();

    println!(
        "mg-scan: {} finding(s), {} blocking",
        findings.len(),
        blocking.len()
    );

    if blocking.is_empty() {
        println!("mg-scan: OK — no blocking advisory or licence violation");
        Ok(ExitCode::SUCCESS)
    } else {
        Err(Failure::Refused(format!(
            "{} blocking finding(s); see the BLOCK lines above",
            blocking.len()
        )))
    }
}

/// A recorded advisory exception.
struct AdvisoryException {
    id: String,
    krate: String,
    owner: String,
    expires: String,
}

fn parse_advisory_exceptions(path: &Path) -> Result<Vec<AdvisoryException>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let mut out = Vec::new();
    let mut current: Option<(String, std::collections::BTreeMap<String, String>)> = None;
    let finish = |cur: Option<(String, std::collections::BTreeMap<String, String>)>,
                  out: &mut Vec<AdvisoryException>|
     -> Result<(), String> {
        let Some((id, f)) = cur else { return Ok(()) };
        let need = |k: &str| -> Result<String, String> {
            f.get(k)
                .cloned()
                .ok_or_else(|| format!("advisory exception {id} has no `{k}`"))
        };
        // A compensating control is mandatory: accepting exposure without one is
        // not risk acceptance, it is risk ignorance.
        need("compensating_control")?;
        need("reason")?;
        let (krate, owner, expires) = (need("crate")?, need("owner")?, need("expires")?);
        out.push(AdvisoryException {
            id,
            krate,
            owner,
            expires,
        });
        Ok(())
    };

    for raw in text.lines() {
        let line = if raw.trim_start().starts_with('#') {
            ""
        } else {
            raw
        }
        .trim();
        if line.is_empty() {
            continue;
        }
        if let Some(h) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            finish(current.take(), &mut out)?;
            if let Some(id) = h.strip_prefix("advisories.") {
                current = Some((
                    id.trim_matches('"').to_string(),
                    std::collections::BTreeMap::new(),
                ));
            }
            continue;
        }
        if let (Some((k, v)), Some((_, fields))) = (line.split_once('='), current.as_mut()) {
            fields.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
        }
    }
    finish(current.take(), &mut out)?;
    Ok(out)
}

/// Refuse an advisory database that is missing, empty, or too old to be trusted.
/// Returns the advisory count the snapshot claims to contain.
fn check_db_freshness(db_dir: &Path, today: &str) -> Result<usize, String> {
    let manifest = db_dir.join("snapshot.toml");
    let text = std::fs::read_to_string(&manifest).map_err(|e| {
        format!(
            "cannot read {}: {e}\nrun `just advisory-db-refresh` to vendor the database",
            manifest.display()
        )
    })?;
    let field = |k: &str| -> Option<String> {
        text.lines().find_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix(k)?.trim_start().strip_prefix('=')?.trim();
            Some(rest.trim_matches('"').to_string())
        })
    };
    let date = field("date").ok_or("snapshot.toml records no date")?;
    let warn: i64 = field("warn_after_days")
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let fail: i64 = field("fail_after_days")
        .and_then(|v| v.parse().ok())
        .unwrap_or(180);

    let age = days_between(&date, today)
        .ok_or_else(|| format!("snapshot.toml date {date:?} is not YYYY-MM-DD"))?;

    if age > fail {
        return Err(format!(
            "advisory database is {age} days old (snapshot {date}, limit {fail}) — \
             a silently ancient database still reports clean; run `just advisory-db-refresh`"
        ));
    }
    if age > warn {
        eprintln!("mg-scan: WARNING — advisory database is {age} days old (snapshot {date})");
    }

    let count: usize = field("advisory_count")
        .and_then(|v| v.parse().ok())
        .ok_or("snapshot.toml records no advisory_count")?;
    if count == 0 {
        return Err(
            "snapshot.toml declares zero advisories — an empty database \
                    reports every artefact clean"
                .into(),
        );
    }
    Ok(count)
}

/// Days from `from` to `to`, both `YYYY-MM-DD`. Negative if `to` precedes `from`.
fn days_between(from: &str, to: &str) -> Option<i64> {
    Some(civil_days(to)? - civil_days(from)?)
}

/// Days since an arbitrary epoch, by Howard Hinnant's civil-from-days algorithm.
/// Self-contained because the workspace vendors no date crate.
fn civil_days(date: &str) -> Option<i64> {
    let mut p = date.trim().split('-');
    let y: i64 = p.next()?.parse().ok()?;
    let m: i64 = p.next()?.parse().ok()?;
    let d: i64 = p.next()?.parse().ok()?;
    if p.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn expired(expires: &str, today: &str) -> bool {
    match days_between(expires, today) {
        Some(d) => d > 0,
        // An unparseable expiry is treated as expired: a date nobody can read is
        // not a date anyone is honouring.
        None => true,
    }
}

/// The build date, from SOURCE_DATE_EPOCH when set so the gate is reproducible,
/// else the system clock.
fn build_date() -> String {
    if let Ok(v) = std::env::var("SOURCE_DATE_EPOCH") {
        if let Ok(secs) = v.parse::<i64>() {
            return date_from_unix(secs);
        }
    }
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => date_from_unix(d.as_secs() as i64),
        Err(_) => "1970-01-01".to_string(),
    }
}

fn date_from_unix(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Read components out of a CycloneDX document.
fn parse_sbom(text: &str) -> Result<Vec<Component>, String> {
    if !text.contains("\"bomFormat\"") || !text.contains("\"CycloneDX\"") {
        return Err("not a CycloneDX document".into());
    }
    let mut out = Vec::new();
    for chunk in text.split("\"type\": \"library\"").skip(1) {
        let chunk = chunk.split("}\n").next().unwrap_or(chunk);
        let field = |k: &str| -> Option<String> {
            let key = format!("\"{k}\": \"");
            let at = chunk.find(&key)? + key.len();
            let end = at + chunk[at..].find('"')?;
            Some(chunk[at..end].to_string())
        };
        let (Some(name), Some(version)) = (field("name"), field("version")) else {
            return Err("a component is missing name or version".into());
        };
        out.push(Component {
            purl: field("purl").unwrap_or_else(|| format!("pkg:cargo/{name}@{version}")),
            // An empty `licenses` array means "looked, found nothing"; the
            // expression form means it was resolved.
            licence: field("expression"),
            // A registry crate carries a Cargo.lock checksum, emitted as a `hashes`
            // block; a path/workspace member does not. [RT-41]
            from_registry: chunk.contains("\"hashes\""),
            name,
            version,
        });
    }
    Ok(out)
}

fn write_findings(path: &Path, findings: &[Finding]) -> Result<(), String> {
    let mut s = String::from("{\n  \"findings\": [\n");
    for (i, f) in findings.iter().enumerate() {
        s.push_str(&format!(
            "    {{ \"component\": \"{}\", \"purl\": \"{}\", \"kind\": \"{}\", \"detail\": \"{}\", \"blocking\": {} }}",
            esc(&f.component), esc(&f.purl), esc(&f.kind), esc(&f.detail), f.blocking
        ));
        if i + 1 < findings.len() {
            s.push(',');
        }
        s.push('\n');
    }
    s.push_str("  ]\n}\n");
    std::fs::write(path, s).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SBOM: &str = r#"{
  "bomFormat": "CycloneDX",
  "specVersion": "1.5",
  "components": [
    { "type": "library", "name": "aes-gcm", "version": "0.10.3", "purl": "pkg:cargo/aes-gcm@0.10.3", "licenses": [ { "expression": "Apache-2.0 OR MIT" } ] },
    { "type": "library", "name": "mystery", "version": "1.0.0", "purl": "pkg:cargo/mystery@1.0.0", "licenses": [] }
  ]
}"#;

    #[test]
    fn registry_vs_local_is_detected_from_the_integrity_hash() {
        // A component with a Cargo.lock checksum (hashes block) is a registry crate;
        // one without is a path/workspace member. Advisory matching is skipped for
        // the latter, so a name collision with a crates.io crate is not a phantom
        // vulnerability. [RT-41]
        let sbom = r#"{ "bomFormat": "CycloneDX", "specVersion": "1.5", "components": [
            { "type": "library", "name": "aes-gcm", "version": "0.10.3", "purl": "pkg:cargo/aes-gcm@0.10.3", "licenses": [], "hashes": [ { "alg": "SHA-256", "content": "ab" } ] },
            { "type": "library", "name": "telemetry", "version": "0.0.0", "purl": "pkg:cargo/telemetry@0.0.0", "licenses": [ { "expression": "Apache-2.0" } ] }
        ] }"#;
        let comps = parse_sbom(sbom).expect("parses");
        let registry = comps.iter().find(|c| c.name == "aes-gcm").unwrap();
        let local = comps.iter().find(|c| c.name == "telemetry").unwrap();
        assert!(
            registry.from_registry,
            "a crate with an integrity hash is from the registry"
        );
        assert!(
            !local.from_registry,
            "a workspace member (no hash) is not a registry crate"
        );
    }

    #[test]
    fn reads_components_and_licences() {
        let c = parse_sbom(SBOM).expect("parses");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].name, "aes-gcm");
        assert_eq!(c[0].licence.as_deref(), Some("Apache-2.0 OR MIT"));
        assert_eq!(
            c[1].licence, None,
            "an empty licences array is not a licence"
        );
    }

    #[test]
    fn refuses_a_non_cyclonedx_document() {
        assert!(parse_sbom(r#"{"hello":"world"}"#).is_err());
    }

    #[test]
    fn dates_and_ages_are_computed_without_a_date_crate() {
        assert_eq!(days_between("2026-01-01", "2026-01-31"), Some(30));
        assert_eq!(
            days_between("2026-02-28", "2026-03-01"),
            Some(1),
            "2026 is not a leap year"
        );
        assert_eq!(
            days_between("2024-02-28", "2024-03-01"),
            Some(2),
            "2024 is a leap year"
        );
        assert_eq!(days_between("2026-01-01", "2025-12-31"), Some(-1));
        assert_eq!(days_between("2026-12-31", "2027-01-01"), Some(1));
        assert!(days_between("not-a-date", "2026-01-01").is_none());
    }

    #[test]
    fn unix_seconds_round_trip_to_the_same_civil_date() {
        assert_eq!(date_from_unix(0), "1970-01-01");
        assert_eq!(date_from_unix(1_767_225_600), "2026-01-01");
        for date in ["1970-01-01", "2000-02-29", "2026-08-28", "2038-01-19"] {
            let days = civil_days(date).unwrap();
            assert_eq!(date_from_unix(days * 86_400), date);
        }
    }

    #[test]
    fn expiry_is_inclusive_of_the_expiry_day_and_fails_closed() {
        assert!(
            !expired("2026-12-31", "2026-12-31"),
            "valid through its expiry day"
        );
        assert!(!expired("2026-12-31", "2026-06-01"));
        assert!(expired("2026-01-01", "2026-01-02"));
        assert!(
            expired("whenever", "2026-01-01"),
            "an unreadable expiry is treated as expired"
        );
    }
}
