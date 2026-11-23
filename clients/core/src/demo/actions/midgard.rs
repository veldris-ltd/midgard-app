//! Midgard's actions in the sample mode, ported method by method from the release's
//! `MockGateway` (`sdk/ts/src/mock/MockGateway.ts`), which is what the rc.3 web console
//! runs on in mock mode. `[FR-C-4]` `[AC-C-13]` `[NFR-SEC-2]` `[NFR-AI-1]` `[DD-1]`
//!
//! Each runs inside the demo enclosure's action pipeline, so the core's rules hold
//! around it: the target must be a row (or, for a computed read, a collection) the
//! persona may see, every outcome is decided once and audited in `midgard.audit`
//! (numbered `AUD-1043`, `AUD-1044`, … and stamped from `14:32:10` in steps of 2 s,
//! as the mock numbers its rows), and only the persona signed in acts.
//!
//! | Verb | Target | Port of |
//! |---|---|---|
//! | `midgard.requestApproval` (`actionType`) | `midgard.objects/<type>/<id>` | `requestApproval` |
//! | `midgard.decideApproval` (`approve`) | `midgard.approvals/<APR-n>` | `decideApproval` |
//! | `midgard.applyAction` (`actionType`, `humanGateToken`) | `midgard.objects/<type>/<id>` | `applyAction` |
//! | `midgard.query` (`where`, `orderBy`, `descending`, `offset`, `maxResults`) | `midgard.objectTypes/<apiName>` | `query` |
//! | `midgard.searchAround` (`seedId`, `hops`) | `midgard.objectTypes/<apiName>` | `searchAround` |
//! | `midgard.simulatePolicy` (`purpose`, `marking`) | `midgard.policies/*` | `simulatePolicy` |
//!
//! Where the port knowingly differs from the mock (each for governance, or because the
//! apps have the personas the mock has not):
//! - **The second person is a persona.** The mock has one session, so a simulated
//!   "approver" approves every request after 3 s. Here a request waits for another
//!   persona to decide it (`decideApproval`), and the requester deciding their own is
//!   refused with the separation-of-duties wording of the release's Admin mock.
//! - **An unknown action type is refused**, where the mock's `applyAction` would apply
//!   it (with the wording of the mock's own `requestApproval` refusal).
//! - **Conditions apply.** The mock accepts and ignores a query's conditions and
//!   ordering; the port applies them to each object's properties as the gateway's
//!   fabric search does (`services/fabric/src/search.rs`: `*` is any text property,
//!   `contains` ignores case).
//! - **Counts cover what the persona may read.** A query returns only objects the
//!   persona may see, and a search-around withholds the nodes above the persona's
//!   level (counted in `withheld`); the seed node carries its own type's name and
//!   marking, where the mock calls every seed an aircraft.
//! - **A policy dry run** evaluates the rules the persona may read, with the
//!   persona's own subject and roles (the mock always uses its default subject).

use attest::json::{parse, Json};

use crate::demo::enclosure::{
    ActionRequest, Collection, DemoState, MockAuditClock, Outcome, StoredRow,
};
use crate::marking::{may_see, MarkingLevel, RowMarking};
use crate::source::Grant;

/// The runtime collection of approval requests (`Approval` rows).
pub const APPROVALS: &str = "midgard.approvals";
/// The read model of [`APPROVALS`].
pub const APPROVAL_TYPE: &str = "MidgardApproval";
/// How long an approved human gate can be redeemed, in seconds (`APPROVAL_TTL`).
pub const APPROVAL_TTL: u64 = 900;
/// `MockGateway`'s audit numbering: ids after `AUD-1042` (`1042 + seq - 5`), stamps
/// after `14:32:08`, each `1 + floor(180 / 100)` = 2 s later.
pub const MIDGARD_CLOCK: MockAuditClock = MockAuditClock {
    prefix: "AUD-",
    seq: 1042,
    clock: 14 * 3600 + 32 * 60 + 8,
    step: 2,
};

const OBJECTS: &str = "midgard.objects";
const TYPES: &str = "midgard.objectTypes";
const POLICIES: &str = "midgard.policies";
/// The pseudo-property for "any text property" (`ANY_TEXT`).
const ANY_TEXT: &str = "*";
/// The most hops a search-around walks (the web console offers 1 to 4).
const MAX_HOPS: u64 = 4;
/// The most objects one query page returns (the gateway's own cap).
const MAX_RESULTS: usize = 500;

pub(super) fn param<'a>(req: &'a ActionRequest, key: &str) -> Option<&'a str> {
    req.params
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

