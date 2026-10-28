//! The enclosure the HTTP edge serves: the **real** object fabric and governance
//! spine, projected into the `@midgard/sdk` view models the consoles render.
//!
//! Everything a console reads or changes here goes through the same crates the
//! acceptance suite verifies:
//!
//! - **Object types, queries and search-around** come from `fabric`'s type registry
//!   and indexed store, authorised per object at read time — marking dominance,
//!   compartments and purpose — so a caller only ever sees what they are cleared
//!   for. [DD-1] [NFR-SEC-2] [AC-M-4]
//! - **Actions** run through `Fabric::apply_action`: typed parameter validation, an
//!   audited authorisation on the action's own api name, then the bound function's
//!   effects applied through the store. [NFR-AI-1]
//! - **Audit** is the governance ledger itself — the PQ-signed chain every edge
//!   admission and every fabric decision is written to — with each row's hash,
//!   chain link and ML-DSA signature re-verified as it is served. [NFR-PQ-3]
//! - **Policies and the policy dry-run** are the rule set the PDP actually enforces,
//!   and a real, non-auditing PDP decision.
//!
//! Three concerns the fabric domain deliberately does not carry live here, at the
//! edge: an action's display name and the object type it acts on, its human gate (a
//! workflow control), and when each object type last changed.

use std::collections::{BTreeMap, BTreeSet};

use audit_writer::{entry_hash, genesis_hash, signing_preimage, SealedEntry};
use fabric::{
    ActionFunction, ActionType, Cardinality, Effect, Fabric, FabricError, FunctionRuntime,
    Governance, InMemoryLedger, InMemoryStore, LedgerStore, LinkInstance, LinkType, ObjectInstance,
    ObjectStore, ObjectType, PolicyContext, PropertyType, Query, TypeRegistry, Value, ValueType,
};
use policy::{
    ClassificationLevel, Compartment, DecisionReason, Effect as RuleEffect, Marking, Purpose,
    Resource, ResourceId, Selector,
};
use pqc_agility::{CryptoProvider, VerifyingKey};

use crate::json::Json;
use crate::ontology_file::{ActionEffect, Ontology};

use service_runtime::marking::BANDS;
/// The console's four marking bands, lowest to highest, mapped to policy levels by
/// ordinal (see `service_runtime::marking`), shared with the mesh services.
pub use service_runtime::marking::{band, level_of};

/// Most root objects a `<type>/*` search-around starts from.
const MAX_ROOTS: usize = 25;
/// Most nodes a search-around returns.
const MAX_NODES: usize = 60;
/// Deepest search-around.
const MAX_HOPS: i64 = 4;
/// How long an approved human gate can be redeemed (seconds).
const APPROVAL_TTL: u64 = 900;
/// Most approvals the enclosure holds; the oldest are dropped first.
const MAX_APPROVALS: usize = 256;

/// A request the enclosure refused, already mapped to its HTTP status.
pub struct EdgeError {
    /// HTTP status code.
    pub status: u16,
    /// JSON error body.
    pub body: Json,
}

impl EdgeError {
    fn new(status: u16, message: &str) -> Self {
        Self {
            status,
            body: Json::obj(vec![("error", Json::s(message))]),
        }
    }

    /// A malformed request body.
    pub fn bad_request(message: &str) -> Self {
        Self::new(400, message)
    }

    fn denied(audit_seq: u64) -> Self {
        Self {
            status: 403,
            body: Json::obj(vec![
                ("error", Json::s("denied by policy")),
                ("auditSeq", Json::s(&audit_seq.to_string())),
            ]),
        }
    }
}

/// An action as the console presents it. The fabric's [`ActionType`] carries
/// neither a display name, the object type it acts on, nor a human gate — those are
/// presentation and workflow, and they live here.
#[derive(Debug, Clone)]
pub struct CatalogAction {
    /// The fabric action type id.
    pub id: String,
    /// The display name the console lists, and sends back as `actionType`.
    pub title: String,
    /// The object type the action acts on.
    pub object_type: String,
    /// Whether a named human must authorise it first. [NFR-AI-1]
    pub human_gated: bool,
}

/// Where an approval stands.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ApprovalState {
    /// Waiting for a second person.
    Pending,
    /// Approved by `approver`; redeemable by the requester until `expires`.
    Approved { approver: String, expires: u64 },
    /// Refused by `approver`.
    Rejected { approver: String },
    /// Redeemed: the action ran.
    Used { approver: String },
}

/// A request for a human gate on one action against one object. [NFR-AI-1]
///
/// The gate is a two-person rule: the requester asks, someone else approves, and only
/// the requester can then redeem it — once, for that action on that object, before it
/// expires. Approvals live in memory: a restart drops them, and the requester asks
/// again.
#[derive(Debug, Clone)]
struct Approval {
    id: String,
    /// The catalog action id.
    action_id: String,
    /// The action's display name.
    action_title: String,
    /// The one object it acts on.
    target: String,
    requester: String,
    requested: u64,
    state: ApprovalState,
}

/// One action the development enclosure provisions.
pub struct DevAction {
    /// Fabric action type id.
    pub id: &'static str,
    /// The api name the fabric authorises the action on.
    pub api_name: &'static str,
    /// Display name.
    pub title: &'static str,
    /// The object type it acts on.
    pub object_type: &'static str,
    /// Whether a named human must authorise it first.
    pub human_gated: bool,
    /// The marking band the action itself carries.
    pub band: &'static str,
}

/// The development enclosure's actions. The dev policy grants the operator one rule
/// per entry, because the fabric authorises an action on its own api name.
pub const DEV_ACTIONS: &[DevAction] = &[
    DevAction {
        id: "ground_aircraft",
        api_name: "groundAircraft",
        title: "Ground aircraft",
        object_type: "aircraft",
        human_gated: true,
        band: "confidential",
    },
    DevAction {
        id: "schedule_maintenance",
        api_name: "scheduleMaintenance",
        title: "Schedule maintenance",
        object_type: "aircraft",
        human_gated: true,
        band: "confidential",
    },
    DevAction {
        id: "change_facility_status",
        api_name: "changeFacilityStatus",
        title: "Change status",
        object_type: "facility",
        human_gated: true,
        band: "restricted",
    },
    DevAction {
        id: "request_clearance_review",
        api_name: "requestClearanceReview",
        title: "Update clearance",
        object_type: "crew_member",
        human_gated: true,
        band: "internal",
    },
    DevAction {
        id: "recalibrate_sensor",
        api_name: "recalibrateSensor",
        title: "Recalibrate",
        object_type: "sensor_feed",
        human_gated: false,
        band: "internal",
    },
    DevAction {
        id: "close_maintenance_event",
        api_name: "closeMaintenanceEvent",
        title: "Close event",
        object_type: "maintenance_event",
        human_gated: false,
        band: "internal",
    },
];

