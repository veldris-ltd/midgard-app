//! Locating and validating the sibling CycloneDX SBOM.
//!
//! An artefact whose bill of materials is absent or unreadable is never signed
//! and never accepted: an SBOM you cannot parse is an SBOM you cannot scan, and
//! an unscannable dependency graph is the thing the whole gate exists to
//! prevent. `[NFR-OSS-1]` `[AC-B-4]` `[FR-MS-6]`
//!
//! Validation here is structural, not semantic — enough to establish that this
//! really is a CycloneDX document listing components, so that RF-SC-4's scanner
//! has something to work on. Advisory and licence evaluation is that item's job.
//! "Structural" still means *parsed*: the document goes through the strict parser
//! in [`crate::json`], so a truncated file or one that merely mentions the right
//! words is refused. `[AC-RF-SC-1b]`

use std::path::{Path, PathBuf};

use crate::json::{self, Json};

/// The sidecar suffix for an artefact's SBOM.
pub const SBOM_SUFFIX: &str = ".sbom.cdx.json";

/// The one CycloneDX version this gate accepts — the one `mg-sbom` and Bifrost's
/// generator emit. A document in any other version is refused rather than read
/// under rules written for this one.
pub const SPEC_VERSION: &str = "1.5";

/// A loaded, structurally-valid SBOM.
pub struct Sbom {
    /// Where it was read from.
    pub path: PathBuf,
    /// The exact bytes, which are what the signature binds.
    pub bytes: Vec<u8>,
    /// How many components it lists.
    pub components: usize,
}

/// The path of the SBOM belonging to `artefact`.
pub fn sbom_path(artefact: &Path) -> PathBuf {
    let mut s = artefact.as_os_str().to_os_string();
    s.push(SBOM_SUFFIX);
    PathBuf::from(s)
}

/// Structural validation. Returns the component count, or the reason it is not
/// a usable CycloneDX document.
///
/// The document must parse as strict JSON and be an object declaring
/// `bomFormat: "CycloneDX"`, a `specVersion`, and a non-empty top-level
/// `components` array in which **every** entry carries a purl — a component the
/// scanner cannot resolve against advisory data is a component nobody checked.
/// Integrity is still the signature's job, not this function's.
pub fn validate(bytes: &[u8]) -> Result<usize, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "not valid UTF-8".to_string())?;
    let doc = json::parse(text).map_err(|e| format!("not valid JSON: {e}"))?;

    if doc.get("bomFormat").and_then(Json::as_str) != Some("CycloneDX") {
        return Err("missing CycloneDX bomFormat".into());
    }
    match doc.get("specVersion").and_then(Json::as_str) {
        Some(SPEC_VERSION) => {}
        Some(other) => {
            return Err(format!(
                "unsupported specVersion {other:?} (this gate accepts CycloneDX {SPEC_VERSION})"
            ))
        }
        None => return Err("missing specVersion".into()),
    }
    let components = doc
        .get("components")
        .and_then(Json::as_array)
        .ok_or("missing components array")?;
    if components.is_empty() {
        return Err("no components — an empty bill of materials is not a bill of materials".into());
    }
    for (i, c) in components.iter().enumerate() {
        if !matches!(c, Json::Obj(_)) {
            return Err(format!("component {i} is not an object"));
        }
        if !c
            .get("purl")
            .and_then(Json::as_str)
            .is_some_and(|p| !p.is_empty())
        {
            let name = c.get("name").and_then(Json::as_str).unwrap_or("unnamed");
            return Err(format!("component {i} ({name}) has no purl"));
        }
    }
    Ok(components.len())
}

/// Load and validate the SBOM beside `artefact`.
pub fn load(artefact: &Path) -> Result<Sbom, LoadError> {
    let path = sbom_path(artefact);
    let bytes = std::fs::read(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            LoadError::Missing(path.clone())
        } else {
            LoadError::Invalid(path.clone(), e.to_string())
        }
    })?;
    let components = validate(&bytes).map_err(|r| LoadError::Invalid(path.clone(), r))?;
    Ok(Sbom {
        path,
        bytes,
        components,
    })
}