pub(super) fn text(item: &Json, key: &str) -> Option<String> {
    item.get(key).and_then(Json::as_str).map(str::to_string)
}

pub(super) fn num(n: u64) -> Json {
    Json::Num(n.to_string())
}

pub(super) fn obj(pairs: Vec<(&str, Json)>) -> Json {
    Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// Set `key` of the object `item` to `value`, replacing or adding it.
pub(super) fn set(item: &mut Json, key: &str, value: Json) {
    if let Json::Obj(pairs) = item {
        match pairs.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value,
            None => pairs.push((key.to_string(), value)),
        }
    }
}

/// `target` as `(collection, id)` when it names `collection`.
pub(super) fn in_collection<'a>(target: &'a str, collection: &str) -> Option<&'a str> {
    target
        .split_once('/')
        .filter(|(c, _)| *c == collection)
        .map(|(_, id)| id)
}

/// The object type declaring `action` and whether that action is human-gated
/// (`OBJECT_TYPES.find(t => t.actions.some(a => a.name === action))`).
fn action_type(state: &DemoState, action: &str) -> Option<(String, bool)> {
    state.collections.get(TYPES)?.rows.iter().find_map(|t| {
        let found = t
            .item
            .get("actions")?
            .as_array()?
            .iter()
            .find(|a| a.get("name").and_then(Json::as_str) == Some(action))?;
        Some((
            text(&t.item, "apiName").unwrap_or_default(),
            found.get("humanGated").and_then(Json::as_bool) == Some(true),
        ))
    })
}

/// The mock's `oneTarget` refusal.
fn one_target(api_name: &str) -> Outcome {
    Outcome::Refused(format!(
        "an action applies to one object: name it as \"target\":\"{api_name}/<id>\""
    ))
}

/// An approval's status now: an approved gate past its expiry has expired (`status`).
fn status_now(item: &Json, now: u64) -> String {
    let status = text(item, "status").unwrap_or_default();
    let expires = item
        .get("expiresAt")
        .and_then(|v| match v {
            Json::Num(t) => t.parse::<u64>().ok(),
            _ => None,
        })
        .unwrap_or(0);
    if status == "approved" && expires <= now {
        "expired".into()
    } else {
        status
    }
}

/// `requestApproval(actionType, target)`: a pending request for a second person.
pub fn request_approval(state: &mut DemoState, grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(object) = in_collection(&req.target, OBJECTS) else {
        return Outcome::Refused("an approval is requested on midgard.objects/<type>/<id>".into());
    };
    let action = param(req, "actionType").unwrap_or("");
    let Some((api_name, gated)) = action_type(state, action) else {
        return Outcome::Refused(format!("unknown action type: {action}"));
    };
    if !gated {
        return Outcome::Refused(format!("action \"{action}\" needs no approval"));
    }
    let Some(marking) = state.row(OBJECTS, object).map(|r| r.marking.clone()) else {
        return one_target(&api_name);
    };
    let now = state.now_secs();
    let coll = state
        .collections
        .entry(APPROVALS.to_string())
        .or_insert_with(|| Collection {
            type_name: APPROVAL_TYPE.into(),
            rows: Vec::new(),
        });
    let id = format!("APR-{}", coll.rows.len() + 1);
    let item = obj(vec![
        ("id", Json::Str(id.clone())),
        ("actionType", Json::Str(action.into())),
        ("target", Json::Str(object.into())),
        ("requester", Json::Str(grant.persona().id.clone())),
        ("requestedAt", num(now)),
        ("status", Json::Str("pending".into())),
        // Whose it is depends on who reads it: the screen works that out.
        ("mine", Json::Bool(false)),
    ]);
    // Newest first, as `listApprovals` returns them.
    coll.rows.insert(
        0,
        StoredRow {
            id,
            marking,
            item: item.clone(),
            signature: None,
        },
    );
    let mut answer = item;
    set(&mut answer, "mine", Json::Bool(true));
    Outcome::Applied(answer)
}

