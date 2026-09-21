//! The query engine: typed object queries, **search-around** (link traversal),
//! all under the policy point. Every candidate object is authorised individually
//! against the caller's clearance, purpose, and roles, so a result set only ever
//! contains objects the caller may read — dynamic security at query time.
//! [DD-1] [NFR-SEC-2]

use std::collections::{BTreeMap, BTreeSet};

use audit_writer::LedgerStore;
use governance::{GovContext, Governance};
use policy::{Resource, ResourceId};

use crate::error::FabricError;
use crate::object::ObjectInstance;
use crate::store::ObjectStore;
use crate::types::TypeRegistry;
use crate::value::Value;

/// A typed query (mirrors the `ObjectFabric.Query` proto surface).
#[derive(Debug, Clone)]
pub struct Query {
    /// The object type to query.
    pub object_type: String,
    /// Equality filters (property api name → value string, parsed against schema).
    pub filter: BTreeMap<String, String>,
    /// Link types to traverse from each match (search-around).
    pub link_traversals: Vec<String>,
    /// Maximum number of objects to return.
    pub max_results: usize,
}

/// The read-authorization check for one object: mandatory marking dominance +
/// purpose gate + role rule, via the governance point's non-auditing fast path
/// (the query as a whole is audited once by the caller). Reused for base results
/// and for traversed neighbours.
fn can_read<L: LedgerStore>(gov: &Governance<L>, ctx: &GovContext, obj: &ObjectInstance) -> bool {
    let resource = Resource {
        id: ResourceId(obj.id.clone()),
        marking: obj.marking.clone(),
        allowed_purposes: obj.allowed_purposes.clone(),
    };
    gov.permits(ctx, &resource, "Read")
}

/// Run a query. Uses the inverted index for the first equality filter, then
/// applies the rest as predicates, authorises each survivor, and expands by any
/// requested link traversals. Fails closed on an unknown object type.
pub fn run_query<L: LedgerStore>(
    registry: &TypeRegistry,
    store: &dyn ObjectStore,
    gov: &Governance<L>,
    ctx: &GovContext,
    q: &Query,
) -> Result<Vec<ObjectInstance>, FabricError> {
    let ot = registry
        .object_type(&q.object_type)
        .ok_or_else(|| FabricError::UnknownObjectType(q.object_type.clone()))?;

    // Parse each filter against the schema. An unknown property or an unparsable
    // value can match nothing, so we return an empty set (fail closed).
    let mut filters: Vec<(String, String)> = Vec::new();
    for (prop, val) in &q.filter {
        let Some(pt) = ot.property(prop) else {
            return Ok(Vec::new());
        };
        let Some(v) = Value::parse(pt.value_type, val) else {
            return Ok(Vec::new());
        };
        filters.push((prop.clone(), v.as_index_key()));
    }

    // Candidate set: inverted index on the first filter, else the whole type.
    let mut candidates: Vec<&ObjectInstance> = match filters.first() {
        Some((p0, k0)) => store.by_property(&q.object_type, p0, k0),
        None => store.objects_of_type(&q.object_type),
    };
    // Remaining filters as predicates (intersection).
    for (p, k) in filters.iter().skip(1) {
        candidates.retain(|o| o.properties.get(p).map(|v| v.as_index_key()) == Some(k.clone()));
    }

    // Authorise base results.
    let base: Vec<&ObjectInstance> = candidates
        .into_iter()
        .filter(|o| can_read(gov, ctx, o))
        .collect();

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<ObjectInstance> = Vec::new();
    for o in &base {
        if seen.insert(o.id.clone()) {
            out.push((*o).clone());
        }
    }

    // Search-around: authorised neighbours over the requested link types.
    if !q.link_traversals.is_empty() {
        for o in &base {
            for lt in &q.link_traversals {
                for n in store.neighbors(&o.id, lt) {
                    if can_read(gov, ctx, n) && seen.insert(n.id.clone()) {
                        out.push(n.clone());
                    }
                }
            }
        }
    }

    out.truncate(q.max_results);
    Ok(out)
}