/// The real object fabric (which owns the single governance spine) plus the edge's
/// own presentation state.
///
/// Generic over the fabric's object store and audit ledger. Development runs both in
/// memory; a deployment opens durable, KMS-sealed ones so objects and lineage
/// survive a restart. [AC-M-4] [RC-09]
pub struct Enclosure<S: ObjectStore = InMemoryStore, L: LedgerStore = InMemoryLedger> {
    fabric: Fabric<S, L>,
    actions: Vec<CatalogAction>,
    /// Last write per object type (Unix seconds), for the `updated` column.
    updated: BTreeMap<String, u64>,
    /// Human-gate approvals, oldest first.
    approvals: Vec<Approval>,
    /// The number the next approval id takes.
    next_approval: u64,
}

impl<L: LedgerStore> Enclosure<InMemoryStore, L> {
    /// An empty in-memory enclosure over `governance`: no schema, no objects, no
    /// actions.
    pub fn new(governance: Governance<L>) -> Self {
        Self::with_store(InMemoryStore::new(), governance)
    }
}

impl<S: ObjectStore, L: LedgerStore> Enclosure<S, L> {
    /// An empty enclosure over an explicit object store and governance spine.
    pub fn with_store(store: S, governance: Governance<L>) -> Self {
        Self {
            fabric: Fabric::new(
                TypeRegistry::new(),
                store,
                FunctionRuntime::new(),
                governance,
            ),
            actions: Vec::new(),
            updated: BTreeMap::new(),
            approvals: Vec::new(),
            next_approval: 1,
        }
    }

    /// The fabric.
    pub fn fabric(&self) -> &Fabric<S, L> {
        &self.fabric
    }

    /// The fabric, mutably (schema and data provisioning).
    pub fn fabric_mut(&mut self) -> &mut Fabric<S, L> {
        &mut self.fabric
    }

    /// The governance spine, for edge admission decisions into the same ledger.
    pub fn governance_mut(&mut self) -> &mut Governance<L> {
        self.fabric.governance_mut()
    }

    /// Entries in the audit ledger.
    pub fn audit_len(&self) -> usize {
        self.fabric.audit_entries().len()
    }

    /// Register an object type.
    pub fn register_object_type(&mut self, ot: ObjectType, now: u64) -> Result<(), FabricError> {
        let id = ot.id.clone();
        self.fabric.registry_mut().register_object_type(ot)?;
        self.updated.insert(id, now);
        Ok(())
    }

    /// Register a minimal object type — an id, a title and an optional `name` — for
    /// callers that only need the type to exist.
    pub fn register_named_type(
        &mut self,
        id: &str,
        title: &str,
        now: u64,
    ) -> Result<(), FabricError> {
        self.register_object_type(
            ObjectType {
                id: id.into(),
                api_name: id.into(),
                title: title.into(),
                properties: vec![PropertyType::new("name", ValueType::Text, false)],
                interfaces: Vec::new(),
            },
            now,
        )
    }

    // ---- object fabric ----------------------------------------------------------

    /// `GET /object-types`: every registered type, **scoped to the caller** — the
    /// instance count, marking and compartments reflect only objects they may read,
    /// so the listing is not an oracle for data above their clearance.
    pub fn object_types(&self, ctx: &PolicyContext, now: u64) -> Json {
        let reg = self.fabric.registry();
        let store = self.fabric.store();
        let gov = self.fabric.governance();
        Json::Arr(
            reg.object_types()
                .map(|ot| {
                    let readable: Vec<&ObjectInstance> = store
                        .objects_of_type(&ot.id)
                        .into_iter()
                        .filter(|o| can_read(gov, ctx, o))
                        .collect();
                    let top = readable
                        .iter()
                        .map(|o| o.marking.level)
                        .max()
                        .unwrap_or(ClassificationLevel::UNCLASSIFIED);
                    let compartments: BTreeSet<&str> = readable
                        .iter()
                        .flat_map(|o| o.marking.compartments.iter())
                        .map(|c| c.0.as_str())
                        .collect();
                    let compartment = if compartments.is_empty() {
                        Json::Null
                    } else {
                        Json::s(&compartments.into_iter().collect::<Vec<_>>().join("/"))
                    };
                    let links = reg
                        .link_types()
                        .filter(|lt| lt.source_type == ot.id)
                        .map(|lt| Json::obj(vec![("label", Json::s(&link_label(reg, lt)))]))
                        .collect();
                    let actions = self
                        .actions
                        .iter()
                        .filter(|a| a.object_type == ot.id)
                        .map(|a| {
                            Json::obj(vec![
                                ("name", Json::s(&a.title)),
                                ("humanGated", Json::Bool(a.human_gated)),
                            ])
                        })
                        .collect();
                    Json::obj(vec![
                        ("name", Json::s(&ot.title)),
                        ("apiName", Json::s(&ot.id)),
                        (
                            "propertyCount",
                            Json::Int(i64::try_from(ot.properties.len()).unwrap_or(i64::MAX)),
                        ),
                        ("instanceCount", Json::s(&thousands(readable.len()))),
                        ("marking", Json::s(band(top))),
                        ("compartment", compartment),
                        ("links", Json::Arr(links)),
                        ("actions", Json::Arr(actions)),
                        (
                            "updated",
                            Json::s(&ago(now, self.updated.get(&ot.id).copied())),
                        ),
                    ])
                })
                .collect(),
        )
    }

    /// `POST /query`: a typed fabric query, authorised and audited once, with every
    /// result marking-filtered against the caller.
    pub fn query(
        &mut self,
        ctx: &PolicyContext,
        object_type: &str,
        max_results: i64,
        filter: BTreeMap<String, String>,
        now: u64,
    ) -> Result<Json, EdgeError> {
        let q = Query {
            object_type: object_type.to_string(),
            filter,
            link_traversals: Vec::new(),
            max_results: usize::try_from(max_results.clamp(1, 500)).unwrap_or(500),
        };
        let resp = match self.fabric.query(ctx, &q, now) {
            Ok(resp) => resp,
            Err(e) => return Err(self.refusal(e)),
        };
        Ok(Json::obj(vec![
            (
                "results",
                Json::Arr(resp.results.iter().map(object_json).collect()),
            ),
            ("auditId", Json::s(&audit_id(resp.audit_seq))),
        ]))
    }