/// `decideApproval(id, approve)`, made by a second persona.
pub fn decide_approval(state: &mut DemoState, grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(id) = in_collection(&req.target, APPROVALS) else {
        return Outcome::Refused("an approval is decided on midgard.approvals/<id>".into());
    };
    let approve = param(req, "approve") == Some("true");
    let now = state.now_secs();
    let me = grant.persona().id.clone();
    let Some(row) = state.row_mut(APPROVALS, id) else {
        return Outcome::Refused(format!("unknown approval: {id}"));
    };
    let status = status_now(&row.item, now);
    if status != "pending" {
        return Outcome::Conflict(format!("approval {id} is already {status}"));
    }
    // Separation of duties (the release's Admin mock): never the requester.
    if text(&row.item, "requester").as_deref() == Some(me.as_str()) {
        return Outcome::Refused(
            "separation-of-duties: requester cannot approve own request".into(),
        );
    }
    set(&mut row.item, "approver", Json::Str(me));
    if approve {
        set(&mut row.item, "status", Json::Str("approved".into()));
        set(&mut row.item, "expiresAt", num(now + APPROVAL_TTL));
    } else {
        set(&mut row.item, "status", Json::Str("rejected".into()));
    }
    Outcome::Applied(row.item.clone())
}

/// `applyAction({actionType, params: {target}, humanGateToken})`: one object; a
/// human-gated action only with a current approval of it, requested by this persona.
pub fn apply_action(state: &mut DemoState, grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(object) = in_collection(&req.target, OBJECTS) else {
        return Outcome::Refused("an action is applied on midgard.objects/<type>/<id>".into());
    };
    let action = param(req, "actionType").unwrap_or("");
    let Some((api_name, gated)) = action_type(state, action) else {
        return Outcome::Refused(format!("unknown action type: {action}"));
    };
    if state.row(OBJECTS, object).is_none() {
        return one_target(&api_name);
    }
    if gated {
        let Some(token) = param(req, "humanGateToken").filter(|t| !t.is_empty()) else {
            return Outcome::Refused(format!("Human gate required for action \"{action}\""));
        };
        let now = state.now_secs();
        let me = grant.persona().id.as_str();
        let current = state.row(APPROVALS, token).is_some_and(|a| {
            text(&a.item, "actionType").as_deref() == Some(action)
                && text(&a.item, "target").as_deref() == Some(object)
                && text(&a.item, "requester").as_deref() == Some(me)
                && status_now(&a.item, now) == "approved"
        });
        if !current {
            return Outcome::Refused(format!(
                "Human gate token is not a current approval of \"{action}\" on {object} for this user"
            ));
        }
        if let Some(a) = state.row_mut(APPROVALS, token) {
            set(&mut a.item, "status", Json::Str("used".into()));
        }
    }
    Outcome::Applied(obj(vec![("applied", Json::Bool(true))]))
}

/// One comparison of a query condition.
#[derive(Clone, Copy, PartialEq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    Contains,
}

impl Op {
    fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "eq" => Op::Eq,
            "ne" => Op::Ne,
            "lt" => Op::Lt,
            "lte" => Op::Lte,
            "gt" => Op::Gt,
            "gte" => Op::Gte,
            "contains" => Op::Contains,
            _ => return None,
        })
    }
}

struct Condition {
    property: String,
    op: Op,
    value: String,
}

/// Order two property values: as numbers when both are, otherwise as text.
fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        _ => a.cmp(b),
    }
}

fn holds(c: &Condition, values: &Json) -> bool {
    let Json::Obj(pairs) = values else {
        return false;
    };
    if c.property == ANY_TEXT {
        let needle = c.value.to_lowercase();
        return pairs
            .iter()
            .any(|(_, v)| matches!(v, Json::Str(t) if t.to_lowercase().contains(&needle)));
    }
    let Some(have) = values.get(&c.property).and_then(Json::as_str) else {
        return false;
    };
    use std::cmp::Ordering::{Equal, Greater, Less};
    match c.op {
        Op::Contains => have.to_lowercase().contains(&c.value.to_lowercase()),
        Op::Eq => compare(have, &c.value) == Equal,
        Op::Ne => compare(have, &c.value) != Equal,
        Op::Lt => compare(have, &c.value) == Less,
        Op::Lte => compare(have, &c.value) != Greater,
        Op::Gt => compare(have, &c.value) == Greater,
        Op::Gte => compare(have, &c.value) != Less,
    }
}

/// The conditions of a query's `where` parameter (a JSON list of `{property, op,
/// value}`), checked against the type's properties as the fabric checks them.
fn conditions(req: &ActionRequest, type_item: &Json) -> Result<Vec<Condition>, String> {
    let Some(text_where) = param(req, "where").filter(|w| !w.is_empty()) else {
        return Ok(Vec::new());
    };
    let Ok(Json::Arr(items)) = parse(text_where) else {
        return Err("\"where\" must be a list".into());
    };
    let known: Vec<String> = type_item
        .get("properties")
        .and_then(Json::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|p| text(p, "name"))
        .collect();
    items
        .iter()
        .map(|item| {
            let field = |k: &str| text(item, k).unwrap_or_default();
            let op = Op::parse(&field("op")).ok_or_else(|| {
                format!(
                    "unknown operator {:?}: use eq, ne, lt, lte, gt, gte or contains",
                    field("op")
                )
            })?;
            let property = field("property");
            if property == ANY_TEXT && op != Op::Contains {
                return Err(format!(
                    "\"{ANY_TEXT}\" (any text property) works only with contains"
                ));
            }
            if property != ANY_TEXT && !known.contains(&property) {
                return Err(format!("unknown property {property:?}"));
            }
            Ok(Condition {
                property,
                op,
                value: field("value"),
            })
        })
        .collect()
}

