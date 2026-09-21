//! Stage-1 model SBOM: a deterministic **CycloneDX 1.5** document describing the
//! weights, tokenizer, config, and declared lineage of a candidate model. [NFR-OSS-1]
//!
//! This mirrors the suite's software SBOM discipline (see `tools/mg-sbom`): the
//! output is sorted and timestamp-free, so it is itself bit-reproducible and can
//! be signed alongside the weights it describes. It is emitted in the staging
//! enclave and travels inside the signed `.mbundle` across the air gap.
//!
//! The component type is CycloneDX 1.5 `machine-learning-model`; each file blob
//! is a `data` sub-component carrying its SHA-256, and the declared lineage is
//! recorded as `metadata.component.properties`.

use pqc_agility::hash::sha256;

use crate::fetch::FetchedModel;
use crate::provenance::{hex, ProvenanceRecord};

/// Minimal JSON string escaping (quotes + backslashes), matching `mg-sbom`.
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

/// One SBOM file component: an in-bundle path and its SHA-256.
struct FileComponent {
    path: String,
    sha256_hex: String,
}

/// Generate a deterministic CycloneDX 1.5 SBOM for a fetched model. The weights
/// digest is taken from the already-verified [`ProvenanceRecord`] so the SBOM and
/// the signed manifest agree byte-for-byte; auxiliary blobs are hashed here. The
/// component list is sorted by path for reproducibility. [NFR-OSS-1]
pub fn generate_sbom(fetched: &FetchedModel, prov: &ProvenanceRecord) -> String {
    // Assemble every file blob: weights first (from provenance), then aux components.
    let mut files: Vec<FileComponent> = Vec::with_capacity(fetched.components.len() + 1);
    files.push(FileComponent {
        path: "model.weights".to_string(),
        sha256_hex: prov.weights_sha256_hex(),
    });
    for (path, blob) in &fetched.components {
        files.push(FileComponent {
            path: path.clone(),
            sha256_hex: hex(&sha256(blob)),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    // Declared lineage: the upstream source + family/version, surfaced as
    // component properties (immutable provenance carried in the bundle).
    let lineage = format!(
        "{} (family={}, version={})",
        fetched.source.label(),
        fetched.family,
        fetched.version
    );

    let mut s = String::new();
    s.push_str("{\n");
    s.push_str("  \"bomFormat\": \"CycloneDX\",\n");
    s.push_str("  \"specVersion\": \"1.5\",\n");
    s.push_str("  \"version\": 1,\n");
    s.push_str("  \"metadata\": {\n");
    s.push_str("    \"component\": {\n");
    s.push_str("      \"type\": \"machine-learning-model\",\n");
    s.push_str(&format!(
        "      \"name\": \"{}\",\n",
        json_escape(&fetched.proposed_id)
    ));
    s.push_str(&format!(
        "      \"version\": \"{}\",\n",
        json_escape(&fetched.version)
    ));
    s.push_str("      \"properties\": [\n");
    s.push_str(&format!(
        "        {{ \"name\": \"midgard:family\", \"value\": \"{}\" }},\n",
        json_escape(&fetched.family)
    ));
    s.push_str(&format!(
        "        {{ \"name\": \"midgard:upstream_source\", \"value\": \"{}\" }},\n",
        json_escape(&fetched.source.label())
    ));
    s.push_str(&format!(
        "        {{ \"name\": \"midgard:declared_lineage\", \"value\": \"{}\" }}\n",
        json_escape(&lineage)
    ));
    s.push_str("      ]\n");
    s.push_str("    }\n");
    s.push_str("  },\n");
    s.push_str("  \"components\": [\n");
    for (i, f) in files.iter().enumerate() {
        s.push_str("    {\n");
        s.push_str("      \"type\": \"data\",\n");
        s.push_str(&format!("      \"name\": \"{}\",\n", json_escape(&f.path)));
        s.push_str("      \"hashes\": [\n");
        s.push_str(&format!(
            "        {{ \"alg\": \"SHA-256\", \"content\": \"{}\" }}\n",
            f.sha256_hex
        ));
        s.push_str("      ]\n");
        s.push_str("    }");
        if i + 1 < files.len() {
            s.push(',');
        }
        s.push('\n');
    }
    s.push_str("  ]\n");
    s.push_str("}\n");
    s
}