    /// `POST /actions`: a governed fabric action on one object.
    ///
    /// The human gate is checked first, at the edge, because it is a workflow control
    /// the fabric does not model: a gated action needs the id of an approval a second
    /// person granted for this caller, this action and this object (see
    /// [`Self::request_approval`]). The console's `target` must name one object **the
    /// caller may read**; it is handed to the fabric as the `targets` parameter, so
    /// the action is validated, authorised and audited once. The fabric authorises an
    /// action type rather than a target instance; resolving the target here is what
    /// keeps an action from reaching an object the caller cannot see.
    pub fn apply_action(
        &mut self,
        ctx: &PolicyContext,
        action_type: &str,
        mut params: BTreeMap<String, String>,
        human_gate_token: Option<&str>,
        now: u64,
    ) -> Result<Json, EdgeError> {
        let entry = self.catalog_action(action_type)?;
        let gate = human_gate_token.map(str::trim).filter(|t| !t.is_empty());
        if entry.human_gated && gate.is_none() {
            return Err(EdgeError::new(
                403,
                &format!("Human gate required for action \"{}\"", entry.title),
            ));
        }
        let target = params.remove("target").unwrap_or_default();
        let target = self.readable_target(ctx, &entry.object_type, &target)?;
        let approval = match gate {
            Some(id) if entry.human_gated => Some(self.redeemable(ctx, &entry, &target, id, now)?),
            _ => None,
        };
        params.insert("targets".into(), target);

        let resp = match self.fabric.apply_action(ctx, &entry.id, &params, now) {
            Ok(resp) => resp,
            Err(e) => return Err(self.refusal(e)),
        };
        if resp.applied {
            self.updated.insert(entry.object_type, now);
            if let Some(i) = approval {
                let a = &mut self.approvals[i];
                if let ApprovalState::Approved { approver, .. } = &a.state {
                    let approver = approver.clone();
                    a.state = ApprovalState::Used { approver };
                }
            }
        }
        Ok(Json::obj(vec![
            ("auditId", Json::s(&audit_id(resp.audit_seq))),
            ("applied", Json::Bool(resp.applied)),
        ]))
    }

    /// The catalog entry for an action, by display name or id.
    fn catalog_action(&self, action_type: &str) -> Result<CatalogAction, EdgeError> {
        self.actions
            .iter()
            .find(|a| a.title == action_type || a.id == action_type)
            .cloned()
            .ok_or_else(|| EdgeError::new(404, &format!("unknown action type: {action_type}")))
    }

    /// Resolve a console `target` to the one object id it names, which must exist, be
    /// of `object_type`, and be readable by the caller. A wildcard is refused: an
    /// action changes one object at a time. An unreadable id is reported exactly like
    /// a missing one, so the edge is not an oracle for objects above the caller's
    /// clearance.
    fn readable_target(
        &self,
        ctx: &PolicyContext,
        object_type: &str,
        target: &str,
    ) -> Result<String, EdgeError> {
        if target.is_empty() || target.ends_with("/*") {
            return Err(EdgeError::bad_request(&format!(
                "an action applies to one object: name it as \"target\":\"{object_type}/<id>\""
            )));
        }
        let gov = self.fabric.governance();
        match self.fabric.store().get_object(target) {
            Some(o) if o.type_id == object_type && can_read(gov, ctx, o) => Ok(o.id.clone()),
            _ => Err(EdgeError::new(404, &format!("unknown target: {target}"))),
        }
    }

    // ---- human-gate approvals [NFR-AI-1] -----------------------------------------

    /// `POST /approvals`: ask for a human gate on one gated action against one object
    /// the caller may read. Returns the pending approval; its `id` is the
    /// `humanGateToken` the caller sends once someone else has approved it.
    pub fn request_approval(
        &mut self,
        ctx: &PolicyContext,
        action_type: &str,
        target: &str,
        now: u64,
    ) -> Result<Json, EdgeError> {
        let entry = self.catalog_action(action_type)?;
        if !entry.human_gated {
            return Err(EdgeError::bad_request(&format!(
                "action \"{}\" needs no approval",
                entry.title
            )));
        }
        let target = self.readable_target(ctx, &entry.object_type, target)?;
        if self.approvals.len() >= MAX_APPROVALS {
            self.approvals.remove(0);
        }
        let approval = Approval {
            id: format!("APR-{}", self.next_approval),
            action_id: entry.id,
            action_title: entry.title,
            target,
            requester: ctx.subject.clone(),
            requested: now,
            state: ApprovalState::Pending,
        };
        self.next_approval += 1;
        let json = approval_json(&approval, now, &ctx.subject);
        self.approvals.push(approval);
        Ok(json)
    }

    /// `POST /approvals/{id}/approve` or `/reject`: a second person decides a pending
    /// approval. The requester cannot decide their own, and an approver who cannot
    /// read the target sees the approval as unknown. The edge has already authorised
    /// and audited the decision (`Approve`), so the ledger names who decided.
    pub fn decide_approval(
        &mut self,
        ctx: &PolicyContext,
        id: &str,
        approve: bool,
        now: u64,
    ) -> Result<Json, EdgeError> {
        let unknown = || EdgeError::new(404, &format!("unknown approval: {id}"));
        let i = self
            .approvals
            .iter()
            .position(|a| a.id == id)
            .ok_or_else(unknown)?;
        if !self.can_read_id(ctx, &self.approvals[i].target) {
            return Err(unknown());
        }
        let a = &mut self.approvals[i];
        if a.state != ApprovalState::Pending {
            return Err(EdgeError::new(
                409,
                &format!("approval {id} is already {}", approval_status(a, now)),
            ));
        }
        if a.requester == ctx.subject {
            return Err(EdgeError::new(
                403,
                "an approval must be decided by someone other than its requester",
            ));
        }
        let approver = ctx.subject.clone();
        a.state = if approve {
            ApprovalState::Approved {
                approver,
                expires: now.saturating_add(APPROVAL_TTL),
            }
        } else {
            ApprovalState::Rejected { approver }
        };
        Ok(approval_json(a, now, &ctx.subject))
    }

    /// `GET /approvals`: every approval whose target the caller may read, newest first;
    /// `mine` marks the ones the caller requested.
    pub fn approvals(&self, ctx: &PolicyContext, now: u64) -> Json {
        Json::Arr(
            self.approvals
                .iter()
                .rev()
                .filter(|a| self.can_read_id(ctx, &a.target))
                .map(|a| approval_json(a, now, &ctx.subject))
                .collect(),
        )
    }

    /// The index of approval `id` if the caller can redeem it now for `entry` on
    /// `target`: approved by someone else, requested by this caller, for this action
    /// and object, unexpired and unused. Every failure reads the same, so a token
    /// cannot be probed.
    fn redeemable(
        &self,
        ctx: &PolicyContext,
        entry: &CatalogAction,
        target: &str,
        id: &str,
        now: u64,
    ) -> Result<usize, EdgeError> {
        self.approvals
            .iter()
            .position(|a| {
                a.id == id
                    && a.requester == ctx.subject
                    && a.action_id == entry.id
                    && a.target == target
                    && matches!(a.state, ApprovalState::Approved { expires, .. } if now < expires)
            })
            .ok_or_else(|| {
                EdgeError::new(
                    403,
                    &format!(
                        "Human gate token is not a current approval of \"{}\" on {target} for this user",
                        entry.title
                    ),
                )
            })
    }

    /// Whether the caller may read object `id`.
    fn can_read_id(&self, ctx: &PolicyContext, id: &str) -> bool {
        self.fabric
            .store()
            .get_object(id)
            .is_some_and(|o| can_read(self.fabric.governance(), ctx, o))
    }