fn count_param(req: &ActionRequest, key: &str, default: usize) -> usize {
    param(req, key)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
}

/// `query({objectType, where, orderBy, descending, offset, maxResults})`: the type's
/// objects the persona may read that meet every condition, ordered and paged.
pub fn query(state: &mut DemoState, grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(api_name) = in_collection(&req.target, TYPES) else {
        return Outcome::Refused("a query names its type as midgard.objectTypes/<apiName>".into());
    };
    let Some(type_row) = state.row(TYPES, api_name) else {
        return Outcome::Refused(format!("unknown object type: {api_name}"));
    };
    let conds = match conditions(req, &type_row.item) {
        Ok(c) => c,
        Err(why) => return Outcome::Refused(why),
    };
    let order_by = param(req, "orderBy").filter(|p| !p.is_empty());
    let descending = param(req, "descending") == Some("true");
    let offset = count_param(req, "offset", 0);
    let limit = count_param(req, "maxResults", 5).min(MAX_RESULTS);
    let mut found: Vec<&StoredRow> = state
        .collections
        .get(OBJECTS)
        .map(|c| c.rows.iter().collect())
        .unwrap_or_default();
    found.retain(|r| {
        text(&r.item, "typeId").as_deref() == Some(api_name)
            && may_see(grant.clearance(), &r.marking)
            && r.item
                .get("propertyValues")
                .is_some_and(|v| conds.iter().all(|c| holds(c, v)))
    });
    let key = |r: &StoredRow| match order_by {
        Some(p) => r
            .item
            .get("propertyValues")
            .and_then(|v| v.get(p))
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string(),
        None => r.id.clone(),
    };
    found.sort_by(|a, b| compare(&key(a), &key(b)).then_with(|| a.id.cmp(&b.id)));
    if descending {
        found.reverse();
    }
    let total = found.len();
    let results: Vec<Json> = found
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|r| r.item.clone())
        .collect();
    Outcome::Applied(obj(vec![
        ("results", Json::Arr(results)),
        ("total", num(total as u64)),
        ("offset", num(offset as u64)),
        ("limit", num(limit as u64)),
    ]))
}

/// The mock's link shape: label, type and marking of each node a hop adds, in turn.
const SHAPE: [(&str, &str, &str); 4] = [
    ("operates → Route", "Route", "internal"),
    ("assigned → Crew", "Crew Member", "internal"),
    ("observes → Entity", "Sensor Feed", "confidential"),
    ("on → Event", "Maintenance Event", "internal"),
];

