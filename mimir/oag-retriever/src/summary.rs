//! The ontology summary — the `{{ONTOLOGY_SUMMARY}}` the A.2 planner grounds
//! against. It is built from the fabric's type registry, so the planner can only
//! ever reference **types that actually exist** (the first A.2 rule). [FR-MI-2]

use std::collections::BTreeSet;

use fabric::TypeRegistry;

/// A summarised object type: its ids and the property names it declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeCard {
    /// Stable type id (the query `object_type`).
    pub id: String,
    /// Api name.
    pub api_name: String,
    /// Declared property names.
    pub properties: Vec<String>,
}

/// A summarised link type usable for search-around.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkCard {
    /// Stable link type id (the query `link_traversals` entry).
    pub id: String,
    /// Api name.
    pub api_name: String,
    /// Source object-type id.
    pub source: String,
    /// Target object-type id.
    pub target: String,
}

/// The types available to ground against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OntologySummary {
    /// Object types.
    pub objects: Vec<TypeCard>,
    /// Link types.
    pub links: Vec<LinkCard>,
}

impl OntologySummary {
    /// Build the summary from the live type registry (deterministic order).
    pub fn from_registry(reg: &TypeRegistry) -> Self {
        let objects = reg
            .object_types()
            .map(|ot| TypeCard {
                id: ot.id.clone(),
                api_name: ot.api_name.clone(),
                properties: ot.properties.iter().map(|p| p.api_name.clone()).collect(),
            })
            .collect();
        let links = reg
            .link_types()
            .map(|lt| LinkCard {
                id: lt.id.clone(),
                api_name: lt.api_name.clone(),
                source: lt.source_type.clone(),
                target: lt.target_type.clone(),
            })
            .collect();
        Self { objects, links }
    }

    /// Whether an object type id exists in the summary.
    pub fn has_object(&self, id: &str) -> bool {
        self.objects.iter().any(|o| o.id == id)
    }

    /// The link type ids whose **both** endpoints are in `matched` — the
    /// search-around edges between types the task actually referenced.
    pub fn links_within(&self, matched: &BTreeSet<String>) -> Vec<LinkCard> {
        self.links
            .iter()
            .filter(|l| matched.contains(&l.source) && matched.contains(&l.target))
            .cloned()
            .collect()
    }

    /// Render as the `{{ONTOLOGY_SUMMARY}}` block for the A.2 prompt.
    pub fn render(&self) -> String {
        let mut s = String::from("OBJECT TYPES:\n");
        for o in &self.objects {
            s.push_str(&format!(
                "- {} ({}): {}\n",
                o.id,
                o.api_name,
                o.properties.join(", ")
            ));
        }
        s.push_str("LINK TYPES:\n");
        for l in &self.links {
            s.push_str(&format!(
                "- {} ({}): {} -> {}\n",
                l.id, l.api_name, l.source, l.target
            ));
        }
        s
    }
}
