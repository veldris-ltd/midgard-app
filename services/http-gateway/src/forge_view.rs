//! The Forge console's live view of the enclosure's data operations. [FR-F-2] [FR-F-4]
//!
//! With `--forge-jobs`, signed-in users the policy lets `Read` it get
//! `GET /api/v1/forge/overview`: every Forge job as the jobs file declares it (its
//! source, columns, and how rows become objects and links), the worker's live status
//! for each (`--forge-status`, the `status.json` it rewrites after every run), the
//! Heimdall feeds' live status when they report one (`--heimdall-feeds-status`), and
//! how many objects of each type the caller may read. The files hold definitions,
//! counts and outcomes, no data, and are read afresh on every request; a status file
//! not yet written is reported as absent, not as an error.

use std::path::PathBuf;

use service_runtime::json::{parse, Json};

use crate::enclosure::EdgeError;

/// The route.
pub const OVERVIEW_PATH: &str = "/api/v1/forge/overview";

/// Where the view is read from.
pub struct ForgeView {
    jobs: PathBuf,
    status: Option<PathBuf>,
    feeds: Option<PathBuf>,
}

impl ForgeView {
    /// A view of the jobs file `jobs`, with the worker's `status` file and the Heimdall
    /// feeds' `feeds` status file when given.
    pub fn new(jobs: PathBuf, status: Option<PathBuf>, feeds: Option<PathBuf>) -> Self {
        Self {
            jobs,
            status,
            feeds,
        }
    }

    /// The overview, with `object_types` (the caller's readable object types, as
    /// `GET /object-types` returns them) reduced to counts.
    pub fn overview(&self, object_types: &Json) -> Result<Json, EdgeError> {
        let jobs = read(&self.jobs)?
            .ok_or_else(|| EdgeError::new(503, "the Forge jobs file is missing"))?;
        let field = |doc: Option<Json>, key: &str| doc.and_then(|d| d.get(key).cloned());
        let status = match &self.status {
            Some(p) => read(p)?,
            None => None,
        };
        let feeds = match &self.feeds {
            Some(p) => read(p)?,
            None => None,
        };
        let counts = match object_types {
            Json::Arr(types) => Json::obj(
                types
                    .iter()
                    .filter_map(|t| {
                        let api = t.str_field("apiName")?;
                        let n: i64 = t
                            .str_field("instanceCount")
                            .unwrap_or("0")
                            .chars()
                            .filter(char::is_ascii_digit)
                            .collect::<String>()
                            .parse()
                            .unwrap_or(0);
                        Some((api, Json::Int(n)))
                    })
                    .collect(),
            ),
            _ => Json::obj(Vec::new()),
        };
        Ok(Json::obj(vec![
            (
                "jobs",
                field(Some(jobs), "jobs").unwrap_or(Json::Arr(Vec::new())),
            ),
            (
                "jobStatus",
                field(status.clone(), "jobs").unwrap_or(Json::Arr(Vec::new())),
            ),
            (
                "statusAt",
                field(status, "generatedAt").unwrap_or(Json::Null),
            ),
            (
                "feeds",
                field(feeds.clone(), "sources").unwrap_or(Json::obj(Vec::new())),
            ),
            ("feedsAt", field(feeds, "generatedAt").unwrap_or(Json::Null)),
            ("objectCounts", counts),
        ]))
    }
}

/// A JSON file, or `None` when it does not exist (yet).
fn read(path: &PathBuf) -> Result<Option<Json>, EdgeError> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text).map(Some).map_err(|e| {
            EdgeError::new(503, &format!("{} is not valid JSON: {e}", path.display()))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(EdgeError::new(
            503,
            &format!("cannot read {}: {e}", path.display()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let uniq = Box::into_raw(Box::new(0u8)) as usize;
        let dir =
            std::env::temp_dir().join(format!("forge-view-{tag}-{}-{uniq}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_overview_joins_jobs_status_feeds_and_counts() {
        let dir = scratch("ok");
        std::fs::write(
            dir.join("jobs.json"),
            r#"{"version":1,"jobs":[{"id":"weather","every":600}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("status.json"),
            r#"{"generatedAt":5,"jobs":[{"id":"weather","runs":3}]}"#,
        )
        .unwrap();
        let types = parse(r#"[{"apiName":"weather_report","instanceCount":"1,445"}]"#).unwrap();

        // No status files yet: definitions and counts only.
        let bare = ForgeView::new(dir.join("jobs.json"), Some(dir.join("absent.json")), None);
        let o = bare.overview(&types).unwrap();
        assert_eq!(o.get("jobStatus"), Some(&Json::Arr(Vec::new())));
        assert_eq!(o.get("statusAt"), Some(&Json::Null));

        let full = ForgeView::new(dir.join("jobs.json"), Some(dir.join("status.json")), None);
        let o = full.overview(&types).unwrap();
        assert!(
            o.encode().contains(r#""id":"weather","every":600"#),
            "{}",
            o.encode()
        );
        assert!(o.encode().contains(r#""runs":3"#));
        assert_eq!(o.get("statusAt"), Some(&Json::Int(5)));
        assert_eq!(
            o.get("objectCounts").and_then(|c| c.get("weather_report")),
            Some(&Json::Int(1445))
        );

        std::fs::write(dir.join("status.json"), "{not json").unwrap();
        assert_eq!(full.overview(&types).unwrap_err().status, 503);
        let gone = ForgeView::new(dir.join("missing-jobs.json"), None, None);
        assert_eq!(gone.overview(&types).unwrap_err().status, 503);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
