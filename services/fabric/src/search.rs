//! Searching one object type by its properties: comparisons beyond equality, text
//! matching, ordering and paging — all under the same policy point as a query. Every
//! candidate is authorised individually, so the matches, their count and every page
//! contain only objects the caller may read. [DD-1] [NFR-SEC-2]

use std::cmp::Ordering;

use audit_writer::LedgerStore;
use governance::{GovContext, Governance};
use policy::{Resource, ResourceId};

use crate::error::FabricError;
use crate::object::ObjectInstance;
use crate::store::ObjectStore;
use crate::types::TypeRegistry;
use crate::value::{Value, ValueType};

/// The pseudo-property a [`CompareOp::Contains`] condition names to match any text
/// property of the object.
pub const ANY_TEXT: &str = "*";

/// How a condition compares a property with its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    /// Equal.
    Eq,
    /// Not equal; an object without the property also matches.
    Ne,
    /// Less than.
    Lt,
    /// Less than or equal.
    Le,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Ge,
    /// Text contains the value, ignoring case. Text properties only.
    Contains,
}

impl CompareOp {
    /// Parse the wire name (`eq`, `ne`, `lt`, `lte`, `gt`, `gte`, `contains`).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "eq" => Self::Eq,
            "ne" => Self::Ne,
            "lt" => Self::Lt,
            "lte" => Self::Le,
            "gt" => Self::Gt,
            "gte" => Self::Ge,
            "contains" => Self::Contains,
            _ => return None,
        })
    }
}

/// One condition on a property. `property` may be [`ANY_TEXT`] with `Contains`.
#[derive(Debug, Clone)]
pub struct Condition {
    /// The property's api name.
    pub property: String,
    /// The comparison.
    pub op: CompareOp,
    /// The value, parsed against the property's type.
    pub value: String,
}

/// A search of one object type: every condition must hold.
#[derive(Debug, Clone)]
pub struct Search {
    /// The object type to search.
    pub object_type: String,
    /// Conditions, all of which must hold.
    pub conditions: Vec<Condition>,
    /// The property to order by; objects without it come last. Ties, and no
    /// ordering at all, fall back to the object id, so pages are stable.
    pub order_by: Option<String>,
    /// Order largest first.
    pub descending: bool,
    /// Matches to skip, for paging.
    pub offset: usize,
    /// Most matches to return.
    pub limit: usize,
}

/// One page of a search.
#[derive(Debug, Clone)]
pub struct SearchPage {
    /// The matches on this page.
    pub results: Vec<ObjectInstance>,
    /// How many readable objects match in all.
    pub total: usize,
    /// The audit entry recording the read decision.
    pub audit_seq: u64,
}

/// Why a search cannot be run as asked: the caller named something the type does
/// not have, or a value its property cannot hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchProblem(pub String);

/// Check a search against the schema: every property exists (or is [`ANY_TEXT`] with
/// `Contains`), every value parses as its property's type, `Contains` is used only on
/// text, and the ordering property exists. Fails on an unknown object type.
pub fn check(registry: &TypeRegistry, s: &Search) -> Result<(), SearchProblem> {
    let ot = registry
        .object_type(&s.object_type)
        .ok_or_else(|| SearchProblem(format!("unknown object type: {}", s.object_type)))?;
    for c in &s.conditions {
        if c.property == ANY_TEXT {
            if c.op != CompareOp::Contains {
                return Err(SearchProblem(format!(
                    "\"{ANY_TEXT}\" (any text property) works only with contains"
                )));
            }
            continue;
        }
        let pt = ot.property(&c.property).ok_or_else(|| {
            SearchProblem(format!("{} has no property {}", s.object_type, c.property))
        })?;
        if c.op == CompareOp::Contains && pt.value_type != ValueType::Text {
            return Err(SearchProblem(format!(
                "contains works on text, and {} is {}",
                c.property,
                pt.value_type.as_str()
            )));
        }
        if Value::parse(pt.value_type, &c.value).is_none() {
            return Err(SearchProblem(format!(
                "{:?} is not a valid {} for {}",
                c.value,
                pt.value_type.as_str(),
                c.property
            )));
        }
    }
    if let Some(p) = &s.order_by {
        if ot.property(p).is_none() {
            return Err(SearchProblem(format!(
                "{} has no property {p} to order by",
                s.object_type
            )));
        }
    }
    Ok(())
}

