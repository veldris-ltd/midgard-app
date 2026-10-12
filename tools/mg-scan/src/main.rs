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
//!   * an advisory database that is absent, empty, unreadable in part, dated in
//!     the future, or older than its own staleness limit.
//!
//! ```text
//! mg-scan <sbom.cdx.json> [--advisory-db <dir>] [--policy-dir <dir>]
//!         [--today YYYY-MM-DD] [--advisory-only] [--out findings.json]
//! ```
//!
//! `--advisory-only` reports without failing on anything below CRITICAL — the PR
//! mode, so exposure is discovered at review time rather than at release time.
//! It still fails on CRITICAL, because a critical advisory is not news anyone
//! should merge past. It never calls a run clean that a release would refuse.
//!
//! Exit codes: `0` clean, `2` usage error, `1` refused.
//!
//! ## Offline
//! Everything is local: the SBOM, `supply-chain/*.toml`, and the vendored
//! `vendor/advisory-db`. The scan never resolves a name over the network, which
//! is why the database's staleness is enforced rather than assumed.
//!
//! ## The clock
//! Staleness and exception expiry are judged against the **system clock**.
//! `SOURCE_DATE_EPOCH` is deliberately not consulted: it pins the date *of a
//! build* so its output is reproducible, and reproducible-build shells export it
//! as a fixed instant years in the past. Read as "today", that would make every
//! database fresh and every exception unexpired. `--today` overrides the clock
//! for tests and says so on stderr; either way a snapshot dated after "today" is
//! refused, because a clock that runs behind the data is not one to judge age by.
//!
//! ## One reading of the SBOM
//! The SBOM is parsed with the same strict parser, and passes the same structural
//! gate, as `mg-sign` and `mg-verify` (`attest::sbom`). Every entry under
//! `components` is scanned whatever its `type`, nested components included, so
//! this tool and the verifier cannot disagree about what a signed bill of
//! materials contains.

mod advisory;
mod licence;
mod semver;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use advisory::Database;
use attest::json::{self, Json};
use licence::Policy;
use semver::Version;

const USAGE: &str = "\
usage:
  mg-scan <sbom.cdx.json> [--advisory-db <dir>] [--policy-dir <dir>]
          [--today YYYY-MM-DD] [--advisory-only] [--out <findings.json>]

Refuses unless every component in the SBOM has an allowed licence and no
blocking advisory. Exceptions live in supply-chain/{licences,advisories}.toml
and must carry an owner, a reason and an expiry; an expired exception is a
refusal.
";