    /// `POST /search-around`: walk real links, in both directions, out to `hops`.
    ///
    /// The traversal is authorised and audited once, on the seed; every node reached
    /// is then authorised individually and dropped if the caller may not read it — the
    /// same model as a fabric query. A seed the caller cannot read is reported as
    /// unknown, and writes no object-specific audit row that would reveal it exists.
    pub fn search_around(
        &mut self,
        ctx: &PolicyContext,
        seed: &str,
        hops: i64,
        now: u64,
    ) -> Result<Json, EdgeError> {
        let hops = u64::try_from(hops.clamp(1, MAX_HOPS)).unwrap_or(1);

        let (roots, resource) = {
            let gov = self.fabric.governance();
            let store = self.fabric.store();
            match seed.split_once('/') {
                Some((ty, "*")) => {
                    if self.fabric.registry().object_type(ty).is_none() {
                        return Err(EdgeError::new(404, &format!("unknown object type: {ty}")));
                    }
                    let ids: Vec<String> = store
                        .objects_of_type(ty)
                        .into_iter()
                        .filter(|o| can_read(gov, ctx, o))
                        .take(MAX_ROOTS)
                        .map(|o| o.id.clone())
                        .collect();
                    let resource = Resource {
                        id: ResourceId(ty.to_string()),
                        marking: Marking {
                            level: ClassificationLevel::UNCLASSIFIED,
                            compartments: BTreeSet::new(),
                        },
                        allowed_purposes: [ctx.purpose.clone()].into_iter().collect(),
                    };
                    (ids, resource)
                }
                _ => match store.get_object(seed) {
                    Some(o) if can_read(gov, ctx, o) => (vec![o.id.clone()], resource_of(o)),
                    _ => return Err(EdgeError::new(404, &format!("unknown seed object: {seed}"))),
                },
            }
        };

        let decision = self
            .fabric
            .governance_mut()
            .authorize(ctx, &resource, "Read", now)
            .map_err(|e| EdgeError::new(500, &format!("audit failure: {e}")))?;
        if !decision.permitted {
            return Err(EdgeError::denied(decision.audit_seq));
        }

        let reg = self.fabric.registry();
        let store = self.fabric.store();
        let gov = self.fabric.governance();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut edge_keys: BTreeSet<(String, String, String)> = BTreeSet::new();
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut frontier: Vec<String> = Vec::new();

        for id in &roots {
            if let Some(o) = store.get_object(id) {
                if seen.insert(o.id.clone()) {
                    nodes.push(node_json(reg, o, 0));
                    frontier.push(o.id.clone());
                }
            }
        }

        for hop in 1..=hops {
            let mut next = Vec::new();
            for id in &frontier {
                let Some(node) = store.get_object(id) else {
                    continue;
                };
                // (link type, source, target) for every link touching this node.
                let mut adjacent: Vec<(&LinkType, &ObjectInstance, &ObjectInstance)> = Vec::new();
                for lt in reg.link_types() {
                    if lt.source_type == node.type_id {
                        for target in store.neighbors(&node.id, &lt.id) {
                            adjacent.push((lt, node, target));
                        }
                    }
                    if lt.target_type == node.type_id {
                        for source in store.objects_of_type(&lt.source_type) {
                            if store
                                .neighbors(&source.id, &lt.id)
                                .iter()
                                .any(|t| t.id == node.id)
                            {
                                adjacent.push((lt, source, node));
                            }
                        }
                    }
                }
                for (lt, source, target) in adjacent {
                    let other = if source.id == node.id { target } else { source };
                    if !can_read(gov, ctx, other) {
                        continue;
                    }
                    let is_new = !seen.contains(&other.id);
                    if is_new && nodes.len() >= MAX_NODES {
                        continue;
                    }
                    if edge_keys.insert((lt.id.clone(), source.id.clone(), target.id.clone())) {
                        edges.push(Json::obj(vec![
                            ("from", Json::s(&source.id)),
                            ("to", Json::s(&target.id)),
                            ("label", Json::s(&link_label(reg, lt))),
                        ]));
                    }
                    if is_new {
                        seen.insert(other.id.clone());
                        nodes.push(node_json(reg, other, hop));
                        next.push(other.id.clone());
                    }
                }
            }
            frontier = next;
        }

        Ok(Json::obj(vec![
            ("seed", Json::s(seed)),
            ("nodes", Json::Arr(nodes)),
            ("edges", Json::Arr(edges)),
            ("auditId", Json::s(&audit_id(decision.audit_seq))),
        ]))
    }

    // ---- governance -------------------------------------------------------------

    /// `GET /audit`: the governance ledger, newest first.
    ///
    /// Each served row is re-verified — its entry hash recomputed, its link to the
    /// previous entry checked, and its ML-DSA signature verified — and marked `✓` only
    /// if all three hold. The cost is per row served, not per ledger entry, so a
    /// console polling this view does not pay for the whole chain on every poll.
    pub fn audit(&self, provider: &CryptoProvider, limit: usize) -> Json {
        let entries = self.fabric.audit_entries();
        let key = self.fabric.audit_verifying_key();
        let genesis = genesis_hash(self.fabric.audit_ledger_id());
        Json::Arr(
            entries
                .iter()
                .rev()
                .take(limit.clamp(1, 500))
                .map(|e| {
                    let linked = match e.seq.checked_sub(1) {
                        None => e.prev_hash == genesis,
                        Some(prev) => usize::try_from(prev)
                            .ok()
                            .and_then(|i| entries.get(i))
                            .is_some_and(|p| p.seq == prev && p.entry_hash == e.prev_hash),
                    };
                    let alg = e.sig_alg.as_str();
                    let marker = if linked && signature_verifies(provider, key, e) {
                        format!("{alg} ✓")
                    } else {
                        format!("{alg} ✗ unverified")
                    };
                    Json::obj(vec![
                        ("id", Json::s(&audit_id(e.seq))),
                        ("actor", Json::s(&e.record.actor.0)),
                        ("action", Json::s(&e.record.action.0)),
                        ("resource", Json::s(&e.record.resource.0)),
                        ("ts", Json::s(&time_of_day(&e.record.ts.0))),
                        ("pqSignature", Json::s(&marker)),
                    ])
                })
                .collect(),
        )
    }

    /// `GET /policies`: the rule set the PDP enforces.
    ///
    /// A role rule places no marking ceiling of its own — mandatory access control is
    /// enforced before any rule is consulted — so each rule is reported at the highest
    /// band, which under the console's `marking <= X` reading means "at every level".
    pub fn policies(&self) -> Json {
        Json::Arr(
            self.fabric
                .governance()
                .policies()
                .rules()
                .iter()
                .map(|r| {
                    let (effect, verb) = match r.effect {
                        RuleEffect::Permit => ("allow", "may"),
                        RuleEffect::Deny => ("deny", "may not"),
                    };
                    let scope = match &r.selector {
                        Selector::Any => "any resource".to_string(),
                        Selector::Resource(id) => id.0.clone(),
                    };
                    Json::obj(vec![
                        ("id", Json::s(&r.id.0)),
                        (
                            "name",
                            Json::s(&format!("{} {verb} {} · {scope}", r.role.0, r.action.0)),
                        ),
                        ("effect", Json::s(effect)),
                        ("marking", Json::s(BANDS[BANDS.len() - 1])),
                        (
                            "purpose",
                            Json::s(r.purpose.as_ref().map_or("*", |p| p.0.as_str())),
                        ),
                        ("roles", Json::Arr(vec![Json::s(&r.role.0)])),
                    ])
                })
                .collect(),
        )
    }

