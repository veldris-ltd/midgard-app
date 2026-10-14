//! A deterministic **CycloneDX 1.5** SBOM for a deployable artefact — the same
//! discipline as the model SBOM (`mimir/staging`) and `tools/mg-sbom`: sorted and
//! timestamp-free, so it is itself bit-reproducible and can be signed alongside the
//! artefact it describes. [NFR-OSS-1]

/// Minimal JSON string escaping (quotes + control chars), matching `mg-sbom`.
fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            _ => o.push(c),
        }
    }
    o
}

/// Percent-encode a purl name or version segment: everything outside the
/// unreserved set, so a `/`, `@` or `?` in a name cannot change the purl's shape.
fn purl_segment(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'~') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

/// One SBOM dependency component: a name and a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SbomComponent {
    /// Component name.
    pub name: String,
    /// Component version.
    pub version: String,
}

impl SbomComponent {
    /// Build a component entry.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// Generate a deterministic CycloneDX 1.5 SBOM for an artefact and its declared
/// dependency components. Components are sorted by (name, version) for
/// reproducibility. [NFR-OSS-1]
pub fn generate_sbom(
    id: &str,
    version: &str,
    component_kind: &str,
    mut components: Vec<SbomComponent>,
) -> String {
    components.sort_by(|a, b| {
        (a.name.as_str(), a.version.as_str()).cmp(&(b.name.as_str(), b.version.as_str()))
    });

    let mut s = String::new();
    s.push_str("{\n");
    s.push_str("  \"bomFormat\": \"CycloneDX\",\n");
    s.push_str("  \"specVersion\": \"1.5\",\n");
    s.push_str("  \"version\": 1,\n");
    s.push_str("  \"metadata\": {\n");
    s.push_str("    \"component\": {\n");
    s.push_str("      \"type\": \"application\",\n");
    s.push_str(&format!("      \"name\": \"{}\",\n", json_escape(id)));
    s.push_str(&format!(
        "      \"version\": \"{}\",\n",
        json_escape(version)
    ));
    s.push_str("      \"properties\": [\n");
    s.push_str(&format!(
        "        {{ \"name\": \"midgard:component_kind\", \"value\": \"{}\" }}\n",
        json_escape(component_kind)
    ));
    s.push_str("      ]\n");
    s.push_str("    }\n");
    s.push_str("  },\n");
    s.push_str("  \"components\": [\n");
    for (i, c) in components.iter().enumerate() {
        s.push_str("    {\n");
        s.push_str("      \"type\": \"library\",\n");
        s.push_str(&format!("      \"name\": \"{}\",\n", json_escape(&c.name)));
        s.push_str(&format!(
            "      \"version\": \"{}\",\n",
            json_escape(&c.version)
        ));
        // Every component carries a purl, so a scanner can resolve it and the
        // verifier's SBOM gate accepts the document. [AC-B-4]
        s.push_str(&format!(
            "      \"purl\": \"pkg:generic/{}@{}\"\n",
            purl_segment(&c.name),
            purl_segment(&c.version)
        ));
        s.push_str("    }");
        if i + 1 < components.len() {
            s.push(',');
        }
        s.push('\n');
    }
    s.push_str("  ]\n");
    s.push_str("}\n");
    s
}
