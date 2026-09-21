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

use std::path::{Path, PathBuf};

/// The sidecar suffix for an artefact's SBOM.
pub const SBOM_SUFFIX: &str = ".sbom.cdx.json";

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
/// Deliberately does not build a JSON tree: nothing downstream needs one, and a
/// parser over attacker-controlled input is attack surface we do not have to
/// carry. The signature — not this function — is what establishes integrity.
pub fn validate(bytes: &[u8]) -> Result<usize, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "not valid UTF-8".to_string())?;

    if !text.contains("\"bomFormat\"") || !text.contains("\"CycloneDX\"") {
        return Err("missing CycloneDX bomFormat".into());
    }
    if !text.contains("\"specVersion\"") {
        return Err("missing specVersion".into());
    }
    if !text.contains("\"components\"") {
        return Err("missing components array".into());
    }

    // Every component must carry a purl, so the scanner can resolve it against
    // advisory data. Counting purls also gives the component count.
    let components = text.matches("\"purl\"").count();
    if components == 0 {
        return Err(
            "no components with a purl — an empty bill of materials is not a bill of materials"
                .into(),
        );
    }
    Ok(components)
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