    /// `POST /policy/simulate`: a real, non-auditing PDP decision for the **caller's
    /// own** roles and clearance, at `marking`, under `purpose`.
    ///
    /// Roles and clearance come from the verified token, never the request body, so a
    /// caller cannot dry-run privileges they do not hold. The purpose is the
    /// hypothetical: the token authorises one purpose, and asking about another meets
    /// the PDP's purpose gate exactly as a real request would. `audit_seq` is the
    /// edge's own admission row for this call, so the dry-run itself is on the record.
    pub fn simulate_policy(
        &self,
        ctx: &PolicyContext,
        purpose: &str,
        marking: &str,
        audit_seq: u64,
    ) -> Result<Json, EdgeError> {
        let level = level_of(marking)
            .ok_or_else(|| EdgeError::bad_request(&format!("unknown marking: {marking}")))?;
        let purpose = Purpose(purpose.to_string());
        let resource = Resource {
            id: ResourceId("policy/dry-run".into()),
            marking: Marking {
                level,
                compartments: BTreeSet::new(),
            },
            allowed_purposes: [purpose.clone()].into_iter().collect(),
        };
        let d = self
            .fabric
            .governance()
            .decide(ctx, &resource, "Read", &purpose);
        let decision = if d.is_permit() { "grant" } else { "deny" };
        let reason = d.reasons.first().map_or_else(
            || "Deny by default — no matching allow rule.".to_string(),
            describe,
        );
        Ok(Json::obj(vec![
            ("decision", Json::s(decision)),
            (
                "matchedRule",
                d.matched_rules
                    .first()
                    .map_or(Json::Null, |r| Json::s(&r.0)),
            ),
            ("reason", Json::s(&reason)),
            ("auditId", Json::s(&audit_id(audit_seq))),
        ]))
    }

    /// Map a fabric refusal to its HTTP form. A denial carries the sequence number of
    /// the ledger row that recorded it, which is the row the fabric just wrote.
    fn refusal(&self, e: FabricError) -> EdgeError {
        match &e {
            FabricError::Denied => EdgeError::denied(
                self.fabric
                    .audit_entries()
                    .last()
                    .map_or(0, |entry| entry.seq),
            ),
            FabricError::UnknownObjectType(_) | FabricError::UnknownActionType(_) => {
                EdgeError::new(404, &e.to_string())
            }
            FabricError::MissingRequiredParam(_) | FabricError::BadParamType { .. } => {
                EdgeError::new(400, &e.to_string())
            }
            _ => EdgeError::new(500, &e.to_string()),
        }
    }

    // ---- ontology -----------------------------------------------------------------

    /// Register a deployment's ontology: its object types, link types and actions, each
    /// action bound to the function its declared effect describes and listed in the
    /// console catalogue. The registry is not journalled, so this runs on every boot,
    /// before anything is served. [DD-1] [NFR-AI-1]
    pub fn load_ontology(&mut self, ontology: &Ontology, now: u64) -> Result<(), FabricError> {
        for object_type in &ontology.object_types {
            self.register_object_type(object_type.clone(), now)?;
        }
        for link_type in &ontology.link_types {
            self.fabric
                .registry_mut()
                .register_link_type(link_type.clone())?;
        }
        for a in &ontology.actions {
            let function_id = format!("fn_{}", a.id);
            self.fabric
                .registry_mut()
                .register_action_type(ActionType {
                    id: a.id.clone(),
                    api_name: a.api_name.clone(),
                    parameters: vec![PropertyType::new("targets", ValueType::Text, true)],
                    function_id: function_id.clone(),
                    marking: a.marking.clone(),
                    allowed_purposes: a.purposes.clone(),
                })?;
            let function: Box<dyn ActionFunction> = match &a.effect {
                ActionEffect::SetProperty { property, value } => Box::new(SetProperty {
                    property: property.clone(),
                    value: value.clone(),
                }),
                ActionEffect::CycleProperty { property, values } => Box::new(CycleProperty {
                    property: property.clone(),
                    values: values.clone(),
                }),
            };
            self.fabric.runtime_mut().register(&function_id, function);
            self.actions.push(CatalogAction {
                id: a.id.clone(),
                title: a.title.clone(),
                object_type: a.object_type.clone(),
                human_gated: a.human_gated,
            });
        }
        Ok(())
    }

    // ---- development seed -------------------------------------------------------

    /// Register the development schema: a small aviation-operations ontology — object
    /// types, link types — and its actions, each bound to its function and presented
    /// in the console catalogue. [`Enclosure::seed_dev_objects`] adds the data.
    ///
    /// The data is illustrative; nothing about how it is served is. It lives in the
    /// real fabric, every read is authorised per object, and every action is
    /// validated, authorised and audited. One sensor feed carries the SIG compartment,
    /// which the development operator does not hold, so mandatory access control is
    /// visible in every console view that touches it.
    pub fn register_dev_schema(&mut self, purpose: &str, now: u64) -> Result<(), FabricError> {
        let text = |api: &str, required: bool| PropertyType::new(api, ValueType::Text, required);
        let object_type = |id: &str, title: &str, properties: Vec<PropertyType>| ObjectType {
            id: id.into(),
            api_name: id.into(),
            title: title.into(),
            properties,
            interfaces: Vec::new(),
        };

        for ot in [
            object_type(
                "aircraft",
                "Aircraft",
                vec![
                    text("name", true),
                    text("tail", false),
                    text("model", false),
                    text("status", false),
                    text("base", false),
                    PropertyType::new("flight_hours", ValueType::Int, false),
                ],
            ),
            object_type(
                "facility",
                "Facility",
                vec![
                    text("name", true),
                    text("kind", false),
                    text("status", false),
                    text("region", false),
                ],
            ),
            object_type(
                "route",
                "Route",
                vec![
                    text("name", true),
                    text("origin", false),
                    text("destination", false),
                ],
            ),
            object_type(
                "crew_member",
                "Crew Member",
                vec![
                    text("name", true),
                    text("rank", false),
                    text("qualification", false),
                    text("clearance_review", false),
                ],
            ),
            object_type(
                "sensor_feed",
                "Sensor Feed",
                vec![
                    text("name", true),
                    text("modality", false),
                    PropertyType::new("rate_hz", ValueType::Float, false),
                    text("status", false),
                ],
            ),
            object_type(
                "maintenance_event",
                "Maintenance Event",
                vec![
                    text("name", true),
                    text("severity", false),
                    text("status", false),
                ],
            ),
        ] {
            self.register_object_type(ot, now)?;
        }

        let link_type = |id: &str, verb: &str, source: &str, target: &str, cardinality| LinkType {
            id: id.into(),
            api_name: verb.into(),
            source_type: source.into(),
            target_type: target.into(),
            cardinality,
        };
        for lt in [
            link_type(
                "aircraft_operates",
                "operates",
                "aircraft",
                "route",
                Cardinality::OneToMany,
            ),
            link_type(
                "aircraft_crew",
                "assigned",
                "aircraft",
                "crew_member",
                Cardinality::OneToMany,
            ),
            link_type(
                "aircraft_base",
                "based at",
                "aircraft",
                "facility",
                Cardinality::OneToOne,
            ),
            link_type(
                "maintenance_on",
                "on",
                "maintenance_event",
                "aircraft",
                Cardinality::OneToOne,
            ),
            link_type(
                "sensor_observes",
                "observes",
                "sensor_feed",
                "aircraft",
                Cardinality::OneToMany,
            ),
        ] {
            self.fabric.registry_mut().register_link_type(lt)?;
        }

        let purposes: BTreeSet<Purpose> = [Purpose(purpose.to_string())].into_iter().collect();
        for a in DEV_ACTIONS {
            let function_id = format!("fn_{}", a.id);
            self.fabric
                .registry_mut()
                .register_action_type(ActionType {
                    id: a.id.into(),
                    api_name: a.api_name.into(),
                    parameters: vec![text("targets", true)],
                    function_id: function_id.clone(),
                    marking: marking(a.band, &[]),
                    allowed_purposes: purposes.clone(),
                })?;
            self.fabric
                .runtime_mut()
                .register(&function_id, dev_function(a.id));
            self.actions.push(CatalogAction {
                id: a.id.into(),
                title: a.title.into(),
                object_type: a.object_type.into(),
                human_gated: a.human_gated,
            });
        }
        Ok(())
    }

