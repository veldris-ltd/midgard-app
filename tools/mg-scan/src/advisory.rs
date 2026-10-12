//! Loading the vendored RustSec advisory database.
//!
//! Each advisory is a markdown file with a TOML frontmatter block fenced by
//! ```` ```toml ````. Only the frontmatter is parsed; the prose below it is for
//! humans.
//!
//! ## Severity
//! RustSec records a CVSS v3 vector, not a severity word, and 417 of 1206
//! advisories carry no vector at all. Severity is therefore computed from the
//! base score where a vector exists, and an advisory *without* one is treated as
//! [`Severity::Unknown`] — which this gate escalates rather than ignores. An
//! advisory nobody scored is not an advisory nobody needs to read.

use std::collections::BTreeMap;
use std::path::Path;

use crate::semver::{Req, Version};

/// How serious an advisory is, from its CVSS base score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    None,
    Low,
    Medium,
    High,
    Critical,
    /// No CVSS vector was recorded. Ranked above Critical for triage ordering:
    /// it demands a human decision rather than an automatic pass.
    Unknown,
}

impl Severity {
    /// The word used in reports and in exception matching.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::None => "NONE",
            Severity::Low => "LOW",
            Severity::Medium => "MEDIUM",
            Severity::High => "HIGH",
            Severity::Critical => "CRITICAL",
            Severity::Unknown => "UNSCORED",
        }
    }

    /// Whether this severity blocks a release absent an exception.
    pub fn blocks_release(self) -> bool {
        matches!(
            self,
            Severity::High | Severity::Critical | Severity::Unknown
        )
    }

    fn from_score(score: f64) -> Self {
        match score {
            s if s >= 9.0 => Severity::Critical,
            s if s >= 7.0 => Severity::High,
            s if s >= 4.0 => Severity::Medium,
            s if s > 0.0 => Severity::Low,
            _ => Severity::None,
        }
    }
}

/// One parsed advisory.
#[derive(Debug, Clone)]
pub struct Advisory {
    pub id: String,
    pub package: String,
    pub severity: Severity,
    /// Present only for real vulnerabilities; `Some` marks an *informational*
    /// entry (unmaintained, unsound, notice) which is reported but never blocks.
    pub informational: Option<String>,
    pub withdrawn: bool,
    pub url: Option<String>,
    /// Version requirements that are fixed.
    patched: Vec<String>,
    /// Version requirements never affected in the first place.
    unaffected: Vec<String>,
}

impl Advisory {
    /// Whether `version` is exposed to this advisory.
    ///
    /// Returns `true` when a requirement cannot be parsed: an advisory whose
    /// ranges we cannot read must surface for a human, not vanish.
    pub fn affects(&self, version: &Version) -> bool {
        if self.withdrawn {
            return false;
        }
        for r in self.unaffected.iter().chain(self.patched.iter()) {
            match Req::parse(r) {
                Some(req) if req.matches(version) => return false,
                Some(_) => {}
                None => return true, // unreadable range -> assume exposed
            }
        }
        true
    }
}

/// The vendored database, indexed by crate name.
#[derive(Debug, Default)]
pub struct Database {
    by_crate: BTreeMap<String, Vec<Advisory>>,
    pub count: usize,
}