/// The CycloneDX property `mg-sbom` records each component's origin under.
const SOURCE_PROPERTY: &str = "midgard:source";
/// Its value for a path/workspace member — our own code, not a registry crate.
const SOURCE_WORKSPACE: &str = "workspace";

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // Help is its own invocation. It exits 0, so it must never be reachable from
    // a command line that also names an SBOM: a caller that checks only the exit
    // code would read that as "scanned and clean".
    if matches!(argv.as_slice(), [only] if is_help(only)) {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match run(&argv) {
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

fn is_help(arg: &str) -> bool {
    arg == "-h" || arg == "--help"
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
    /// Whether the SBOM positively identifies this as a path/workspace member.
    ///
    /// RustSec advisories are keyed to crates.io package names and do NOT apply
    /// to a local crate that merely shares one, so a workspace member is not
    /// matched against the database. That exemption is granted only on a
    /// positive statement (`midgard:source = workspace`, which `mg-sbom` derives
    /// from `Cargo.lock`). A component that says nothing about its origin — a
    /// git dependency, an SBOM from another tool — is matched. [RT-41] [FR-MS-6]
    workspace_member: bool,
}

/// One reason a component was flagged.
#[derive(Debug)]
struct Finding {
    component: String,
    purl: String,
    kind: String,
    detail: String,
    /// Whether this finding refuses a release.
    blocking: bool,
}

impl Finding {
    /// Whether this finding fails the run in the given mode. A release is
    /// refused by every blocking finding; a PR (`--advisory-only`) only by an
    /// unexcused CRITICAL advisory.
    fn fails(&self, advisory_only: bool) -> bool {
        self.blocking && (!advisory_only || self.kind == "advisory-critical")
    }
}

fn run(argv: &[String]) -> Result<ExitCode, Failure> {
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
            help if is_help(help) => {
                return Err(Failure::Usage(format!(
                    "{help} must be the only argument — it does not scan anything"
                )))
            }
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
    let today = match today {
        Some(t) => {
            if civil_days(&t).is_none() {
                return Err(Failure::Usage(format!("--today {t:?} is not YYYY-MM-DD")));
            }
            eprintln!("mg-scan: WARNING — judging age and expiry against --today {t}, not the system clock");
            t
        }
        None => system_date(),
    };

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
        load_advisory_exceptions(&policy_dir.join("advisories.toml")).map_err(Failure::Refused)?;

    let expected_advisories = check_db_freshness(&db_dir, &today).map_err(Failure::Refused)?;
    let db = Database::load(&db_dir).map_err(Failure::Refused)?;

    // The snapshot manifest records how many advisories it vendored, and the
    // number loaded must be that number exactly. Fewer means something is
    // dropping them — a parser that silently skips what it cannot read is
    // indistinguishable from a clean scan (it happened once already: CVSS 4.0
    // vectors were being discarded whole). More means the directory is not the
    // snapshot the manifest describes, and a surplus could mask the same number
    // dropped.
    if db.count != expected_advisories {
        return Err(Failure::Refused(format!(
            "advisory database declares {expected_advisories} advisories but {} loaded — \
             the directory is not the snapshot its manifest describes",
            db.count
        )));
    }

    let members = components.iter().filter(|c| c.workspace_member).count();
    eprintln!(
        "mg-scan: {} component(s) — {} matched against {} advisories, {} workspace member(s) \
         exempt from advisory matching; {} allowed licence id(s)",
        components.len(),
        components.len() - members,
        db.count,
        members,
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

        // A workspace member is our own code, not the registry crate of the same
        // name, so it is not matched against the advisory database — that would
        // flag a name collision (e.g. our `telemetry` vs the abandoned crates.io
        // `telemetry`) as a phantom vulnerability. Licence checks above still
        // apply to every component. [RT-41] [FR-MS-6]
        if c.workspace_member {
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
            .then_with(|| a.detail.cmp(&b.detail))
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

    let release_blocking = findings.iter().filter(|f| f.blocking).count();
    let failing = findings.iter().filter(|f| f.fails(advisory_only)).count();

    println!(
        "mg-scan: {} finding(s), {release_blocking} blocking a release",
        findings.len()
    );

    if failing > 0 {
        return Err(Failure::Refused(format!(
            "{failing} blocking finding(s); see the BLOCK lines above"
        )));
    }
    if release_blocking > 0 {
        // Only reachable in --advisory-only mode. The run passes, and says
        // exactly what it is passing over.
        println!(
            "mg-scan: PASSED (advisory-only) — nothing CRITICAL, but {release_blocking} \
             finding(s) above WOULD BLOCK A RELEASE"
        );
    } else {
        println!("mg-scan: OK — no blocking advisory or licence violation");
    }
    Ok(ExitCode::SUCCESS)
}

/// A recorded advisory exception.
#[derive(Debug)]
struct AdvisoryException {
    id: String,
    krate: String,
    owner: String,
    expires: String,
}

fn load_advisory_exceptions(path: &Path) -> Result<Vec<AdvisoryException>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse_advisory_exceptions(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Parse `supply-chain/advisories.toml`.
///
/// Every field the format demands must be present **and say something**: an
/// exception whose owner or reason is the empty string is not a decision anyone
/// made, and a compensating control with nothing to follow is a claim nobody can
/// check.
fn parse_advisory_exceptions(text: &str) -> Result<Vec<AdvisoryException>, String> {
    type Fields = std::collections::BTreeMap<String, String>;

    let finish =
        |cur: Option<(String, Fields)>, out: &mut Vec<AdvisoryException>| -> Result<(), String> {
            let Some((id, f)) = cur else { return Ok(()) };
            let need = |k: &str| -> Result<String, String> {
                match f.get(k).map(|v| v.trim()) {
                    None => Err(format!("advisory exception {id} has no `{k}`")),
                    Some("") => Err(format!("advisory exception {id} has an empty `{k}`")),
                    Some(v) => Ok(v.to_string()),
                }
            };
            need("reason")?;
            // A compensating control is mandatory: accepting exposure without one is
            // not risk acceptance, it is risk ignorance. It must point at something.
            let control = need("compensating_control")?;
            if !has_link(&control) {
                return Err(format!(
                    "advisory exception {id}: `compensating_control` must link to the control \
                 (a URL, or a path to a document in this repository)"
                ));
            }
            let (krate, owner, expires) = (need("crate")?, need("owner")?, need("expires")?);
            if civil_days(&expires).is_none() {
                return Err(format!(
                    "advisory exception {id}: `expires` {expires:?} is not YYYY-MM-DD"
                ));
            }
            out.push(AdvisoryException {
                id,
                krate,
                owner,
                expires,
            });
            Ok(())
        };

    let mut out = Vec::new();
    let mut current: Option<(String, Fields)> = None;
    let mut lines = text.lines();
    while let Some(raw) = lines.next() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(h) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            finish(current.take(), &mut out)?;
            if let Some(id) = h.strip_prefix("advisories.") {
                current = Some((id.trim_matches('"').to_string(), Fields::new()));
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("expected `key = value`, got {line:?}"));
        };
        let Some((_, fields)) = current.as_mut() else {
            return Err(format!(
                "`{}` is outside any [advisories.\"<id>\"] table",
                key.trim()
            ));
        };
        // A basic string may continue over lines with a trailing backslash, as
        // the format's own example does.
        let mut value = value.trim().to_string();
        while value.ends_with('\\') {
            value.pop();
            match lines.next() {
                Some(next) => value.push_str(next.trim()),
                None => return Err(format!("`{}` ends in a dangling backslash", key.trim())),
            }
        }
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .ok_or_else(|| format!("`{}` must be a quoted string", key.trim()))?;
        fields.insert(key.trim().to_string(), value.to_string());
    }
    finish(current.take(), &mut out)?;
    Ok(out)
}

/// Whether a compensating control points at something a reader can follow: a
/// URL, or a path to a file (`docs/…/runbook.md`).
fn has_link(text: &str) -> bool {
    text.split_whitespace()
        .map(|t| t.trim_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | '(' | ')' | '`')))
        .any(|t| {
            let url = ["https://", "http://"]
                .iter()
                .any(|p| t.strip_prefix(p).is_some_and(|rest| rest.contains('.')));
            let path = t.contains('/')
                && t.rsplit('/')
                    .next()
                    .is_some_and(|file| file.contains('.') && !file.starts_with('.'));
            url || path
        })
}

/// Refuse an advisory database that is missing, empty, or whose age cannot be
/// trusted. Returns the advisory count the snapshot claims to contain.
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
    // A limit that is written down but unreadable is an error, not an invitation
    // to substitute a default the author did not choose.
    let days = |k: &str, default: i64| -> Result<i64, String> {
        match field(k) {
            None => Ok(default),
            Some(v) => v.parse::<i64>().ok().filter(|n| *n > 0).ok_or_else(|| {
                format!("snapshot.toml: {k} = {v:?} is not a positive number of days")
            }),
        }
    };
    let date = field("date").ok_or("snapshot.toml records no date")?;
    let warn = days("warn_after_days", 30)?;
    let fail = days("fail_after_days", 180)?;
    if fail < warn {
        return Err(format!(
            "snapshot.toml: fail_after_days ({fail}) is below warn_after_days ({warn})"
        ));
    }

    let age = days_between(&date, today)
        .ok_or_else(|| format!("snapshot.toml date {date:?} is not YYYY-MM-DD"))?;

    if age < 0 {
        return Err(format!(
            "advisory database snapshot is dated {date}, after today ({today}) — the clock is \
             wrong, so neither the database's age nor any exception's expiry can be judged"
        ));
    }
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

/// Today's date (UTC) from the system clock. See "The clock" in the module docs
/// for why `SOURCE_DATE_EPOCH` is not consulted.
fn system_date() -> String {
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

/// Read every component out of a CycloneDX document.
///
/// The document first passes `attest::sbom::validate` — the gate `mg-sign` and
/// `mg-verify` apply — and is then walked as parsed JSON, so a component is
/// scanned however the file happens to be formatted and whatever its `type`.
fn parse_sbom(text: &str) -> Result<Vec<Component>, String> {
    attest::sbom::validate(text.as_bytes())?;
    let doc = json::parse(text).map_err(|e| format!("not valid JSON: {e}"))?;
    let mut out = Vec::new();
    collect_components(&doc, &mut out)?;
    Ok(out)
}

/// Append the components listed under `parent`, and theirs, recursively.
fn collect_components(parent: &Json, out: &mut Vec<Component>) -> Result<(), String> {
    let Some(list) = parent.get("components") else {
        return Ok(());
    };
    let list = list.as_array().ok_or("`components` is not an array")?;
    for c in list {
        let field = |k: &str| -> Result<String, String> {
            c.get(k)
                .and_then(Json::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .ok_or_else(|| format!("a component has no `{k}`"))
        };
        let name = field("name")?;
        let version = field("version").map_err(|_| format!("component {name} has no `version`"))?;
        let purl = field("purl").map_err(|_| format!("component {name} has no `purl`"))?;
        out.push(Component {
            licence: licence_of(c).map_err(|e| format!("component {name}: {e}"))?,
            workspace_member: source_of(c) == Some(SOURCE_WORKSPACE),
            name,
            version,
            purl,
        });
        collect_components(c, out)?;
    }
    Ok(())
}

/// A component's licence as one SPDX expression, or `None` if it records none.
///
/// CycloneDX allows several entries; every one of them is a term the component
/// is under, so they are joined with `AND` — an entry this gate would refuse
/// cannot hide behind one it accepts.
fn licence_of(component: &Json) -> Result<Option<String>, String> {
    let Some(licenses) = component.get("licenses") else {
        return Ok(None);
    };
    let entries = licenses.as_array().ok_or("`licenses` is not an array")?;
    let mut terms: Vec<&str> = Vec::new();
    for e in entries {
        let term = e
            .get("expression")
            .and_then(Json::as_str)
            .or_else(|| {
                let l = e.get("license")?;
                l.get("id").or_else(|| l.get("name")).and_then(Json::as_str)
            })
            .ok_or("a `licenses` entry has neither an expression nor a licence id")?;
        terms.push(term);
    }
    Ok(match terms.as_slice() {
        [] => None,
        [one] => Some(one.to_string()),
        many => Some(
            many.iter()
                .map(|t| format!("({t})"))
                .collect::<Vec<_>>()
                .join(" AND "),
        ),
    })
}

/// The origin `mg-sbom` recorded for a component, if any.
fn source_of(component: &Json) -> Option<&str> {
    component
        .get("properties")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(Json::as_str) == Some(SOURCE_PROPERTY))?
        .get("value")?
        .as_str()
}

fn write_findings(path: &Path, findings: &[Finding]) -> Result<(), String> {
    std::fs::write(path, render_findings(findings))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn render_findings(findings: &[Finding]) -> String {
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
    s
}

/// JSON string escaping: quotes, backslashes and every control character.
fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
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
    fn only_a_positive_statement_exempts_a_component_from_advisory_matching() {
        // A workspace member shares nothing with the crates.io crate of the same
        // name, so it is exempt — but only when the SBOM says it is one. A
        // component that says nothing about its origin is matched. [RT-41]
        let sbom = r#"{ "bomFormat": "CycloneDX", "specVersion": "1.5", "components": [
            { "type": "library", "name": "telemetry", "version": "0.0.0", "purl": "pkg:cargo/telemetry@0.0.0",
              "properties": [ { "name": "midgard:source", "value": "workspace" } ] },
            { "type": "library", "name": "aes-gcm", "version": "0.10.3", "purl": "pkg:cargo/aes-gcm@0.10.3",
              "properties": [ { "name": "midgard:source", "value": "registry" } ] },
            { "type": "library", "name": "from-git", "version": "1.0.0", "purl": "pkg:cargo/from-git@1.0.0",
              "properties": [ { "name": "midgard:source", "value": "git" } ] },
            { "type": "library", "name": "says-nothing", "version": "1.0.0", "purl": "pkg:cargo/says-nothing@1.0.0" }
        ] }"#;
        let comps = parse_sbom(sbom).expect("parses");
        let exempt: Vec<&str> = comps
            .iter()
            .filter(|c| c.workspace_member)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(exempt, ["telemetry"]);
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
    fn every_component_is_read_whatever_its_type_or_formatting() {
        // Compact, no `type`, a non-library type, and a nested component: a
        // reader keyed on the text `"type": "library"` sees none of these.
        let sbom = concat!(
            r#"{"bomFormat":"CycloneDX","specVersion":"1.5","components":["#,
            r#"{"type":"library","name":"compact","version":"1.0.0","purl":"pkg:cargo/compact@1.0.0"},"#,
            r#"{"name":"untyped","version":"1.0.0","purl":"pkg:cargo/untyped@1.0.0"},"#,
            r#"{"type":"framework","name":"framework","version":"1.0.0","purl":"pkg:cargo/framework@1.0.0","#,
            r#""components":[{"type":"library","name":"nested","version":"2.0.0","purl":"pkg:cargo/nested@2.0.0"}]}"#,
            r#"]}"#
        );
        let names: Vec<String> = parse_sbom(sbom)
            .expect("parses")
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["compact", "untyped", "framework", "nested"]);
    }

    #[test]
    fn every_licence_entry_of_a_component_counts() {
        let sbom = r#"{ "bomFormat": "CycloneDX", "specVersion": "1.5", "components": [
            { "name": "two", "version": "1.0.0", "purl": "pkg:cargo/two@1.0.0",
              "licenses": [ { "expression": "MIT OR Apache-2.0" }, { "license": { "id": "GPL-3.0" } } ] },
            { "name": "by-id", "version": "1.0.0", "purl": "pkg:cargo/by-id@1.0.0",
              "licenses": [ { "license": { "id": "MIT" } } ] }
        ] }"#;
        let c = parse_sbom(sbom).expect("parses");
        assert_eq!(
            c[0].licence.as_deref(),
            Some("(MIT OR Apache-2.0) AND (GPL-3.0)"),
            "a second entry cannot hide behind the first"
        );
        assert_eq!(c[1].licence.as_deref(), Some("MIT"));
    }

    #[test]
    fn refuses_what_the_signer_and_verifier_refuse() {
        for (tag, doc) in [
            ("not cyclonedx", r#"{"hello":"world"}"#.to_string()),
            ("truncated", SBOM[..SBOM.len() - 1].to_string()),
            ("another version", SBOM.replace("\"1.5\"", "\"1.4\"")),
            (
                "a component with no purl",
                r#"{"bomFormat":"CycloneDX","specVersion":"1.5","components":[{"name":"a","version":"1"}]}"#
                    .to_string(),
            ),
            (
                "a component with no version",
                r#"{"bomFormat":"CycloneDX","specVersion":"1.5","components":[{"name":"a","purl":"pkg:cargo/a@1"}]}"#
                    .to_string(),
            ),
        ] {
            assert!(parse_sbom(&doc).is_err(), "{tag} must be refused");
        }
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

    // --- advisory exceptions ---

    const EXCEPTION: &str = r#"
# Accepted exposure.
[advisories]

[advisories."RUSTSEC-2099-0001"]
crate = "example"
owner = "a.name"
reason = "The vulnerable parser is only reachable from the CLI path, which \
          the enclosure never invokes."
compensating_control = "Egress broker denies the path; see docs/dev/operations/runbooks/example.md"
expires = "2026-12-31"
"#;

    #[test]
    fn a_complete_advisory_exception_is_read() {
        let e = parse_advisory_exceptions(EXCEPTION).expect("parses");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].id, "RUSTSEC-2099-0001");
        assert_eq!(e[0].krate, "example");
        assert_eq!(e[0].owner, "a.name");
        assert_eq!(e[0].expires, "2026-12-31");
        assert!(parse_advisory_exceptions("[advisories]\n# (empty)\n")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn an_advisory_exception_must_say_something_in_every_field() {
        for (tag, broken) in [
            ("no owner", EXCEPTION.replace("owner = \"a.name\"\n", "")),
            ("empty owner", EXCEPTION.replace("\"a.name\"", "\"\"")),
            ("blank owner", EXCEPTION.replace("\"a.name\"", "\"  \"")),
            ("empty crate", EXCEPTION.replace("\"example\"", "\"\"")),
            (
                "no compensating control",
                EXCEPTION
                    .lines()
                    .filter(|l| !l.starts_with("compensating_control"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            (
                "a control that links to nothing",
                EXCEPTION.replace(
                    "Egress broker denies the path; see docs/dev/operations/runbooks/example.md",
                    "Trust us, it is handled.",
                ),
            ),
            (
                "an unreadable expiry",
                EXCEPTION.replace("2026-12-31", "next year"),
            ),
            (
                "an unquoted value",
                EXCEPTION.replace("\"a.name\"", "a.name"),
            ),
        ] {
            assert!(
                parse_advisory_exceptions(&broken).is_err(),
                "{tag} must be refused"
            );
        }
    }

    #[test]
    fn a_compensating_control_must_point_somewhere() {
        for yes in [
            "see https://example.org/runbooks/egress",
            "Egress broker denies it; see docs/dev/operations/runbooks/example.md.",
            "(docs/ops/PKI.md)",
        ] {
            assert!(has_link(yes), "{yes:?} links to something");
        }
        for no in [
            "",
            "it is fine",
            "handled and/or mitigated",
            "https://",
            "see the runbook.",
        ] {
            assert!(!has_link(no), "{no:?} links to nothing");
        }
    }

    // --- modes and output ---

    fn finding(kind: &str, blocking: bool) -> Finding {
        Finding {
            component: "c@1".into(),
            purl: String::new(),
            kind: kind.into(),
            detail: String::new(),
            blocking,
        }
    }

    #[test]
    fn pr_mode_fails_only_on_an_unexcused_critical() {
        // Release mode: every blocking finding fails.
        assert!(finding("advisory-high", true).fails(false));
        assert!(finding("licence-unknown", true).fails(false));
        assert!(!finding("advisory-medium", false).fails(false));

        // PR mode: only CRITICAL — and not one a recorded exception covers,
        // which release mode lets through too.
        assert!(finding("advisory-critical", true).fails(true));
        assert!(!finding("advisory-critical", false).fails(true));
        assert!(!finding("advisory-high", true).fails(true));
        assert!(!finding("licence-disallowed", true).fails(true));
    }

    #[test]
    fn the_findings_report_is_valid_json_whatever_it_contains() {
        let text = render_findings(&[Finding {
            component: "odd\"name@1".into(),
            purl: "pkg:cargo/odd".into(),
            kind: "advisory-high".into(),
            detail: "line one\nline two\ttabbed \u{1} back\\slash".into(),
            blocking: true,
        }]);
        let doc = json::parse(text.trim()).expect("valid JSON");
        let row = &doc.get("findings").and_then(Json::as_array).unwrap()[0];
        assert_eq!(
            row.get("detail").and_then(Json::as_str),
            Some("line one\nline two\ttabbed \u{1} back\\slash")
        );
        assert_eq!(
            row.get("component").and_then(Json::as_str),
            Some("odd\"name@1")
        );
        assert_eq!(row.get("blocking").and_then(Json::as_bool), Some(true));
    }
}