    /// Seed the development data set — objects and links — into the fabric's store.
    ///
    /// Run once per store. The type registry and action catalogue live in memory, so a
    /// durable enclosure calls [`Enclosure::register_dev_schema`] on every boot, but its
    /// objects and links are already journalled and must not be written twice. [DD-1]
    pub fn seed_dev_objects(&mut self, purpose: &str) {
        let purposes: BTreeSet<Purpose> = [Purpose(purpose.to_string())].into_iter().collect();
        let t = |s: &str| Value::Text(s.into());
        let object = |ty: &str, key: &str, m: Marking, props: Vec<(&str, Value)>| ObjectInstance {
            id: format!("{ty}/{key}"),
            type_id: ty.into(),
            properties: props.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            marking: m,
            allowed_purposes: purposes.clone(),
        };
        let ops_n = || marking("confidential", &["OPS-N"]);
        let objects = vec![
            object(
                "aircraft",
                "AC-2214",
                ops_n(),
                vec![
                    ("name", t("AC-2214")),
                    ("tail", t("MG-404")),
                    ("model", t("Heavy transport")),
                    ("status", t("active")),
                    ("base", t("FAC-NORTH")),
                    ("flight_hours", Value::Int(6_412)),
                ],
            ),
            object(
                "aircraft",
                "AC-2261",
                ops_n(),
                vec![
                    ("name", t("AC-2261")),
                    ("tail", t("MG-336")),
                    ("model", t("Tanker / transport")),
                    ("status", t("active")),
                    ("base", t("FAC-NORTH")),
                    ("flight_hours", Value::Int(9_120)),
                ],
            ),
            object(
                "aircraft",
                "AC-2198",
                ops_n(),
                vec![
                    ("name", t("AC-2198")),
                    ("tail", t("MG-417")),
                    ("model", t("Heavy transport")),
                    ("status", t("maintenance")),
                    ("base", t("FAC-SOUTH")),
                    ("flight_hours", Value::Int(5_307)),
                ],
            ),
            object(
                "aircraft",
                "AC-2307",
                marking("restricted", &[]),
                vec![
                    ("name", t("AC-2307")),
                    ("tail", t("MG-950")),
                    ("model", t("Surveillance")),
                    ("status", t("standby")),
                    ("base", t("FAC-SOUTH")),
                    ("flight_hours", Value::Int(3_188)),
                ],
            ),
            object(
                "facility",
                "FAC-NORTH",
                marking("restricted", &[]),
                vec![
                    ("name", t("Northern operating base")),
                    ("kind", t("airfield")),
                    ("status", t("operational")),
                    ("region", t("north")),
                ],
            ),
            object(
                "facility",
                "FAC-SOUTH",
                marking("restricted", &[]),
                vec![
                    ("name", t("Southern maintenance depot")),
                    ("kind", t("depot")),
                    ("status", t("operational")),
                    ("region", t("south")),
                ],
            ),
            object(
                "route",
                "RT-N-S",
                marking("internal", &[]),
                vec![
                    ("name", t("North–south corridor")),
                    ("origin", t("FAC-NORTH")),
                    ("destination", t("FAC-SOUTH")),
                ],
            ),
            object(
                "route",
                "RT-N-LOOP",
                marking("internal", &[]),
                vec![
                    ("name", t("Northern patrol circuit")),
                    ("origin", t("FAC-NORTH")),
                    ("destination", t("FAC-NORTH")),
                ],
            ),
            object(
                "crew_member",
                "CR-101",
                marking("internal", &[]),
                vec![
                    ("name", t("Crew 101")),
                    ("rank", t("Flight lieutenant")),
                    ("qualification", t("captain")),
                    ("clearance_review", t("current")),
                ],
            ),
            object(
                "crew_member",
                "CR-102",
                marking("internal", &[]),
                vec![
                    ("name", t("Crew 102")),
                    ("rank", t("Flying officer")),
                    ("qualification", t("co-pilot")),
                    ("clearance_review", t("current")),
                ],
            ),
            object(
                "crew_member",
                "CR-103",
                marking("internal", &[]),
                vec![
                    ("name", t("Crew 103")),
                    ("rank", t("Sergeant")),
                    ("qualification", t("loadmaster")),
                    ("clearance_review", t("current")),
                ],
            ),
            object(
                "crew_member",
                "CR-104",
                marking("internal", &[]),
                vec![
                    ("name", t("Crew 104")),
                    ("rank", t("Flight sergeant")),
                    ("qualification", t("air engineer")),
                    ("clearance_review", t("current")),
                ],
            ),
            object(
                "sensor_feed",
                "SF-TRANSPONDER-4",
                marking("internal", &[]),
                vec![
                    ("name", t("Transponder feed 4")),
                    ("modality", t("transponder")),
                    ("rate_hz", Value::Float(1.0)),
                    ("status", t("live")),
                ],
            ),
            // SIG-compartmented: the development operator cannot see this one.
            object(
                "sensor_feed",
                "SF-NORTH-2",
                marking("confidential", &["SIG"]),
                vec![
                    ("name", t("North radar 2")),
                    ("modality", t("radar")),
                    ("rate_hz", Value::Float(4.0)),
                    ("status", t("live")),
                ],
            ),
            object(
                "maintenance_event",
                "ME-88121",
                marking("internal", &[]),
                vec![
                    ("name", t("Hydraulic pressure drift · AC-2214")),
                    ("severity", t("moderate")),
                    ("status", t("open")),
                ],
            ),
        ];
        for o in objects {
            self.fabric.store_mut().put_object(o);
        }

        let link = |link_type: &str, source: &str, target: &str| LinkInstance {
            link_type: link_type.into(),
            source_id: source.into(),
            target_id: target.into(),
        };
        for l in [
            link("aircraft_operates", "aircraft/AC-2214", "route/RT-N-S"),
            link("aircraft_operates", "aircraft/AC-2261", "route/RT-N-S"),
            link("aircraft_operates", "aircraft/AC-2198", "route/RT-N-LOOP"),
            link("aircraft_operates", "aircraft/AC-2307", "route/RT-N-LOOP"),
            link("aircraft_crew", "aircraft/AC-2214", "crew_member/CR-101"),
            link("aircraft_crew", "aircraft/AC-2214", "crew_member/CR-102"),
            link("aircraft_crew", "aircraft/AC-2261", "crew_member/CR-103"),
            link("aircraft_crew", "aircraft/AC-2198", "crew_member/CR-104"),
            link("aircraft_base", "aircraft/AC-2214", "facility/FAC-NORTH"),
            link("aircraft_base", "aircraft/AC-2261", "facility/FAC-NORTH"),
            link("aircraft_base", "aircraft/AC-2198", "facility/FAC-SOUTH"),
            link("aircraft_base", "aircraft/AC-2307", "facility/FAC-SOUTH"),
            link(
                "maintenance_on",
                "maintenance_event/ME-88121",
                "aircraft/AC-2214",
            ),
            link(
                "sensor_observes",
                "sensor_feed/SF-TRANSPONDER-4",
                "aircraft/AC-2214",
            ),
            link(
                "sensor_observes",
                "sensor_feed/SF-NORTH-2",
                "aircraft/AC-2261",
            ),
        ] {
            self.fabric.store_mut().put_link(l);
        }
    }