impl Database {
    /// Advisories recorded for a crate.
    pub fn for_crate(&self, name: &str) -> &[Advisory] {
        self.by_crate.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Load every advisory under `<root>/crates`.
    pub fn load(root: &Path) -> Result<Self, String> {
        let crates = root.join("crates");
        let dirs = std::fs::read_dir(&crates).map_err(|e| {
            format!(
                "cannot read advisory database at {}: {e}\n\
                 run `just advisory-db-refresh` to vendor it",
                crates.display()
            )
        })?;

        let mut db = Database::default();
        for dir in dirs {
            let dir = dir.map_err(|e| format!("cannot read advisory dir: {e}"))?;
            if !dir.path().is_dir() {
                continue;
            }
            let files =
                std::fs::read_dir(dir.path()).map_err(|e| format!("cannot read entry: {e}"))?;
            for f in files {
                let path = f.map_err(|e| format!("cannot read entry: {e}"))?.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                // An advisory this build cannot read is a refusal, not a skip:
                // a database that loads all but one file reports clean about
                // exactly the crate that file covers.
                let a = parse_advisory(&text)
                    .ok_or_else(|| format!("cannot parse advisory {}", path.display()))?;
                db.count += 1;
                db.by_crate.entry(a.package.clone()).or_default().push(a);
            }
        }
        Ok(db)
    }
}

/// Pull one scalar out of a TOML frontmatter line, e.g. `id = "RUSTSEC-..."`.
fn scalar(block: &str, key: &str) -> Option<String> {
    block.lines().find_map(|l| {
        let l = l.trim();
        let rest = l.strip_prefix(key)?.trim_start();
        let rest = rest.strip_prefix('=')?.trim();
        Some(rest.trim_matches('"').to_string())
    })
}

/// Pull a string array, e.g. `patched = [">= 0.9.1"]`.
///
/// Each **quoted string** is one element. A comma inside the quotes belongs to
/// the element: `">= 0.8.26, < 1.0.0"` is one requirement with two comparators,
/// both of which must hold. Splitting the array on every comma instead turns
/// that into two independent ranges, and `< 1.0.0` on its own then clears every
/// vulnerable version below the fix.
fn array(block: &str, key: &str) -> Vec<String> {
    // The key must start a line, so `patched` cannot match inside another key.
    let mut offset = 0;
    let start = block.split_inclusive('\n').find_map(|line| {
        let here = offset;
        offset += line.len();
        let rest = line.trim_start().strip_prefix(key)?.trim_start();
        rest.strip_prefix('=')?
            .trim_start()
            .starts_with('[')
            .then_some(here)
    });
    let Some(start) = start else {
        return Vec::new();
    };
    let Some(open) = block[start..].find('[') else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for c in block[start + open + 1..].chars() {
        match (&mut current, c) {
            (Some(_), '"') => out.extend(current.take()),
            (Some(s), c) => s.push(c),
            (None, '"') => current = Some(String::new()),
            (None, ']') => break,
            (None, _) => {}
        }
    }
    out.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse the TOML frontmatter of one advisory file.
pub fn parse_advisory(text: &str) -> Option<Advisory> {
    // Frontmatter is the first ```toml fenced block.
    let start = text.find("```toml")? + "```toml".len();
    let end = start + text[start..].find("```")?;
    let block = &text[start..end];

    let id = scalar(block, "id")?;
    let package = scalar(block, "package")?;
    // A vector this build cannot score must NOT drop the advisory — that would
    // silently shrink the database. CVSS 4.0 uses a macrovector lookup this
    // scorer does not implement, so those advisories become `Unknown`, which
    // blocks and forces a human to look. Losing them entirely is the one
    // outcome that must not happen.
    let severity = match scalar(block, "cvss") {
        Some(vector) => cvss_v3_base_score(&vector)
            .map(Severity::from_score)
            .unwrap_or(Severity::Unknown),
        None => Severity::Unknown,
    };

    Some(Advisory {
        id,
        package,
        severity,
        informational: scalar(block, "informational"),
        withdrawn: scalar(block, "withdrawn").is_some(),
        url: scalar(block, "url"),
        patched: array(block, "patched"),
        unaffected: array(block, "unaffected"),
    })
}

/// Compute a CVSS v3.x base score from its vector string.
///
/// Implemented from the CVSS v3.1 specification rather than pulled from a table,
/// because the score decides whether a release is blocked and a wrong constant
/// would silently mis-rank real advisories.
pub fn cvss_v3_base_score(vector: &str) -> Option<f64> {
    let mut m: BTreeMap<&str, &str> = BTreeMap::new();
    for part in vector.split('/') {
        if let Some((k, v)) = part.split_once(':') {
            m.insert(k, v);
        }
    }
    // Reject anything that is not a v3 vector rather than scoring it wrongly.
    match m.get("CVSS") {
        Some(v) if v.starts_with("3.") => {}
        _ => return None,
    }

    let av = match *m.get("AV")? {
        "N" => 0.85,
        "A" => 0.62,
        "L" => 0.55,
        "P" => 0.2,
        _ => return None,
    };
    let ac = match *m.get("AC")? {
        "L" => 0.77,
        "H" => 0.44,
        _ => return None,
    };
    let ui = match *m.get("UI")? {
        "N" => 0.85,
        "R" => 0.62,
        _ => return None,
    };
    let scope_changed = match *m.get("S")? {
        "U" => false,
        "C" => true,
        _ => return None,
    };
    // Privileges Required is scope-dependent.
    let pr = match (*m.get("PR")?, scope_changed) {
        ("N", _) => 0.85,
        ("L", false) => 0.62,
        ("L", true) => 0.68,
        ("H", false) => 0.27,
        ("H", true) => 0.5,
        _ => return None,
    };
    let cia = |k: &str| -> Option<f64> {
        Some(match *m.get(k)? {
            "H" => 0.56,
            "L" => 0.22,
            "N" => 0.0,
            _ => return None,
        })
    };
    let (c, i, a) = (cia("C")?, cia("I")?, cia("A")?);

    let iss = 1.0 - ((1.0 - c) * (1.0 - i) * (1.0 - a));
    let impact = if scope_changed {
        7.52 * (iss - 0.029) - 3.25 * (iss - 0.02).powi(15)
    } else {
        6.42 * iss
    };
    if impact <= 0.0 {
        return Some(0.0);
    }
    let exploitability = 8.22 * av * ac * pr * ui;
    let raw = if scope_changed {
        (1.08 * (impact + exploitability)).min(10.0)
    } else {
        (impact + exploitability).min(10.0)
    };
    Some(roundup(raw))
}

/// CVSS v3.1 `Roundup` (specification, Appendix A): the smallest number with one
/// decimal place that is not less than the input.
///
/// Done in integers, as the specification prescribes, because a plain
/// `ceil(x * 10) / 10` rounds a value that floating point stores as
/// `4.000000000000001` up to 4.1 — one notch high, and across a band edge that
/// is a different severity.
fn roundup(x: f64) -> f64 {
    let scaled = (x * 100_000.0).round() as i64;
    if scaled % 10_000 == 0 {
        scaled as f64 / 100_000.0
    } else {
        ((scaled / 10_000) as f64 + 1.0) / 10.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"```toml
[advisory]
id = "RUSTSEC-2020-0105"
package = "abi_stable"
cvss = "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H"
date = "2020-12-21"
url = "https://example.invalid/44"

[versions]
patched = [">= 0.9.1"]
```

# Prose below the frontmatter is ignored.
"#;

    #[test]
    fn parses_frontmatter() {
        let a = parse_advisory(SAMPLE).expect("parses");
        assert_eq!(a.id, "RUSTSEC-2020-0105");
        assert_eq!(a.package, "abi_stable");
        assert_eq!(a.severity, Severity::High); // 7.5
        assert!(a.informational.is_none());
        assert!(!a.withdrawn);
    }

    #[test]
    fn version_ranges_decide_exposure() {
        let a = parse_advisory(SAMPLE).unwrap();
        assert!(
            a.affects(&Version::parse("0.9.0").unwrap()),
            "before the fix"
        );
        assert!(!a.affects(&Version::parse("0.9.1").unwrap()), "the fix");
        assert!(
            !a.affects(&Version::parse("1.0.0").unwrap()),
            "after the fix"
        );
    }

    #[test]
    fn a_compound_range_is_one_requirement_not_two() {
        // RUSTSEC-2025-0073's shape: a backported fix on the 0.8 line and a fix
        // on the 1.x line. Everything below 0.8.26, and 1.0.0 up to 1.4.1, is
        // vulnerable. Split on the comma, `< 1.0.0` alone would clear 0.8.0.
        let text = SAMPLE.replace(
            "patched = [\">= 0.9.1\"]",
            "patched = [\">=0.8.26, <1.0.0\", \">=1.4.1\"]",
        );
        let a = parse_advisory(&text).unwrap();
        assert_eq!(a.patched, [">=0.8.26, <1.0.0", ">=1.4.1"]);

        for vulnerable in ["0.1.0", "0.8.0", "0.8.25", "1.0.0", "1.2.0", "1.4.0"] {
            assert!(
                a.affects(&Version::parse(vulnerable).unwrap()),
                "{vulnerable} is vulnerable"
            );
        }
        for fixed in ["0.8.26", "0.9.0", "1.4.1", "2.0.0"] {
            assert!(
                !a.affects(&Version::parse(fixed).unwrap()),
                "{fixed} carries the fix"
            );
        }
    }

    #[test]
    fn arrays_are_read_by_element_across_lines_and_keys() {
        let block = "unaffected = [\"< 0.5.0\"]\npatched = [\n  \">= 1.2.3, < 2.0.0\",\n  \">= 2.1.0\",\n]\n";
        assert_eq!(array(block, "patched"), [">= 1.2.3, < 2.0.0", ">= 2.1.0"]);
        assert_eq!(array(block, "unaffected"), ["< 0.5.0"]);
        assert!(array(block, "missing").is_empty());
        assert!(array("patched = []\n", "patched").is_empty());
        // `patched` must not be found inside a longer key.
        assert!(array("unpatched = [\"x\"]\n", "patched").is_empty());
    }

    #[test]
    fn roundup_follows_the_specification_not_floating_point() {
        assert_eq!(roundup(4.02), 4.1);
        assert_eq!(roundup(4.0), 4.0);
        // Stored a hair above 4.0; a plain ceil would say 4.1.
        assert_eq!(roundup(4.000_000_000_000_001), 4.0);
        assert_eq!(roundup(6.9), 6.9);
        assert_eq!(roundup(8.999_999), 9.0);
        assert_eq!(roundup(0.0), 0.0);
    }

    #[test]
    fn an_unreadable_range_is_treated_as_exposed() {
        // A range the comparator cannot parse must surface, not vanish.
        let text = SAMPLE.replace(">= 0.9.1", "~> 0.9.1");
        let a = parse_advisory(&text).unwrap();
        assert!(a.affects(&Version::parse("99.0.0").unwrap()));
    }

    #[test]
    fn a_withdrawn_advisory_affects_nothing() {
        let text = SAMPLE.replace("date =", "withdrawn = \"2021-01-01\"\ndate =");
        let a = parse_advisory(&text).unwrap();
        assert!(!a.affects(&Version::parse("0.0.1").unwrap()));
    }

    #[test]
    fn an_advisory_with_no_cvss_is_unscored_and_still_blocks() {
        let text = SAMPLE
            .lines()
            .filter(|l| !l.starts_with("cvss"))
            .collect::<Vec<_>>()
            .join("\n");
        let a = parse_advisory(&text).expect("parses without cvss");
        assert_eq!(a.severity, Severity::Unknown);
        assert!(
            a.severity.blocks_release(),
            "an advisory nobody scored still needs a human"
        );
    }

    #[test]
    fn cvss_scores_match_the_specification() {
        // Vectors and scores from the CVSS v3.1 specification examples.
        let cases = [
            ("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H", 9.8),
            ("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H", 7.5),
            ("CVSS:3.1/AV:N/AC:H/PR:N/UI:R/S:U/C:L/I:N/A:N", 3.1),
            ("CVSS:3.1/AV:L/AC:L/PR:L/UI:N/S:U/C:H/I:H/A:H", 7.8),
            ("CVSS:3.1/AV:N/AC:L/PR:N/UI:R/S:C/C:L/I:L/A:N", 6.1),
        ];
        for (vector, expected) in cases {
            let got =
                cvss_v3_base_score(vector).unwrap_or_else(|| panic!("failed to score {vector}"));
            assert!(
                (got - expected).abs() < 0.05,
                "{vector}: expected {expected}, got {got}"
            );
        }
    }

    #[test]
    fn severity_bands_follow_the_cvss_ratings() {
        assert_eq!(Severity::from_score(9.8), Severity::Critical);
        assert_eq!(Severity::from_score(9.0), Severity::Critical);
        assert_eq!(Severity::from_score(7.5), Severity::High);
        assert_eq!(Severity::from_score(4.0), Severity::Medium);
        assert_eq!(Severity::from_score(3.9), Severity::Low);
        assert_eq!(Severity::from_score(0.0), Severity::None);

        assert!(Severity::Critical.blocks_release());
        assert!(Severity::High.blocks_release());
        assert!(!Severity::Medium.blocks_release());
        assert!(!Severity::Low.blocks_release());
    }

    #[test]
    fn a_non_v3_vector_is_rejected_rather_than_mis_scored() {
        assert!(cvss_v3_base_score("CVSS:2.0/AV:N/AC:L/Au:N/C:P/I:P/A:P").is_none());
        assert!(cvss_v3_base_score("nonsense").is_none());
    }

    #[test]
    fn an_unscoreable_vector_escalates_rather_than_dropping_the_advisory() {
        // The database holds CVSS 4.0 vectors this scorer does not implement.
        // Dropping them would silently shrink the database — the exact failure
        // this gate exists to prevent.
        let text = SAMPLE.replace(
            "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H",
            "CVSS:4.0/AV:N/AC:L/AT:N/PR:N/UI:N/VC:H/VI:H/VA:H/SC:H/SI:H/SA:H",
        );
        let a = parse_advisory(&text).expect("still parsed");
        assert_eq!(a.severity, Severity::Unknown);
        assert!(a.severity.blocks_release());
    }
}
