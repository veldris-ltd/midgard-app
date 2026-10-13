//! RF-SC-4 — the scan gate, driven through the **actual binary**. AC-RF-SC-4a … 4f.
//!
//! The unit tests cover the evaluators; these cover the thing the release gate
//! consumes — an exit code. Each builds a small advisory database and policy
//! directory of its own, so nothing here depends on what `vendor/advisory-db`
//! happens to contain today, and everything runs offline. `[NFR-PRV-1]`

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const EXE: &str = env!("CARGO_BIN_EXE_mg-scan");

/// The date the fixtures treat as today.
const TODAY: &str = "2026-10-12";

/// CVSS 9.8 — CRITICAL.
const CRITICAL: &str = "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H";
/// CVSS 7.5 — HIGH.
const HIGH: &str = "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H";
/// CVSS 5.3 — MEDIUM.
const MEDIUM: &str = "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:L";

/// A scratch directory holding an SBOM, an advisory database and a policy.
struct Fixture {
    dir: PathBuf,
    advisories: usize,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("mg-scan-cli-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("db/crates")).unwrap();
        std::fs::create_dir_all(dir.join("policy")).unwrap();
        let f = Self { dir, advisories: 0 };
        f.licences("");
        f.exceptions("");
        f
    }

    /// Add one advisory to the database.
    fn advisory(&mut self, id: &str, krate: &str, cvss: &str, patched: &str) -> &mut Self {
        let dir = self.dir.join("db/crates").join(krate);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{id}.md")),
            format!(
                "```toml\n[advisory]\nid = \"{id}\"\npackage = \"{krate}\"\ncvss = \"{cvss}\"\n\
                 date = \"2026-01-01\"\nurl = \"https://example.invalid/{id}\"\n\n\
                 [versions]\npatched = [{patched}]\n```\n\n# {id}\n"
            ),
        )
        .unwrap();
        self.advisories += 1;
        self
    }

    /// Write the snapshot manifest, declaring `count` advisories dated `date`.
    fn snapshot_with(&self, date: &str, count: usize, extra: &str) {
        std::fs::write(
            self.dir.join("db/snapshot.toml"),
            format!(
                "[snapshot]\ndate = \"{date}\"\nadvisory_count = {count}\n\n\
                 [staleness]\nwarn_after_days = 30\nfail_after_days = 180\n{extra}"
            ),
        )
        .unwrap();
    }

    /// A fresh snapshot declaring exactly the advisories added so far.
    fn snapshot(&self) {
        self.snapshot_with("2026-10-01", self.advisories, "");
    }

    fn licences(&self, exceptions: &str) {
        std::fs::write(
            self.dir.join("policy/licences.toml"),
            format!(
                "[allowed]\n\"MIT\" = \"permissive\"\n\"Apache-2.0\" = \"ours\"\n\n\
                 [modifiers]\n\"LLVM-exception\" = \"relaxes Apache-2.0\"\n\n[exceptions]\n{exceptions}"
            ),
        )
        .unwrap();
    }

    fn exceptions(&self, entries: &str) {
        std::fs::write(
            self.dir.join("policy/advisories.toml"),
            format!("[advisories]\n{entries}"),
        )
        .unwrap();
    }

    /// Write an SBOM of `(name, version, licence)` components, none of which
    /// says where it came from — so every one is matched against advisories.
    fn sbom(&self, components: &[(&str, &str, Option<&str>)]) -> PathBuf {
        let rows: Vec<String> = components
            .iter()
            .map(|(n, v, l)| {
                let licences = match l {
                    Some(e) => format!("[ {{ \"expression\": \"{e}\" }} ]"),
                    None => "[]".to_string(),
                };
                format!(
                    "    {{ \"type\": \"library\", \"name\": \"{n}\", \"version\": \"{v}\", \
                     \"purl\": \"pkg:cargo/{n}@{v}\", \"licenses\": {licences} }}"
                )
            })
            .collect();
        self.raw_sbom(&format!(
            "{{\n  \"bomFormat\": \"CycloneDX\",\n  \"specVersion\": \"1.5\",\n  \
             \"components\": [\n{}\n  ]\n}}\n",
            rows.join(",\n")
        ))
    }

    fn raw_sbom(&self, text: &str) -> PathBuf {
        let p = self.dir.join("app.sbom.cdx.json");
        std::fs::write(&p, text).unwrap();
        p
    }

    fn command(&self, sbom: &Path) -> Command {
        let mut c = Command::new(EXE);
        c.arg(sbom)
            .arg("--advisory-db")
            .arg(self.dir.join("db"))
            .arg("--policy-dir")
            .arg(self.dir.join("policy"))
            // The fixtures are dated; a real clock would age them into failure.
            .args(["--today", TODAY])
            .env_remove("SOURCE_DATE_EPOCH");
        c
    }

    fn scan(&self, sbom: &Path, extra: &[&str]) -> Output {
        self.command(sbom)
            .args(extra)
            .output()
            .expect("run mg-scan")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn refused(o: &Output) -> bool {
    o.status.code() == Some(1)
}

// --- the positive case -------------------------------------------------------

#[test]
fn a_clean_sbom_passes() {
    let mut f = Fixture::new("clean");
    f.advisory("RUSTSEC-2026-0001", "vuln", CRITICAL, "\">= 2.0.0\"");
    f.snapshot();
    let sbom = f.sbom(&[
        ("vuln", "2.0.0", Some("MIT")),
        ("other", "1.0.0", Some("MIT OR Apache-2.0")),
    ]);

    let out = f.scan(&sbom, &[]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert!(stdout(&out).contains("OK — no blocking advisory or licence violation"));
}

// --- AC-RF-SC-4a — a CRITICAL or HIGH advisory blocks ------------------------

#[test]
fn ac_rf_sc_4a_a_critical_or_high_advisory_blocks_and_names_the_component() {
    let mut f = Fixture::new("advisory");
    f.advisory("RUSTSEC-2026-0001", "vuln", CRITICAL, "\">= 2.0.0\"")
        .advisory("RUSTSEC-2026-0002", "shaky", HIGH, "\">= 1.5.0\"")
        .advisory("RUSTSEC-2026-0003", "minor", MEDIUM, "\">= 9.0.0\"");
    f.snapshot();
    let sbom = f.sbom(&[
        ("vuln", "1.0.0", Some("MIT")),
        ("shaky", "1.0.0", Some("MIT")),
        ("minor", "1.0.0", Some("MIT")),
    ]);

    let out = f.scan(
        &sbom,
        &["--out", f.dir.join("findings.json").to_str().unwrap()],
    );
    let text = stdout(&out);
    assert!(refused(&out), "must refuse: {text}");
    assert!(
        text.contains("BLOCK  vuln@1.0.0  advisory-critical  RUSTSEC-2026-0001"),
        "{text}"
    );
    assert!(
        text.contains("BLOCK  shaky@1.0.0  advisory-high  RUSTSEC-2026-0002"),
        "{text}"
    );
    // MEDIUM is reported and does not block.
    assert!(
        text.contains("note   minor@1.0.0  advisory-medium  RUSTSEC-2026-0003"),
        "{text}"
    );
    assert!(
        text.contains("3 finding(s), 2 blocking a release"),
        "{text}"
    );
    assert!(!text.contains("OK"), "nothing may claim success: {text}");

    // The findings report is written even though the run refused.
    let report = std::fs::read_to_string(f.dir.join("findings.json")).expect("report written");
    assert!(report.contains("\"kind\": \"advisory-critical\""));
}

#[test]
fn a_compound_range_does_not_clear_the_versions_below_it() {
    // A backported fix: `>= 0.8.26, < 1.0.0` is ONE requirement. Everything
    // below 0.8.26, and 1.0.0 up to the 1.x fix, is still vulnerable.
    let mut f = Fixture::new("compound");
    f.advisory(
        "RUSTSEC-2026-0004",
        "backported",
        HIGH,
        "\">=0.8.26, <1.0.0\", \">=1.4.1\"",
    );
    f.snapshot();

    for (version, vulnerable) in [
        ("0.8.0", true),
        ("0.8.26", false),
        ("1.2.0", true),
        ("1.4.1", false),
    ] {
        let sbom = f.sbom(&[("backported", version, Some("MIT"))]);
        let out = f.scan(&sbom, &[]);
        assert_eq!(
            refused(&out),
            vulnerable,
            "backported {version}: {}",
            stdout(&out)
        );
    }
}

#[test]
fn a_component_is_scanned_however_the_sbom_is_written() {
    // Compact JSON and a non-library type. A reader keyed on the text
    // `"type": "library"` skips this component and reports the SBOM clean.
    let mut f = Fixture::new("formatting");
    f.advisory("RUSTSEC-2026-0001", "vuln", CRITICAL, "\">= 2.0.0\"");
    f.snapshot();
    let sbom = f.raw_sbom(concat!(
        r#"{"bomFormat":"CycloneDX","specVersion":"1.5","components":["#,
        r#"{"type":"framework","name":"vuln","version":"1.0.0","purl":"pkg:cargo/vuln@1.0.0","#,
        r#""licenses":[{"expression":"MIT"}]}]}"#
    ));

    let out = f.scan(&sbom, &[]);
    assert!(refused(&out), "{}{}", stdout(&out), stderr(&out));
    assert!(stdout(&out).contains("advisory-critical"));
}

#[test]
fn only_a_declared_workspace_member_is_exempt_from_advisory_matching() {
    // Our own crate may share a name with a vulnerable registry crate. It is
    // exempt when the SBOM says it is a workspace member — and only then.
    let mut f = Fixture::new("origin");
    f.advisory("RUSTSEC-2026-0001", "telemetry", CRITICAL, "\">= 2.0.0\"");
    f.snapshot();
    let with = |source: &str| {
        format!(
            "{{ \"bomFormat\": \"CycloneDX\", \"specVersion\": \"1.5\", \"components\": [\n\
             {{ \"type\": \"library\", \"name\": \"telemetry\", \"version\": \"0.1.0\", \
             \"purl\": \"pkg:cargo/telemetry@0.1.0\", \"licenses\": [ {{ \"expression\": \"MIT\" }} ]{source} }}\n] }}"
        )
    };
    let property = |v: &str| {
        format!(", \"properties\": [ {{ \"name\": \"midgard:source\", \"value\": \"{v}\" }} ]")
    };

    let ours = f.raw_sbom(&with(&property("workspace")));
    let out = f.scan(&ours, &[]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        stderr(&out).contains("1 workspace member(s) exempt"),
        "{}",
        stderr(&out)
    );

    for source in [property("registry"), property("git"), String::new()] {
        let sbom = f.raw_sbom(&with(&source));
        let out = f.scan(&sbom, &[]);
        assert!(
            refused(&out),
            "source {source:?} must be matched: {}",
            stdout(&out)
        );
    }
}

// --- AC-RF-SC-4b / 4c — licences ---------------------------------------------

#[test]
fn ac_rf_sc_4b_a_disallowed_licence_blocks_naming_component_licence_and_purl() {
    let mut f = Fixture::new("licence");
    f.advisory("RUSTSEC-2026-0001", "unrelated", MEDIUM, "\">= 1.0.0\"");
    f.snapshot();
    let sbom = f.sbom(&[("copyleft", "3.1.0", Some("GPL-3.0"))]);
    let report = f.dir.join("findings.json");

    let out = f.scan(&sbom, &["--out", report.to_str().unwrap()]);
    let text = stdout(&out);
    assert!(refused(&out), "{text}");
    assert!(
        text.contains("copyleft@3.1.0  licence-disallowed"),
        "{text}"
    );
    assert!(text.contains("GPL-3.0"), "the licence is named: {text}");
    let report = std::fs::read_to_string(report).unwrap();
    assert!(
        report.contains("\"purl\": \"pkg:cargo/copyleft@3.1.0\""),
        "{report}"
    );
}

#[test]
fn ac_rf_sc_4c_a_component_with_no_licence_blocks() {
    let mut f = Fixture::new("unlicensed");
    f.advisory("RUSTSEC-2026-0001", "unrelated", MEDIUM, "\">= 1.0.0\"");
    f.snapshot();
    let sbom = f.sbom(&[("mystery", "1.0.0", None), ("fine", "1.0.0", Some("MIT"))]);

    let out = f.scan(&sbom, &[]);
    let text = stdout(&out);
    assert!(refused(&out), "unknown is not permitted: {text}");
    assert!(
        text.contains("BLOCK  mystery@1.0.0  licence-unknown"),
        "{text}"
    );
    assert!(!text.contains("fine@1.0.0"), "{text}");
}

// --- AC-RF-SC-4d — exceptions expire -----------------------------------------

fn exception(id: &str, krate: &str, expires: &str) -> String {
    format!(
        "\n[advisories.\"{id}\"]\ncrate = \"{krate}\"\nowner = \"a.name\"\n\
         reason = \"Only reachable from a path the service never invokes.\"\n\
         compensating_control = \"Egress broker denies it; see docs/ops/runbooks/egress.md\"\n\
         expires = \"{expires}\"\n"
    )
}

#[test]
fn ac_rf_sc_4d_an_exception_holds_until_it_expires_then_blocks_with_date_and_owner() {
    let mut f = Fixture::new("exception");
    f.advisory("RUSTSEC-2026-0001", "vuln", CRITICAL, "\">= 2.0.0\"");
    f.snapshot();
    let sbom = f.sbom(&[("vuln", "1.0.0", Some("MIT"))]);

    // Valid through its expiry day: the advisory is a note, in both modes.
    f.exceptions(&exception("RUSTSEC-2026-0001", "vuln", TODAY));
    for mode in [&[][..], &["--advisory-only"][..]] {
        let out = f.scan(&sbom, mode);
        let text = stdout(&out);
        assert!(out.status.success(), "{mode:?}: {text}{}", stderr(&out));
        assert!(text.contains("[exception recorded]"), "{text}");
        assert!(text.contains("OK — no blocking"), "{mode:?}: {text}");
    }

    // The day after, it fails — and says whose it was and when it lapsed.
    f.exceptions(&exception("RUSTSEC-2026-0001", "vuln", "2026-10-11"));
    let out = f.scan(&sbom, &[]);
    let text = stdout(&out);
    assert!(refused(&out), "{text}");
    assert!(
        text.contains("expired-advisory-exception  exception for RUSTSEC-2026-0001 expired 2026-10-11 (owner a.name)"),
        "{text}"
    );
    assert!(
        text.contains("BLOCK  vuln@1.0.0  advisory-critical"),
        "the advisory blocks again: {text}"
    );

    // An exception for a different crate does not cover this one.
    f.exceptions(&exception("RUSTSEC-2026-0001", "another", "2027-01-01"));
    assert!(refused(&f.scan(&sbom, &[])));
}

#[test]
fn an_exception_that_does_not_say_who_why_or_how_is_refused_outright() {
    let mut f = Fixture::new("bad-exception");
    f.advisory("RUSTSEC-2026-0001", "vuln", CRITICAL, "\">= 2.0.0\"");
    f.snapshot();
    let sbom = f.sbom(&[("vuln", "2.0.0", Some("MIT"))]);
    let good = exception("RUSTSEC-2026-0001", "vuln", "2027-01-01");

    for (tag, broken) in [
        ("an empty owner", good.replace("\"a.name\"", "\"\"")),
        (
            "no reason",
            good.replace(
                "reason = \"Only reachable from a path the service never invokes.\"\n",
                "",
            ),
        ),
        (
            "a control that links to nothing",
            good.replace(
                "Egress broker denies it; see docs/ops/runbooks/egress.md",
                "It is handled.",
            ),
        ),
    ] {
        f.exceptions(&broken);
        let out = f.scan(&sbom, &[]);
        assert!(refused(&out), "{tag} must refuse the scan");
        assert!(
            stderr(&out).contains("RUSTSEC-2026-0001"),
            "{tag}: {}",
            stderr(&out)
        );
    }

    // A licence exception needs its reason too.
    f.exceptions("");
    f.licences("\n[exceptions.\"copyleft\"]\nlicence = \"MPL-2.0\"\nowner = \"a.name\"\nexpires = \"2027-01-01\"\n");
    let out = f.scan(&sbom, &[]);
    assert!(refused(&out));
    assert!(stderr(&out).contains("has no `reason`"), "{}", stderr(&out));
}

// --- AC-RF-SC-4f — the database's age, judged by a clock that can be trusted --

#[test]
fn ac_rf_sc_4f_a_stale_database_fails_naming_the_snapshot_date() {
    let mut f = Fixture::new("stale");
    f.advisory("RUSTSEC-2026-0001", "vuln", CRITICAL, "\">= 2.0.0\"");
    let sbom = f.sbom(&[("fine", "1.0.0", Some("MIT"))]);

    // 181 days before the fixture's today: one past the hard limit.
    f.snapshot_with("2026-04-14", 1, "");
    let out = f.scan(&sbom, &[]);
    let err = stderr(&out);
    assert!(refused(&out), "{err}");
    assert!(
        err.contains("181 days old (snapshot 2026-04-14, limit 180)"),
        "{err}"
    );
    assert!(
        stdout(&out).is_empty(),
        "no verdict from a database this old"
    );

    // At the limit it still runs, and warns.
    f.snapshot_with("2026-04-15", 1, "");
    let out = f.scan(&sbom, &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("WARNING — advisory database is 180 days old"));
}

#[test]
fn the_build_date_variable_cannot_make_an_old_database_fresh() {
    // Reproducible-build shells export SOURCE_DATE_EPOCH as a fixed instant
    // years ago. Read as "today", it would hide any amount of staleness and
    // keep every exception alive. The scan judges age by the real clock.
    let mut f = Fixture::new("epoch");
    f.advisory("RUSTSEC-2026-0001", "vuln", CRITICAL, "\">= 2.0.0\"");
    f.snapshot_with("2020-01-01", 1, "");
    let sbom = f.sbom(&[("fine", "1.0.0", Some("MIT"))]);

    let out = Command::new(EXE)
        .arg(&sbom)
        .arg("--advisory-db")
        .arg(f.dir.join("db"))
        .arg("--policy-dir")
        .arg(f.dir.join("policy"))
        .env("SOURCE_DATE_EPOCH", "315532800") // 1980-01-01
        .output()
        .expect("run mg-scan");
    assert!(refused(&out), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("days old (snapshot 2020-01-01"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_clock_behind_the_data_is_refused() {
    let mut f = Fixture::new("clock");
    f.advisory("RUSTSEC-2026-0001", "vuln", CRITICAL, "\">= 2.0.0\"");
    f.snapshot_with("2026-10-13", 1, ""); // the day after the fixture's today
    let sbom = f.sbom(&[("fine", "1.0.0", Some("MIT"))]);

    let out = f.scan(&sbom, &[]);
    assert!(refused(&out));
    assert!(
        stderr(&out).contains("dated 2026-10-13, after today (2026-10-12)"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_database_that_is_not_what_its_manifest_says_is_refused() {
    let mut f = Fixture::new("integrity");
    f.advisory("RUSTSEC-2026-0001", "vuln", CRITICAL, "\">= 2.0.0\"")
        .advisory("RUSTSEC-2026-0002", "other", HIGH, "\">= 2.0.0\"");
    let sbom = f.sbom(&[("fine", "1.0.0", Some("MIT"))]);

    // Fewer loaded than declared, and more: both mean the directory is not the
    // snapshot the manifest describes.
    for declared in [3, 1] {
        f.snapshot_with("2026-10-01", declared, "");
        let out = f.scan(&sbom, &[]);
        assert!(refused(&out), "declared {declared}");
        assert!(
            stderr(&out).contains(&format!("declares {declared} advisories but 2 loaded")),
            "{}",
            stderr(&out)
        );
    }

    // An advisory file the parser cannot read is a refusal, not a skip.
    f.snapshot_with("2026-10-01", 2, "");
    std::fs::write(
        f.dir.join("db/crates/other/RUSTSEC-2026-0002.md"),
        "# no frontmatter\n",
    )
    .unwrap();
    let out = f.scan(&sbom, &[]);
    assert!(refused(&out));
    assert!(
        stderr(&out).contains("cannot parse advisory"),
        "{}",
        stderr(&out)
    );

    // A staleness limit that is written down but unreadable is not replaced by
    // a default.
    let mut g = Fixture::new("limits");
    g.advisory("RUSTSEC-2026-0001", "vuln", CRITICAL, "\">= 2.0.0\"");
    std::fs::write(
        g.dir.join("db/snapshot.toml"),
        "[snapshot]\ndate = \"2026-10-01\"\nadvisory_count = 1\n\n[staleness]\nfail_after_days = \"never\"\n",
    )
    .unwrap();
    let sbom = g.sbom(&[("fine", "1.0.0", Some("MIT"))]);
    let out = g.scan(&sbom, &[]);
    assert!(refused(&out));
    assert!(stderr(&out).contains("fail_after_days"), "{}", stderr(&out));
}

// --- AC-RF-SC-5b — npm components, and one database per ecosystem ------------

/// An OSV advisory for one npm package, CRITICAL, fixed in `fixed`.
fn osv(id: &str, package: &str, fixed: &str) -> String {
    format!(
        r#"{{ "id": "{id}", "affected": [ {{ "package": {{ "ecosystem": "npm", "name": "{package}" }},
            "ranges": [ {{ "type": "SEMVER", "events": [ {{ "introduced": "0" }}, {{ "fixed": "{fixed}" }} ] }} ] }} ],
            "severity": [ {{ "type": "CVSS_V3", "score": "{CRITICAL}" }} ],
            "references": [ {{ "type": "ADVISORY", "url": "https://example.invalid/{id}" }} ] }}"#
    )
}

impl Fixture {
    /// Write an npm advisory snapshot covering `packages`, holding `advisories`.
    fn npm_db(&self, packages: &[&str], advisories: &[(&str, String)]) {
        let dir = self.dir.join("npm-db");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("advisories")).unwrap();
        for (id, doc) in advisories {
            std::fs::write(dir.join(format!("advisories/{id}.json")), doc).unwrap();
        }
        std::fs::write(dir.join("packages.txt"), packages.join("\n")).unwrap();
        std::fs::write(
            dir.join("snapshot.toml"),
            format!(
                "[snapshot]\necosystem = \"npm\"\ndate = \"2026-10-01\"\nadvisory_count = {}\n",
                advisories.len()
            ),
        )
        .unwrap();
    }

    /// An SBOM of `(purl-type, name, version, source)` components, all MIT.
    fn mixed_sbom(&self, components: &[(&str, &str, &str, &str)]) -> PathBuf {
        let rows: Vec<String> = components
            .iter()
            .map(|(kind, n, v, source)| {
                let purl_name = n.replacen('@', "%40", 1);
                format!(
                    "    {{ \"type\": \"library\", \"name\": \"{n}\", \"version\": \"{v}\", \
                     \"purl\": \"pkg:{kind}/{purl_name}@{v}\", \
                     \"licenses\": [ {{ \"expression\": \"MIT\" }} ], \
                     \"properties\": [ {{ \"name\": \"midgard:source\", \"value\": \"{source}\" }} ] }}"
                )
            })
            .collect();
        self.raw_sbom(&format!(
            "{{\n  \"bomFormat\": \"CycloneDX\",\n  \"specVersion\": \"1.5\",\n  \
             \"components\": [\n{}\n  ]\n}}\n",
            rows.join(",\n")
        ))
    }

    fn scan_npm(&self, sbom: &Path, extra: &[&str]) -> Output {
        self.command(sbom)
            .arg("--npm-advisory-db")
            .arg(self.dir.join("npm-db"))
            .args(extra)
            .output()
            .expect("run mg-scan")
    }
}

#[test]
fn ac_rf_sc_5b_an_npm_component_is_matched_against_npm_advisories() {
    let mut f = Fixture::new("npm");
    f.advisory("RUSTSEC-2026-0001", "unrelated", MEDIUM, "\">= 1.0.0\"");
    f.snapshot();
    f.npm_db(
        &["@scope/widget", "react"],
        &[(
            "GHSA-1111-2222-3333",
            osv("GHSA-1111-2222-3333", "@scope/widget", "2.0.0"),
        )],
    );

    let sbom = f.mixed_sbom(&[
        ("npm", "@scope/widget", "1.4.0", "registry"),
        ("npm", "react", "18.3.1", "registry"),
    ]);
    let out = f.scan_npm(&sbom, &[]);
    let text = stdout(&out);
    assert!(refused(&out), "{text}{}", stderr(&out));
    assert!(
        text.contains("BLOCK  @scope/widget@1.4.0  advisory-critical  GHSA-1111-2222-3333"),
        "{text}"
    );
    assert!(!text.contains("react@18.3.1"), "{text}");
    assert!(stderr(&out).contains("1 npm"), "{}", stderr(&out));

    // The fixed version passes.
    let sbom = f.mixed_sbom(&[("npm", "@scope/widget", "2.0.0", "registry")]);
    let out = f.scan_npm(&sbom, &[]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
}

#[test]
fn a_shared_name_does_not_cross_ecosystems() {
    // `shared` is vulnerable as a crate (RustSec) and, separately, clean as an
    // npm package; `twin` is the reverse. Matched by name alone, each would be
    // judged by the other's advisory.
    let mut f = Fixture::new("ecosystems");
    f.advisory("RUSTSEC-2026-0001", "shared", CRITICAL, "\">= 9.0.0\"");
    f.snapshot();
    f.npm_db(
        &["shared", "twin"],
        &[(
            "GHSA-4444-5555-6666",
            osv("GHSA-4444-5555-6666", "twin", "9.0.0"),
        )],
    );

    let clean = f.mixed_sbom(&[
        ("npm", "shared", "1.0.0", "registry"),
        ("cargo", "twin", "1.0.0", "registry"),
    ]);
    let out = f.scan_npm(&clean, &[]);
    assert!(
        out.status.success(),
        "neither advisory is for these ecosystems: {}{}",
        stdout(&out),
        stderr(&out)
    );

    let vulnerable = f.mixed_sbom(&[
        ("cargo", "shared", "1.0.0", "registry"),
        ("npm", "twin", "1.0.0", "registry"),
    ]);
    let out = f.scan_npm(&vulnerable, &[]);
    let text = stdout(&out);
    assert!(refused(&out), "{text}");
    assert!(
        text.contains("shared@1.0.0  advisory-critical  RUSTSEC-2026-0001"),
        "{text}"
    );
    assert!(
        text.contains("twin@1.0.0  advisory-critical  GHSA-4444-5555-6666"),
        "{text}"
    );
}

#[test]
fn nobody_looked_is_never_reported_as_clean() {
    let mut f = Fixture::new("coverage");
    f.advisory("RUSTSEC-2026-0001", "unrelated", MEDIUM, "\">= 1.0.0\"");
    f.snapshot();
    let sbom = f.mixed_sbom(&[("npm", "react", "18.3.1", "registry")]);

    // npm packages, and no npm advisory database at all.
    let out = f.scan_npm(&sbom, &[]);
    assert!(refused(&out));
    assert!(
        stderr(&out).contains("the npm advisory database is not usable"),
        "{}",
        stderr(&out)
    );
    assert!(stdout(&out).is_empty(), "no verdict without a database");

    // A database that was not filtered for this package.
    f.npm_db(&["something-else"], &[]);
    let out = f.scan_npm(&sbom, &[]);
    let text = stdout(&out);
    assert!(refused(&out), "{text}");
    assert!(
        text.contains("BLOCK  react@18.3.1  advisory-uncovered"),
        "{text}"
    );

    // Filtered for it, with nothing recorded against it: that is a real "clean".
    f.npm_db(&["react"], &[]);
    let out = f.scan_npm(&sbom, &[]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));

    // An ecosystem with no vendored database at all.
    let sbom = f.mixed_sbom(&[("pypi", "requests", "2.0.0", "registry")]);
    let out = f.scan_npm(&sbom, &[]);
    let text = stdout(&out);
    assert!(refused(&out), "{text}");
    assert!(
        text.contains("requests@2.0.0  ecosystem-unsupported"),
        "{text}"
    );
}

#[test]
fn a_workspace_npm_package_needs_no_npm_database() {
    // The SDK tarball's bill of materials is our own package and nothing else.
    // It is licence-checked like anything, and it is not a registry package, so
    // the scan neither matches it nor needs the npm snapshot to exist.
    let mut f = Fixture::new("workspace-npm");
    f.advisory("RUSTSEC-2026-0001", "unrelated", MEDIUM, "\">= 1.0.0\"");
    f.snapshot();
    let sbom = f.mixed_sbom(&[("npm", "@midgard/sdk", "0.0.0", "workspace")]);

    let out = f.scan_npm(&sbom, &[]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert!(
        stderr(&out).contains("1 workspace member(s) exempt"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn import_osv_builds_a_snapshot_the_scan_accepts() {
    let mut f = Fixture::new("import");
    f.advisory("RUSTSEC-2026-0001", "unrelated", MEDIUM, "\">= 1.0.0\"");
    f.snapshot();

    // An export: one advisory for a locked package, one for a package we do not
    // use, one for another ecosystem, and one document with nothing in it.
    let export = f.dir.join("export/nested");
    std::fs::create_dir_all(&export).unwrap();
    std::fs::write(
        export.join("a.json"),
        osv("GHSA-aaaa-aaaa-aaaa", "react", "19.0.0"),
    )
    .unwrap();
    std::fs::write(
        export.join("b.json"),
        osv("GHSA-bbbb-bbbb-bbbb", "not-ours", "1.0.0"),
    )
    .unwrap();
    std::fs::write(
        export.join("c.json"),
        osv("GHSA-cccc-cccc-cccc", "react", "1.0.0").replace("\"npm\"", "\"PyPI\""),
    )
    .unwrap();
    std::fs::write(export.join("d.json"), r#"{ "id": "GHSA-dddd-dddd-dddd" }"#).unwrap();
    std::fs::write(f.dir.join("packages.list"), "react\nscheduler\n").unwrap();

    let import = |date: &str| {
        Command::new(EXE)
            .arg("import-osv")
            .arg("--from")
            .arg(f.dir.join("export"))
            .arg("--packages")
            .arg(f.dir.join("packages.list"))
            .arg("--out")
            .arg(f.dir.join("npm-db"))
            .args(["--date", date, "--source", "https://example.invalid/osv"])
            .output()
            .expect("run mg-scan import-osv")
    };
    let out = import("2026-10-01");
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("imported 1 advisories affecting 2 locked npm package(s)"));
    assert!(f
        .dir
        .join("npm-db/advisories/GHSA-aaaa-aaaa-aaaa.json")
        .exists());
    assert!(!f
        .dir
        .join("npm-db/advisories/GHSA-bbbb-bbbb-bbbb.json")
        .exists());

    // The snapshot it wrote is one the scan accepts, and it finds the advisory.
    let sbom = f.mixed_sbom(&[
        ("npm", "react", "18.3.1", "registry"),
        ("npm", "scheduler", "0.23.2", "registry"),
    ]);
    let out = f.scan_npm(&sbom, &[]);
    let text = stdout(&out);
    assert!(refused(&out), "{text}{}", stderr(&out));
    assert!(
        text.contains("react@18.3.1  advisory-critical  GHSA-aaaa-aaaa-aaaa"),
        "{text}"
    );
    assert!(
        !text.contains("scheduler"),
        "covered, and nothing recorded: {text}"
    );

    // A document that names one of our packages and cannot be read is fatal.
    std::fs::write(
        export.join("e.json"),
        r#"{ "id": "GHSA-eeee", "note": "react" "#,
    )
    .unwrap();
    assert_eq!(import("2026-10-01").status.code(), Some(1));
    std::fs::remove_file(export.join("e.json")).unwrap();

    assert_eq!(
        import("soon").status.code(),
        Some(2),
        "a date must be a date"
    );
}

// --- modes and exit codes ----------------------------------------------------

#[test]
fn pr_mode_fails_on_critical_and_never_calls_a_release_blocker_clean() {
    let mut f = Fixture::new("pr");
    f.advisory("RUSTSEC-2026-0001", "vuln", CRITICAL, "\">= 2.0.0\"")
        .advisory("RUSTSEC-2026-0002", "shaky", HIGH, "\">= 1.5.0\"");
    f.snapshot();

    // HIGH and a licence violation: a release is refused, a PR is not — but the
    // PR run says what it is passing over.
    let sbom = f.sbom(&[("shaky", "1.0.0", Some("GPL-3.0"))]);
    assert!(refused(&f.scan(&sbom, &[])));
    let out = f.scan(&sbom, &["--advisory-only"]);
    let text = stdout(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("PASSED (advisory-only)"), "{text}");
    assert!(
        text.contains("2 finding(s) above WOULD BLOCK A RELEASE"),
        "{text}"
    );
    assert!(!text.contains("OK — no blocking"), "{text}");

    // CRITICAL fails a PR too.
    let sbom = f.sbom(&[("vuln", "1.0.0", Some("MIT"))]);
    assert!(refused(&f.scan(&sbom, &["--advisory-only"])));
}

#[test]
fn help_cannot_turn_a_scan_into_a_success() {
    let mut f = Fixture::new("help");
    f.advisory("RUSTSEC-2026-0001", "vuln", CRITICAL, "\">= 2.0.0\"");
    f.snapshot();
    let sbom = f.sbom(&[("vuln", "1.0.0", Some("MIT"))]);

    let out = f.scan(&sbom, &["--help"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "--help beside an SBOM is a usage error"
    );
    assert!(stdout(&out).is_empty());

    let out = Command::new(EXE).arg("--help").output().unwrap();
    assert!(out.status.success());
    assert!(stdout(&out).contains("usage:"));
}