/// `searchAround(seedId, hops)`: the mock's graph (three objects at hop 1, two at each
/// hop after, hung off the first object of the hop before), with the nodes above the
/// persona's level withheld.
pub fn search_around(state: &mut DemoState, grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(api_name) = in_collection(&req.target, TYPES) else {
        return Outcome::Refused(
            "a search-around names its seed type as midgard.objectTypes/<apiName>".into(),
        );
    };
    let Some(type_row) = state.row(TYPES, api_name) else {
        return Outcome::Refused(format!("unknown object type: {api_name}"));
    };
    let seed_id = param(req, "seedId")
        .filter(|s| !s.is_empty())
        .unwrap_or("*");
    let seed = format!("{api_name}/{seed_id}");
    let hops = param(req, "hops")
        .and_then(|h| h.parse::<u64>().ok())
        .unwrap_or(1)
        .clamp(1, MAX_HOPS);
    let status = obj(vec![("status", Json::Str("active".into()))]);
    let node = |id: &str, label: &str, type_name: &str, marking: &str, hop: u64| {
        obj(vec![
            ("id", Json::Str(id.into())),
            ("label", Json::Str(label.into())),
            ("typeName", Json::Str(type_name.into())),
            ("marking", Json::Str(marking.into())),
            ("hop", num(hop)),
            ("propertyValues", status.clone()),
        ])
    };
    let seed_marking = text(&type_row.item, "marking").unwrap_or_else(|| "internal".into());
    let mut nodes = vec![node(
        &seed,
        &seed,
        &text(&type_row.item, "name").unwrap_or_else(|| api_name.into()),
        &seed_marking,
        0,
    )];
    let mut edges: Vec<(String, String, String)> = Vec::new();
    let mut idx = 0;
    for hop in 1..=hops {
        let count = if hop == 1 { 3 } else { 2 };
        for i in 0..count {
            let (label, type_name, marking) = SHAPE[idx % SHAPE.len()];
            idx += 1;
            let id = format!("n{hop}-{i}");
            nodes.push(node(
                &id,
                &format!("{type_name} {hop}.{}", i + 1),
                type_name,
                marking,
                hop,
            ));
            let parent = if hop == 1 {
                seed.clone()
            } else {
                format!("n{}-0", hop - 1)
            };
            edges.push((parent, id, label.to_string()));
        }
    }
    // Withhold what the persona may not read, and every link touching it.
    let readable = |n: &Json| {
        text(n, "marking")
            .and_then(|m| MarkingLevel::parse(&m))
            .is_some_and(|l| may_see(grant.clearance(), &RowMarking::new(l, Vec::<String>::new())))
    };
    let before = nodes.len();
    nodes.retain(readable);
    let kept: Vec<String> = nodes.iter().filter_map(|n| text(n, "id")).collect();
    edges.retain(|(f, t, _)| kept.contains(f) && kept.contains(t));
    let withheld = before - nodes.len();
    Outcome::Applied(obj(vec![
        ("seed", Json::Str(seed)),
        ("nodes", Json::Arr(nodes)),
        (
            "edges",
            Json::Arr(
                edges
                    .into_iter()
                    .map(|(f, t, l)| {
                        obj(vec![
                            ("from", Json::Str(f)),
                            ("to", Json::Str(t)),
                            ("label", Json::Str(l)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("truncated", Json::Bool(false)),
        ("nodeLimit", num(60)),
        ("seedObjects", num(1)),
        ("withheld", num(withheld as u64)),
    ]))
}

/// `simulatePolicy(ctx, marking)`: deny-by-default. An explicit deny wins, otherwise an
/// allow whose marking, purpose and a role match; else deny by default.
pub fn simulate_policy(state: &mut DemoState, grant: &Grant, req: &ActionRequest) -> Outcome {
    if req.target != format!("{POLICIES}/*") {
        return Outcome::Refused("a policy dry run is on midgard.policies/*".into());
    }
    let purpose = param(req, "purpose").unwrap_or("");
    let marking = param(req, "marking").unwrap_or("");
    let roles = &grant.persona().roles;
    let rules: Vec<&Json> = state
        .collections
        .get(POLICIES)
        .map(|c| {
            c.rows
                .iter()
                .filter(|r| may_see(grant.clearance(), &r.marking))
                .map(|r| &r.item)
                .collect()
        })
        .unwrap_or_default();
    let rule_roles = |p: &Json| -> Vec<String> {
        p.get("roles")
            .and_then(Json::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|r| r.as_str().map(str::to_string))
            .collect()
    };
    let applicable: Vec<&&Json> = rules
        .iter()
        .filter(|p| {
            text(p, "marking").as_deref() == Some(marking)
                && matches!(text(p, "purpose").as_deref(), Some(x) if x == purpose || x == "*")
        })
        .collect();
    let deny = applicable.iter().find(|p| {
        text(p, "effect").as_deref() == Some("deny")
            && rule_roles(p).iter().any(|r| r == "*" || roles.contains(r))
    });
    let allow = applicable.iter().find(|p| {
        text(p, "effect").as_deref() == Some("allow")
            && rule_roles(p).iter().any(|r| roles.contains(r))
    });
    let decision = |d: &str, rule: Option<&Json>, reason: String| {
        Outcome::Applied(obj(vec![
            ("decision", Json::Str(d.into())),
            (
                "matchedRule",
                rule.and_then(|r| text(r, "id"))
                    .map_or(Json::Null, Json::Str),
            ),
            ("reason", Json::Str(reason)),
        ]))
    };
    let name = |p: &Json| text(p, "name").unwrap_or_default();
    if let Some(p) = deny {
        return decision("deny", Some(p), format!("Explicit deny by {}.", name(p)));
    }
    if let Some(p) = allow {
        return decision("grant", Some(p), format!("Granted by {}.", name(p)));
    }
    decision(
        "deny",
        None,
        "Deny by default — no matching allow rule.".into(),
    )
}