fn can_read<L: LedgerStore>(gov: &Governance<L>, ctx: &GovContext, obj: &ObjectInstance) -> bool {
    let resource = Resource {
        id: ResourceId(obj.id.clone()),
        marking: obj.marking.clone(),
        allowed_purposes: obj.allowed_purposes.clone(),
    };
    gov.permits(ctx, &resource, "Read")
}

/// Order two values of one property: numbers numerically, text and the rest by their
/// natural order. Values of different types do not compare.
fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y),
        (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
        (Value::Timestamp(x), Value::Timestamp(y)) => Some(x.cmp(y)),
        (Value::Text(x), Value::Text(y)) => Some(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

/// Whether `o` meets `c`, given the condition's value parsed for its property.
fn holds(o: &ObjectInstance, c: &Condition, want: Option<&Value>) -> bool {
    if c.property == ANY_TEXT {
        let needle = c.value.to_lowercase();
        return o
            .properties
            .values()
            .any(|v| matches!(v, Value::Text(t) if t.to_lowercase().contains(&needle)));
    }
    let Some(want) = want else { return false };
    let Some(have) = o.properties.get(&c.property) else {
        return c.op == CompareOp::Ne;
    };
    if c.op == CompareOp::Contains {
        return matches!((have, want), (Value::Text(h), Value::Text(w)) if h.to_lowercase().contains(&w.to_lowercase()));
    }
    let Some(ord) = compare(have, want) else {
        return c.op == CompareOp::Ne;
    };
    match c.op {
        CompareOp::Eq => ord == Ordering::Equal,
        CompareOp::Ne => ord != Ordering::Equal,
        CompareOp::Lt => ord == Ordering::Less,
        CompareOp::Le => ord != Ordering::Greater,
        CompareOp::Gt => ord == Ordering::Greater,
        CompareOp::Ge => ord != Ordering::Less,
        CompareOp::Contains => false,
    }
}

/// Run a search the caller may make (see [`check`]; anything it would refuse matches
/// nothing here, failing closed). Uses the inverted index for a first equality
/// condition, then applies every condition, authorises each match, orders, and
/// returns the requested page with the total.
pub fn run_search<L: LedgerStore>(
    registry: &TypeRegistry,
    store: &dyn ObjectStore,
    gov: &Governance<L>,
    ctx: &GovContext,
    s: &Search,
) -> Result<(Vec<ObjectInstance>, usize), FabricError> {
    let ot = registry
        .object_type(&s.object_type)
        .ok_or_else(|| FabricError::UnknownObjectType(s.object_type.clone()))?;
    if check(registry, s).is_err() {
        return Ok((Vec::new(), 0));
    }
    let wants: Vec<Option<Value>> = s
        .conditions
        .iter()
        .map(|c| {
            ot.property(&c.property)
                .and_then(|pt| Value::parse(pt.value_type, &c.value))
        })
        .collect();

    let first_eq = s
        .conditions
        .iter()
        .zip(&wants)
        .find(|(c, w)| c.op == CompareOp::Eq && w.is_some());
    let candidates: Vec<&ObjectInstance> = match first_eq {
        Some((c, Some(w))) => store.by_property(&s.object_type, &c.property, &w.as_index_key()),
        _ => store.objects_of_type(&s.object_type),
    };

    let mut matches: Vec<&ObjectInstance> = candidates
        .into_iter()
        .filter(|o| {
            s.conditions
                .iter()
                .zip(&wants)
                .all(|(c, w)| holds(o, c, w.as_ref()))
        })
        .filter(|o| can_read(gov, ctx, o))
        .collect();

    matches.sort_by(|a, b| {
        let by_prop = s.order_by.as_ref().map_or(Ordering::Equal, |p| {
            match (a.properties.get(p), b.properties.get(p)) {
                (Some(x), Some(y)) => {
                    let ord = compare(x, y).unwrap_or(Ordering::Equal);
                    if s.descending {
                        ord.reverse()
                    } else {
                        ord
                    }
                }
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
        });
        by_prop.then_with(|| a.id.cmp(&b.id))
    });

    let total = matches.len();
    let page = matches
        .into_iter()
        .skip(s.offset)
        .take(s.limit)
        .cloned()
        .collect();
    Ok((page, total))
}