    /// Register the development schema and seed its data set in one step — the
    /// in-memory development enclosure, which starts empty every time.
    pub fn seed_dev(&mut self, purpose: &str, now: u64) -> Result<(), FabricError> {
        self.register_dev_schema(purpose, now)?;
        self.seed_dev_objects(purpose);
        Ok(())
    }
}

// ---- projection helpers ---------------------------------------------------------

fn resource_of(obj: &ObjectInstance) -> Resource {
    Resource {
        id: ResourceId(obj.id.clone()),
        marking: obj.marking.clone(),
        allowed_purposes: obj.allowed_purposes.clone(),
    }
}

/// The per-object read check, exactly as the fabric's query engine applies it.
fn can_read<L: LedgerStore>(
    gov: &Governance<L>,
    ctx: &PolicyContext,
    obj: &ObjectInstance,
) -> bool {
    gov.permits(ctx, &resource_of(obj), "Read")
}

fn audit_id(seq: u64) -> String {
    format!("AUD-{seq}")
}

/// An approval's state as the console shows it.
fn approval_status(a: &Approval, now: u64) -> &'static str {
    match &a.state {
        ApprovalState::Pending => "pending",
        ApprovalState::Approved { expires, .. } if now >= *expires => "expired",
        ApprovalState::Approved { .. } => "approved",
        ApprovalState::Rejected { .. } => "rejected",
        ApprovalState::Used { .. } => "used",
    }
}

/// An approval as the console shows it to `subject`.
fn approval_json(a: &Approval, now: u64, subject: &str) -> Json {
    let secs = |t: u64| Json::Int(i64::try_from(t).unwrap_or(i64::MAX));
    let mut fields = vec![
        ("id", Json::s(&a.id)),
        ("actionType", Json::s(&a.action_title)),
        ("target", Json::s(&a.target)),
        ("requester", Json::s(&a.requester)),
        ("requestedAt", secs(a.requested)),
        ("status", Json::s(approval_status(a, now))),
        ("mine", Json::Bool(a.requester == subject)),
    ];
    match &a.state {
        ApprovalState::Pending => {}
        ApprovalState::Approved { approver, expires } => {
            fields.push(("approver", Json::s(approver)));
            fields.push(("expiresAt", secs(*expires)));
        }
        ApprovalState::Rejected { approver } | ApprovalState::Used { approver } => {
            fields.push(("approver", Json::s(approver)));
        }
    }
    Json::obj(fields)
}

fn link_label(reg: &TypeRegistry, lt: &LinkType) -> String {
    let target = reg
        .object_type(&lt.target_type)
        .map_or(lt.target_type.as_str(), |t| t.title.as_str());
    format!("{} → {target}", lt.api_name)
}

fn object_json(o: &ObjectInstance) -> Json {
    Json::obj(vec![
        ("id", Json::s(&o.id)),
        ("typeId", Json::s(&o.type_id)),
        ("propertyValues", property_values(o)),
        ("markings", Json::Arr(vec![marking_json(&o.marking)])),
    ])
}

/// An object's properties as strings, the way every view shows them.
fn property_values(o: &ObjectInstance) -> Json {
    Json::obj(
        o.properties
            .iter()
            .map(|(k, v)| (k.as_str(), Json::s(&v.as_index_key())))
            .collect(),
    )
}

fn marking_json(m: &Marking) -> Json {
    let level = band(m.level);
    let compartments: Vec<&str> = m.compartments.iter().map(|c| c.0.as_str()).collect();
    let name = if compartments.is_empty() {
        level.to_uppercase()
    } else {
        format!("{} // {}", level.to_uppercase(), compartments.join("/"))
    };
    Json::obj(vec![
        ("id", Json::s(&format!("mk-{level}"))),
        ("name", Json::s(&name)),
        ("level", Json::s(level)),
        (
            "compartments",
            Json::Arr(compartments.iter().map(|c| Json::s(c)).collect()),
        ),
    ])
}

fn node_json(reg: &TypeRegistry, o: &ObjectInstance, hop: u64) -> Json {
    let label = match o.properties.get("name") {
        Some(Value::Text(name)) => name.clone(),
        _ => o.id.clone(),
    };
    let type_name = reg
        .object_type(&o.type_id)
        .map_or_else(|| o.type_id.clone(), |t| t.title.clone());
    Json::obj(vec![
        ("id", Json::s(&o.id)),
        ("label", Json::s(&label)),
        ("typeName", Json::s(&type_name)),
        ("marking", Json::s(band(o.marking.level))),
        ("hop", Json::Int(i64::try_from(hop).unwrap_or(i64::MAX))),
        // Every node reached is one the caller may read, so its properties are theirs
        // to see; the console shows them instead of placeholders. [NFR-SEC-2]
        ("propertyValues", property_values(o)),
    ])
}