/// Why an SBOM could not be loaded. Kept separate from `SignError`/`VerifyError`
/// so both can map it to their own vocabulary.
#[derive(Debug)]
pub enum LoadError {
    /// No SBOM at the expected path.
    Missing(PathBuf),
    /// Present but unusable.
    Invalid(PathBuf, String),
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{
  "bomFormat": "CycloneDX",
  "specVersion": "1.5",
  "version": 1,
  "components": [
    { "type": "library", "name": "aes-gcm", "version": "0.10.3", "purl": "pkg:cargo/aes-gcm@0.10.3" }
  ]
}"#;

    #[test]
    fn accepts_an_mg_sbom_document() {
        assert_eq!(validate(GOOD.as_bytes()).unwrap(), 1);
    }

    #[test]
    fn rejects_a_document_that_is_not_cyclonedx() {
        assert!(validate(br#"{"hello":"world"}"#).is_err());
    }

    #[test]
    fn rejects_an_empty_component_list() {
        let empty = r#"{"bomFormat":"CycloneDX","specVersion":"1.5","components":[]}"#;
        assert!(validate(empty.as_bytes()).is_err());
    }

    #[test]
    fn rejects_components_without_purls() {
        let no_purl = r#"{"bomFormat":"CycloneDX","specVersion":"1.5",
            "components":[{"type":"library","name":"x","version":"1"}]}"#;
        assert!(validate(no_purl.as_bytes()).is_err());
    }

    #[test]
    fn rejects_any_other_cyclonedx_version() {
        for v in ["1.4", "1.6", "", "1.5 "] {
            let doc = GOOD.replace("\"1.5\"", &format!("\"{v}\""));
            assert!(validate(doc.as_bytes()).is_err(), "{v:?} must be refused");
        }
        // A number is not the string the spec requires.
        let doc = GOOD.replace("\"1.5\"", "1.5");
        assert!(validate(doc.as_bytes()).is_err());
    }

    #[test]
    fn rejects_a_single_component_without_a_purl() {
        // One resolvable component must not vouch for the rest.
        let mixed = r#"{"bomFormat":"CycloneDX","specVersion":"1.5","components":[
            {"type":"library","name":"a","version":"1","purl":"pkg:cargo/a@1"},
            {"type":"library","name":"b","version":"1"}]}"#;
        let e = validate(mixed.as_bytes()).unwrap_err();
        assert!(e.contains("component 1 (b)"), "{e}");
    }

    #[test]
    fn rejects_malformed_json_that_mentions_the_right_words() {
        // Every one of these contains all the strings a substring check looks for.
        let truncated = &GOOD[..GOOD.len() - 1];
        let wrong_places = r#"["bomFormat", "CycloneDX", "specVersion", "components", "purl"]"#;
        let nested = r#"{"x": {"bomFormat":"CycloneDX","specVersion":"1.5",
            "components":[{"purl":"pkg:cargo/a@1"}]}}"#;
        let trailing = format!("{GOOD} garbage");
        let duplicate = r#"{"bomFormat":"CycloneDX","specVersion":"1.5",
            "components":[{"purl":"pkg:cargo/a@1"}],"components":[]}"#;
        for (tag, doc) in [
            ("truncated", truncated),
            ("wrong places", wrong_places),
            ("nested", nested),
            ("trailing data", trailing.as_str()),
            ("duplicate key", duplicate),
        ] {
            assert!(validate(doc.as_bytes()).is_err(), "{tag} must be refused");
        }
    }

    #[test]
    fn a_purl_outside_components_does_not_count() {
        // mg-sbom's `metadata.component` describes the artefact itself and must
        // not inflate the count.
        let doc = r#"{"bomFormat":"CycloneDX","specVersion":"1.5",
            "metadata":{"component":{"name":"app","purl":"pkg:generic/app"}},
            "components":[{"name":"a","purl":"pkg:cargo/a@1"}]}"#;
        assert_eq!(validate(doc.as_bytes()).unwrap(), 1);
    }

    #[test]
    fn rejects_non_utf8() {
        assert!(validate(&[0xff, 0xfe, 0xfd]).is_err());
    }

    #[test]
    fn sidecar_path_appends_rather_than_replacing_the_extension() {
        // `target/release/mg-sbom.exe` must look for `mg-sbom.exe.sbom.cdx.json`,
        // not `mg-sbom.sbom.cdx.json` — otherwise Windows and Unix disagree on
        // which file the signature covers.
        let p = sbom_path(Path::new("target/release/mg-sbom.exe"));
        assert!(p.to_string_lossy().ends_with("mg-sbom.exe.sbom.cdx.json"));
    }
}