/// Recompute the entry hash and verify the ML-DSA signature over it.
fn signature_verifies(provider: &CryptoProvider, key: &VerifyingKey, e: &SealedEntry) -> bool {
    if entry_hash(e.seq, &e.prev_hash, &e.record) != e.entry_hash {
        return false;
    }
    match provider.sig_for(e.sig_alg) {
        Ok(scheme) => scheme
            .verify(key, &signing_preimage(&e.entry_hash), &e.pq_signature)
            .is_ok(),
        Err(_) => false,
    }
}

fn describe(reason: &DecisionReason) -> String {
    match reason {
        DecisionReason::ClearanceInsufficient { needed, held } => format!(
            "Clearance {} is below the {} marking — no read-up.",
            band(*held).to_uppercase(),
            band(*needed).to_uppercase()
        ),
        DecisionReason::CompartmentMissing { missing } => format!(
            "Missing compartment(s): {}.",
            missing
                .iter()
                .map(|c| c.0.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        DecisionReason::PurposeNotPermitted { purpose } => format!(
            "Purpose \"{}\" is not authorised for this session.",
            purpose.0
        ),
        DecisionReason::ExplicitDeny { rule } => format!("Explicit deny by rule {}.", rule.0),
        DecisionReason::Permitted { rule } => format!("Granted by rule {}.", rule.0),
        DecisionReason::NoMatchingRule => "Deny by default — no matching allow rule.".to_string(),
    }
}

/// `HH:MM:SS` (UTC) from a Unix-seconds ledger timestamp.
fn time_of_day(ts: &str) -> String {
    ts.parse::<u64>().map_or_else(
        |_| ts.to_string(),
        |secs| {
            let s = secs % 86_400;
            format!("{:02}:{:02}:{:02}", s / 3_600, s % 3_600 / 60, s % 60)
        },
    )
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn ago(now: u64, then: Option<u64>) -> String {
    let Some(then) = then else {
        return "—".to_string();
    };
    match now.saturating_sub(then) {
        d @ 0..=59 => format!("{d}s ago"),
        d @ 60..=3_599 => format!("{}m ago", d / 60),
        d @ 3_600..=86_399 => format!("{}h ago", d / 3_600),
        d => format!("{}d ago", d / 86_400),
    }
}

fn marking(band: &str, compartments: &[&str]) -> Marking {
    Marking {
        level: level_of(band).unwrap_or_else(|| panic!("unknown marking band {band}")),
        compartments: compartments
            .iter()
            .map(|c| Compartment((*c).to_string()))
            .collect(),
    }
}

// ---- development action functions ------------------------------------------------

fn dev_function(action_id: &str) -> Box<dyn ActionFunction> {
    let set = |property: &str, value: &str| -> Box<dyn ActionFunction> {
        Box::new(SetProperty {
            property: property.into(),
            value: value.into(),
        })
    };
    match action_id {
        "ground_aircraft" => set("status", "grounded"),
        "schedule_maintenance" => Box::new(ScheduleMaintenance),
        "change_facility_status" => Box::new(CycleProperty {
            property: "status".into(),
            values: vec!["operational".into(), "degraded".into()],
        }),
        "request_clearance_review" => set("clearance_review", "requested"),
        "recalibrate_sensor" => set("status", "calibrating"),
        "close_maintenance_event" => set("status", "closed"),
        other => panic!("DEV_ACTIONS entry {other} has no bound function"),
    }
}

/// The object ids the edge resolved for this action — only ever ids the caller may
/// read — carried as one comma-separated text parameter.
fn target_ids(params: &BTreeMap<String, Value>) -> Result<Vec<String>, FabricError> {
    match params.get("targets") {
        Some(Value::Text(s)) => Ok(s
            .split(',')
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .collect()),
        _ => Err(FabricError::MissingRequiredParam("targets".into())),
    }
}

fn existing<'a>(store: &'a dyn ObjectStore, id: &str) -> Result<&'a ObjectInstance, FabricError> {
    store
        .get_object(id)
        .ok_or_else(|| FabricError::Store(format!("target no longer exists: {id}")))
}

/// Set one text property to a fixed value on every target.
struct SetProperty {
    property: String,
    value: String,
}

impl ActionFunction for SetProperty {
    fn apply(
        &self,
        params: &BTreeMap<String, Value>,
        store: &dyn ObjectStore,
    ) -> Result<Vec<Effect>, FabricError> {
        target_ids(params)?
            .iter()
            .map(|id| {
                let mut obj = existing(store, id)?.clone();
                obj.properties.insert(
                    self.property.to_string(),
                    Value::Text(self.value.to_string()),
                );
                Ok(Effect::PutObject(obj))
            })
            .collect()
    }
}

/// Advance one text property to the next value in a fixed cycle on every target.
struct CycleProperty {
    property: String,
    values: Vec<String>,
}

impl ActionFunction for CycleProperty {
    fn apply(
        &self,
        params: &BTreeMap<String, Value>,
        store: &dyn ObjectStore,
    ) -> Result<Vec<Effect>, FabricError> {
        target_ids(params)?
            .iter()
            .map(|id| {
                let mut obj = existing(store, id)?.clone();
                let current = match obj.properties.get(&self.property) {
                    Some(Value::Text(s)) => self.values.iter().position(|v| v == s),
                    _ => None,
                };
                let next = &self.values[current.map_or(0, |i| (i + 1) % self.values.len())];
                obj.properties
                    .insert(self.property.clone(), Value::Text(next.clone()));
                Ok(Effect::PutObject(obj))
            })
            .collect()
    }
}

/// Open a maintenance event against each target aircraft, and link it.
struct ScheduleMaintenance;

impl ActionFunction for ScheduleMaintenance {
    fn apply(
        &self,
        params: &BTreeMap<String, Value>,
        store: &dyn ObjectStore,
    ) -> Result<Vec<Effect>, FabricError> {
        let mut next = store.objects_of_type("maintenance_event").len();
        let mut effects = Vec::new();
        for id in target_ids(params)? {
            let aircraft = existing(store, &id)?;
            next += 1;
            let event_id = format!("maintenance_event/ME-{}", 88_120 + next);
            let label = match aircraft.properties.get("name") {
                Some(Value::Text(name)) => name.clone(),
                _ => id.clone(),
            };
            let properties = [
                ("name", format!("Scheduled maintenance · {label}")),
                ("severity", "routine".to_string()),
                ("status", "scheduled".to_string()),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), Value::Text(v)))
            .collect();
            effects.push(Effect::PutObject(ObjectInstance {
                id: event_id.clone(),
                type_id: "maintenance_event".into(),
                properties,
                marking: marking("internal", &[]),
                allowed_purposes: aircraft.allowed_purposes.clone(),
            }));
            effects.push(Effect::PutLink(LinkInstance {
                link_type: "maintenance_on".into(),
                source_id: event_id,
                target_id: id,
            }));
        }
        Ok(effects)
    }
}
