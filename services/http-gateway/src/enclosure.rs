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

use audit_writer::{
    entry_hash, genesis_hash, signing_preimage, AuditContext, Outcome, SealedEntry, SealedJournal,
};
use fabric::{
    ActionFunction, ActionType, Cardinality, Effect, Fabric, FabricError, FunctionRuntime,
    Governance, InMemoryLedger, InMemoryStore, LedgerStore, LinkInstance, LinkType, ObjectInstance,
    ObjectStore, ObjectType, PolicyContext, PropertyType, Search, TypeRegistry, Value, ValueType,
};
use policy::{
    ClassificationLevel, Compartment, DecisionReason, Effect as RuleEffect, Marking, Purpose,
    Resource, ResourceId, Selector,
};
use pqc_agility::{CryptoProvider, VerifyingKey};

use crate::approval_journal::ApprovalFrame;
use crate::json::Json;
use crate::ontology_file::{ActionEffect, Ontology};

use service_runtime::marking::BANDS;
/// The console's four marking bands, lowest to highest, mapped to policy levels by
/// ordinal (see `service_runtime::marking`), shared with the mesh services.
pub use service_runtime::marking::{band, level_of};

/// Nodes a search-around returns when the caller names no limit.
const DEFAULT_NODE_LIMIT: i64 = 60;
/// Most nodes a search-around returns, whatever the caller asks for.
const MAX_NODE_LIMIT: i64 = 500;
/// Deepest search-around.
const MAX_HOPS: i64 = 4;
/// Most matches one page of a search returns.
const MAX_PAGE: i64 = 500;
/// How long an approved human gate can be redeemed (seconds).
const APPROVAL_TTL: u64 = 900;
/// Most approvals the enclosure holds. Expired entries are pruned, then rejected
/// and used ones make room, oldest first; a live (pending or approved) entry is never
/// evicted, and a request that finds no room is refused with 429. [VLD-ANB-MIDGARD-0039]
const MAX_APPROVALS: usize = 256;
/// How long a pending approval waits for a decision before it expires (seconds).
const PENDING_TTL: u64 = 86_400;
/// Most open approvals (pending, or approved and unexpired) one requester may hold.
const MAX_OPEN_PER_REQUESTER: usize = 16;
/// How long a caller refused for a full approval queue is asked to wait (seconds).
const APPROVAL_RETRY_AFTER: u64 = 60;

/// A request the enclosure refused, already mapped to its HTTP status.
pub struct EdgeError {
    /// HTTP status code.
    pub status: u16,
    /// JSON error body.
    pub body: Json,
    /// What went wrong, for the gateway's log only; never sent to the client.
    /// [VLD-ANB-MIDGARD-0053]
    pub detail: Option<String>,
    /// For a 429, the seconds the client is asked to wait (`Retry-After`).
    /// [VLD-ANB-MIDGARD-0039]
    pub retry_after: Option<u64>,
}

impl EdgeError {
    fn new(status: u16, message: &str) -> Self {
        Self {
            status,
            body: Json::obj(vec![("error", Json::s(message))]),
            detail: None,
            retry_after: None,
        }
    }

    /// Too many requests: refused with 429 and a `Retry-After`. [VLD-ANB-MIDGARD-0039]
    fn too_many(message: &str) -> Self {
        Self {
            retry_after: Some(APPROVAL_RETRY_AFTER),
            ..Self::new(429, message)
        }
    }

    /// A server error: the client reads `internal error` and the request's reference,
    /// which the gateway adds, and `detail` goes to the gateway's log under that
    /// reference. [VLD-ANB-MIDGARD-0053]
    pub fn internal(detail: impl Into<String>) -> Self {
        Self {
            detail: Some(detail.into()),
            ..Self::new(500, "internal error")
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
            detail: None,
            retry_after: None,
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
/// expires. A pending approval expires after [`PENDING_TTL`]. Each state change is
/// written to the signed ledger as an `APPROVAL ...` row on `approval:<id>`, and, in a
/// served enclosure, to the sealed approvals journal, so approvals survive a restart.
/// [VLD-ANB-MIDGARD-0038] [VLD-ANB-MIDGARD-0039]
///
/// While an approval is pending, approved and unexpired, or redeemed, its target is
/// protected from mesh sync ([`Enclosure::gated_targets`]) until the approval is
/// pruned, so a sync cannot overwrite a change a named human authorised.
/// [VLD-ANB-MIDGARD-0001]
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
    /// Where each approval state change is journalled; `None` keeps approvals in
    /// memory only (the development gateway). [VLD-ANB-MIDGARD-0039]
    approval_journal: Option<SealedJournal>,
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
            approval_journal: None,
        }
    }

    /// Journal approvals to `journal`, first replaying the frames `recovered` from it
    /// in order, so the approvals open when it was last written are open again. The
    /// next approval id follows the highest one seen. A frame that does not decode, or
    /// names an approval the journal never opened, refuses the boot.
    /// [VLD-ANB-MIDGARD-0039]
    pub fn with_approval_journal(
        mut self,
        journal: SealedJournal,
        recovered: Vec<Vec<u8>>,
    ) -> Result<Self, String> {
        let mut approvals: Vec<Approval> = Vec::new();
        let mut next = 1u64;
        for bytes in &recovered {
            let frame = ApprovalFrame::decode(bytes)?;
            let unknown = |id: &str| format!("approval record names unknown approval {id}");
            match frame {
                ApprovalFrame::Request {
                    id,
                    action_id,
                    action_title,
                    target,
                    requester,
                    requested,
                } => {
                    if let Some(n) = id.strip_prefix("APR-").and_then(|n| n.parse::<u64>().ok()) {
                        next = next.max(n.saturating_add(1));
                    }
                    approvals.push(Approval {
                        id,
                        action_id,
                        action_title,
                        target,
                        requester,
                        requested,
                        state: ApprovalState::Pending,
                    });
                }
                ApprovalFrame::Decide {
                    id,
                    approved,
                    approver,
                    expires,
                } => {
                    let a = approvals
                        .iter_mut()
                        .find(|a| a.id == id)
                        .ok_or_else(|| unknown(&id))?;
                    a.state = if approved {
                        ApprovalState::Approved { approver, expires }
                    } else {
                        ApprovalState::Rejected { approver }
                    };
                }
                ApprovalFrame::Use { id } => {
                    let a = approvals
                        .iter_mut()
                        .find(|a| a.id == id)
                        .ok_or_else(|| unknown(&id))?;
                    let approver = match &a.state {
                        ApprovalState::Approved { approver, .. } => approver.clone(),
                        _ => {
                            return Err(format!("approval record uses {id}, which is not approved"))
                        }
                    };
                    a.state = ApprovalState::Used { approver };
                }
                ApprovalFrame::Prune { id } => {
                    let i = approvals
                        .iter()
                        .position(|a| a.id == id)
                        .ok_or_else(|| unknown(&id))?;
                    approvals.remove(i);
                }
            }
        }
        self.approvals = approvals;
        self.next_approval = next;
        self.approval_journal = Some(journal);
        Ok(self)
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
                        (
                            "properties",
                            Json::Arr(
                                ot.properties
                                    .iter()
                                    .map(|p| {
                                        Json::obj(vec![
                                            ("name", Json::s(&p.api_name)),
                                            ("type", Json::s(p.value_type.as_str())),
                                        ])
                                    })
                                    .collect(),
                            ),
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

    /// `POST /query`: a search of one object type — conditions beyond equality, text
    /// matching, ordering and paging — authorised and audited once, with every match
    /// marking-filtered against the caller. `total` counts only what they may read.
    /// A search naming a property the type lacks, or a value it cannot hold, is
    /// refused with 400 rather than quietly matching nothing. [DD-1] [NFR-SEC-2]
    pub fn query(
        &mut self,
        ctx: &PolicyContext,
        mut s: Search,
        now: u64,
    ) -> Result<Json, EdgeError> {
        s.limit = s.limit.clamp(1, usize::try_from(MAX_PAGE).unwrap_or(500));
        if let Err(problem) = fabric::search::check(self.fabric.registry(), &s) {
            return Err(
                if self.fabric.registry().object_type(&s.object_type).is_none() {
                    EdgeError::new(404, &problem.0)
                } else {
                    EdgeError::bad_request(&problem.0)
                },
            );
        }
        let page = match self.fabric.search(ctx, &s, now) {
            Ok(page) => page,
            Err(e) => return Err(self.refusal(e)),
        };
        let int = |n: usize| Json::Int(i64::try_from(n).unwrap_or(i64::MAX));
        Ok(Json::obj(vec![
            (
                "results",
                Json::Arr(page.results.iter().map(object_json).collect()),
            ),
            ("total", int(page.total)),
            ("offset", int(s.offset)),
            ("limit", int(s.limit)),
            ("auditId", Json::s(&audit_id(page.audit_seq))),
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
            let target = params.get("target").map(String::as_str).unwrap_or_default();
            let refused = EdgeError::new(
                403,
                &format!("Human gate required for action \"{}\"", entry.title),
            );
            return Err(self.refused(
                ctx,
                "Apply",
                "human-gate-missing",
                None,
                target,
                now,
                refused,
            ));
        }
        let target = params.remove("target").unwrap_or_default();
        let target = self.readable_target(ctx, &entry.object_type, &target)?;
        let approval = match gate {
            Some(id) if entry.human_gated => match self.redeemable(ctx, &entry, &target, id, now) {
                Ok(i) => Some(i),
                Err(refused) => {
                    return Err(self.refused(
                        ctx,
                        "Apply",
                        "human-gate-invalid",
                        Some(id),
                        &target,
                        now,
                        refused,
                    ))
                }
            },
            _ => None,
        };
        // A gated action's lineage row names the approval that authorised it, its
        // approver and its target. [VLD-ANB-MIDGARD-0038]
        let audit = match approval {
            Some(i) => {
                let a = &self.approvals[i];
                AuditContext {
                    approval_id: Some(a.id.clone()),
                    approver: approver_of(a).map(str::to_string),
                    target: Some(target.clone()),
                    ..AuditContext::default()
                }
            }
            None => AuditContext::default(),
        };
        params.insert("targets".into(), target);

        let resp = match self
            .fabric
            .apply_action_with(ctx, &entry.id, &params, now, audit)
        {
            Ok(resp) => resp,
            Err(e) => return Err(self.refusal(e)),
        };
        if resp.applied {
            self.updated.insert(entry.object_type, now);
            if let Some(i) = approval {
                self.redeem(ctx, i, now)?;
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
        self.prune_approvals(now)?;
        let open = self
            .approvals
            .iter()
            .filter(|a| a.requester == ctx.subject && is_open(a, now))
            .count();
        let refusal = if open >= MAX_OPEN_PER_REQUESTER {
            Some((
                "approval-quota",
                "too many open approval requests for this user",
            ))
        } else if self.approvals.len() >= MAX_APPROVALS {
            Some((
                "approval-queue-full",
                "the approval queue is full; try again later",
            ))
        } else {
            None
        };
        if let Some((reason, message)) = refusal {
            let refused = EdgeError::too_many(message);
            return Err(self.refused(ctx, "Apply", reason, None, &target, now, refused));
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
        // Journalled first, so an id that reaches the ledger is never handed out again
        // after a restart; if the ledger then refuses the row, the journal entry is
        // withdrawn and the approval is not kept. [VLD-ANB-MIDGARD-0038]
        // [VLD-ANB-MIDGARD-0039]
        self.journal_approval(&ApprovalFrame::Request {
            id: approval.id.clone(),
            action_id: approval.action_id.clone(),
            action_title: approval.action_title.clone(),
            target: approval.target.clone(),
            requester: approval.requester.clone(),
            requested: approval.requested,
        })?;
        self.next_approval += 1;
        if let Err(e) = self.approval_row(&ctx.subject, "APPROVAL requested", &approval, now) {
            let _ = self.journal_approval(&ApprovalFrame::Prune {
                id: approval.id.clone(),
            });
            return Err(e);
        }
        let json = approval_json(&approval, now, &ctx.subject);
        self.approvals.push(approval);
        Ok(json)
    }

    /// Make room in the approval store. Every expired entry (pending past
    /// [`PENDING_TTL`], or approved past its redemption window) is removed and written
    /// to the ledger as `APPROVAL expired`; then, while the store is full, rejected and
    /// then used entries are removed, oldest first. A live entry is never removed.
    /// Each removal is journalled; a failure stops the prune and fails the request.
    /// [VLD-ANB-MIDGARD-0039]
    fn prune_approvals(&mut self, now: u64) -> Result<(), EdgeError> {
        while let Some(i) = self.approvals.iter().position(|a| is_expired(a, now)) {
            let a = self.approvals[i].clone();
            // The row is the requester's: it is their request that lapsed.
            self.approval_row(&a.requester, "APPROVAL expired", &a, now)?;
            self.journal_approval(&ApprovalFrame::Prune { id: a.id })?;
            self.approvals.remove(i);
        }
        let settled: [fn(&ApprovalState) -> bool; 2] = [
            |s| matches!(s, ApprovalState::Rejected { .. }),
            |s| matches!(s, ApprovalState::Used { .. }),
        ];
        for is_settled in settled {
            while self.approvals.len() >= MAX_APPROVALS {
                let Some(i) = self.approvals.iter().position(|a| is_settled(&a.state)) else {
                    break;
                };
                let id = self.approvals[i].id.clone();
                self.journal_approval(&ApprovalFrame::Prune { id })?;
                self.approvals.remove(i);
            }
        }
        Ok(())
    }

    /// Mark approval `i` used once its action has run: journalled, so it cannot be
    /// redeemed again after a restart, and written to the ledger as `APPROVAL
    /// redeemed`. The action has already applied, so if either write fails the
    /// approval is still marked used here and the request reports a server error.
    /// [VLD-ANB-MIDGARD-0038] [VLD-ANB-MIDGARD-0039]
    fn redeem(&mut self, ctx: &PolicyContext, i: usize, now: u64) -> Result<(), EdgeError> {
        let a = &mut self.approvals[i];
        let ApprovalState::Approved { approver, .. } = &a.state else {
            return Ok(());
        };
        a.state = ApprovalState::Used {
            approver: approver.clone(),
        };
        let a = a.clone();
        let journalled = self
            .journal_approval(&ApprovalFrame::Use { id: a.id.clone() })
            .map_err(|e| {
                EdgeError::internal(format!(
                    "applied, but the redemption was not journalled: {}",
                    e.detail.unwrap_or_default()
                ))
            });
        let audited = self.approval_row(&ctx.subject, "APPROVAL redeemed", &a, now);
        journalled.and(audited)
    }

    /// Write one approval lifecycle row, `event` on `approval:<id>`, with the
    /// approval's id, its approver (once decided), its target and, as the reason, the
    /// action it gates. [VLD-ANB-MIDGARD-0038]
    fn approval_row(
        &mut self,
        actor: &str,
        event: &str,
        a: &Approval,
        now: u64,
    ) -> Result<(), EdgeError> {
        let context = AuditContext {
            outcome: Outcome::Info,
            reason: Some(a.action_id.clone()),
            approval_id: Some(a.id.clone()),
            approver: approver_of(a).map(str::to_string),
            target: Some(a.target.clone()),
            ..AuditContext::default()
        };
        self.fabric
            .governance_mut()
            .record_event_with(actor, event, &format!("approval:{}", a.id), now, context)
            .map(|_| ())
            .map_err(|e| EdgeError::internal(format!("approval not audited: {e}")))
    }

    /// Append one approval state change to the journal, if there is one.
    /// [VLD-ANB-MIDGARD-0039]
    fn journal_approval(&mut self, frame: &ApprovalFrame) -> Result<(), EdgeError> {
        match &mut self.approval_journal {
            None => Ok(()),
            Some(journal) => journal
                .append(&frame.encode())
                .map_err(|e| EdgeError::internal(format!("approval not journalled: {e}"))),
        }
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
        let a = &self.approvals[i];
        let target = a.target.clone();
        if a.state != ApprovalState::Pending || is_expired(a, now) {
            let refused = EdgeError::new(
                409,
                &format!("approval {id} is already {}", approval_status(a, now)),
            );
            return Err(self.refused(
                ctx,
                "Approve",
                "approval-not-pending",
                Some(id),
                &target,
                now,
                refused,
            ));
        }
        if a.requester == ctx.subject {
            let refused = EdgeError::new(
                403,
                "an approval must be decided by someone other than its requester",
            );
            return Err(self.refused(
                ctx,
                "Approve",
                "self-approval",
                Some(id),
                &target,
                now,
                refused,
            ));
        }
        let approver = ctx.subject.clone();
        let expires = now.saturating_add(APPROVAL_TTL);
        let mut decided = self.approvals[i].clone();
        decided.state = if approve {
            ApprovalState::Approved {
                approver: approver.clone(),
                expires,
            }
        } else {
            ApprovalState::Rejected {
                approver: approver.clone(),
            }
        };
        // The decision is in the ledger, then the journal, before it takes effect; a
        // failure of either leaves the approval pending. [VLD-ANB-MIDGARD-0038]
        let event = if approve {
            "APPROVAL approved"
        } else {
            "APPROVAL rejected"
        };
        self.approval_row(&ctx.subject, event, &decided, now)?;
        self.journal_approval(&ApprovalFrame::Decide {
            id: decided.id.clone(),
            approved: approve,
            approver,
            expires: if approve { expires } else { 0 },
        })?;
        let json = approval_json(&decided, now, &ctx.subject);
        self.approvals[i] = decided;
        Ok(json)
    }

    /// Record a refusal in the ledger before it is returned: the edge has already
    /// written its PERMIT row for the request, so without this the ledger would show a
    /// permit and nothing of the refusal. The row is `REFUSED <verb> (<reason>)` on
    /// the target, with the reason code, approval and target in its context. A
    /// refusal the ledger cannot record is a server error. [NFR-AI-1] [AC-M-4]
    /// [VLD-ANB-MIDGARD-0054]
    #[allow(clippy::too_many_arguments)]
    fn refused(
        &mut self,
        ctx: &PolicyContext,
        verb: &str,
        reason: &str,
        approval_id: Option<&str>,
        target: &str,
        now: u64,
        refusal: EdgeError,
    ) -> EdgeError {
        // A target the caller named but that was not resolved is recorded as given,
        // stripped of control characters and capped.
        let target: String = target
            .chars()
            .filter(|c| !c.is_control())
            .take(128)
            .collect();
        let resource = if target.is_empty() {
            "/api/v1/actions".to_string()
        } else {
            target.clone()
        };
        let context = AuditContext {
            outcome: Outcome::Refused,
            reason: Some(reason.to_string()),
            approval_id: approval_id
                .map(|id| id.chars().filter(|c| !c.is_control()).take(64).collect()),
            target: (!target.is_empty()).then_some(target),
            ..AuditContext::default()
        };
        match self.fabric.governance_mut().record_event_with(
            &ctx.subject,
            &format!("REFUSED {verb} ({reason})"),
            &resource,
            now,
            context,
        ) {
            Ok(_) => refusal,
            Err(e) => EdgeError::internal(format!("refusal not audited: {e}")),
        }
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

    /// The objects under a human gate at `now`: the target of every approval that is
    /// pending, approved and unexpired, or redeemed. A mesh sync may not touch them.
    /// A rejected or expired approval protects nothing. [NFR-AI-1]
    /// [VLD-ANB-MIDGARD-0001]
    pub fn gated_targets(&self, now: u64) -> BTreeSet<String> {
        self.approvals
            .iter()
            .filter(|a| match &a.state {
                ApprovalState::Pending | ApprovalState::Used { .. } => true,
                ApprovalState::Approved { expires, .. } => now < *expires,
                ApprovalState::Rejected { .. } => false,
            })
            .map(|a| a.target.clone())
            .collect()
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
    ///
    /// At most `limit` nodes come back (60 by default, 500 at most). When the walk
    /// had more to show — more starting objects than the limit, or more objects
    /// reached — the response says so with `truncated`, so a partial graph is never
    /// shown as the whole. `seedObjects` counts the starting objects the caller may
    /// read. [DD-1]
    ///
    /// Seed resolution ([`Self::seed_roots`]), link collection ([`adjacent_links`])
    /// and the per-node clearance filter ([`Walk::admit`]) are separate so each can be
    /// reasoned about, and tested, on its own. [VLD-ANB-MIDGARD-0094]
    pub fn search_around(
        &mut self,
        ctx: &PolicyContext,
        seed: &str,
        hops: i64,
        limit: Option<i64>,
        now: u64,
    ) -> Result<Json, EdgeError> {
        let hops = u64::try_from(hops.clamp(1, MAX_HOPS)).unwrap_or(1);
        let node_limit = limit.unwrap_or(DEFAULT_NODE_LIMIT).clamp(1, MAX_NODE_LIMIT);
        let node_limit = usize::try_from(node_limit).unwrap_or(60);

        let (roots, resource) = self.seed_roots(ctx, seed)?;
        let decision = self
            .fabric
            .governance_mut()
            .authorize(ctx, &resource, "Read", now)
            .map_err(|e| EdgeError::internal(format!("audit failure: {e}")))?;
        if !decision.permitted {
            return Err(EdgeError::denied(decision.audit_seq));
        }

        let reg = self.fabric.registry();
        let store = self.fabric.store();
        let gov = self.fabric.governance();
        let readable = |o: &ObjectInstance| can_read(gov, ctx, o);
        let mut walk = Walk::new(node_limit);
        walk.seed(reg, store, &roots);
        for hop in 1..=hops {
            // Nodes admitted on this hop become the next hop's frontier.
            for id in std::mem::take(&mut walk.frontier) {
                let Some(node) = store.get_object(&id) else {
                    continue;
                };
                for link in adjacent_links(reg, store, node) {
                    walk.admit(&readable, reg, node, link, hop);
                }
            }
        }
        Ok(walk.into_json(seed, roots.len(), decision.audit_seq))
    }

    /// The objects a search-around starts from and the resource its one audited
    /// decision is taken on. `<type>/*` starts from every object of the type the
    /// caller may read, authorised on the type; anything else names one object, which
    /// must exist and be readable, and is authorised as [`seed_resource`] says. An
    /// unknown type, or a seed that is missing or unreadable, is 404 before anything
    /// is audited, so the ledger does not reveal that a hidden seed exists.
    fn seed_roots(
        &self,
        ctx: &PolicyContext,
        seed: &str,
    ) -> Result<(Vec<String>, Resource), EdgeError> {
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
                    .map(|o| o.id.clone())
                    .collect();
                Ok((ids, fabric::type_resource(ty, &ctx.purpose)))
            }
            _ => match store.get_object(seed) {
                Some(o) if can_read(gov, ctx, o) => {
                    Ok((vec![o.id.clone()], seed_resource(gov, ctx, o)))
                }
                _ => Err(EdgeError::new(404, &format!("unknown seed object: {seed}"))),
            },
        }
    }

    // ---- governance -------------------------------------------------------------

    /// `GET /audit`: the governance ledger, newest first.
    ///
    /// Each served row is re-verified — its entry hash recomputed, its link to the
    /// previous entry checked, and its ML-DSA signature verified — and marked `✓` only
    /// if all three hold. The cost is per row served, not per ledger entry, so a
    /// console polling this view does not pay for the whole chain on every poll.
    ///
    /// The view is scoped to the caller (see [`Self::audit_row_visible`]). Rows the
    /// caller may not see are omitted, not redacted, and are dropped before `limit` is
    /// applied, so up to `limit` visible rows come back. The response stays a bare
    /// array; the second value says whether any row was omitted, which the edge sends
    /// as the `X-Audit-Filtered` header. [VLD-ANB-MIDGARD-0031]
    pub fn audit(
        &self,
        ctx: &PolicyContext,
        provider: &CryptoProvider,
        limit: usize,
    ) -> (Json, bool) {
        let entries = self.fabric.audit_entries();
        let key = self.fabric.audit_verifying_key();
        let genesis = genesis_hash(self.fabric.audit_ledger_id());
        let mut filtered = false;
        let rows = Json::Arr(
            entries
                .iter()
                .rev()
                .filter(|e| {
                    let visible = self.audit_row_visible(ctx, e);
                    filtered |= !visible;
                    visible
                })
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
                    let mut row = vec![
                        ("id", Json::s(&audit_id(e.seq))),
                        ("actor", Json::s(&e.record.actor.0)),
                        ("action", Json::s(&e.record.action.0)),
                        ("resource", Json::s(&e.record.resource.0)),
                        ("ts", Json::s(&time_of_day(&e.record.ts.0))),
                        ("pqSignature", Json::s(&marker)),
                    ];
                    // The row's context, where it states one: how it ended, why, and
                    // for a human gate the approval, its approver and its target.
                    // [VLD-ANB-MIDGARD-0038]
                    let c = &e.record.context;
                    if c.outcome != Outcome::Unspecified {
                        row.push(("outcome", Json::s(c.outcome.as_str())));
                    }
                    for (key, value) in [
                        ("reason", &c.reason),
                        ("approvalId", &c.approval_id),
                        ("approver", &c.approver),
                        ("target", &c.target),
                    ] {
                        if let Some(value) = value {
                            row.push((key, Json::s(value)));
                        }
                    }
                    Json::obj(row)
                })
                .collect(),
        );
        (rows, filtered)
    }

    /// Whether the caller may see ledger row `entry`. A row about an object is shown
    /// only if the caller may read that object now (the same non-auditing check that
    /// filters query results), so the view is not an oracle for hidden objects. A row
    /// naming a target — an approval's lifecycle rows, an action's own row — is shown
    /// only if the caller may read that target, as the approvals list is filtered. A
    /// login row about another principal needs `ReadAudit` on `audit-ledger:sessions`.
    /// Every other row (route admissions, dry-runs, the caller's own logins) is shown
    /// to any caller the edge admitted. [VLD-ANB-MIDGARD-0031] [VLD-ANB-MIDGARD-0038]
    fn audit_row_visible(&self, ctx: &PolicyContext, entry: &SealedEntry) -> bool {
        if let Some(target) = &entry.record.context.target {
            if !self.can_read_id(ctx, target) {
                return false;
            }
        }
        let resource = entry.record.resource.0.as_str();
        if let Some(id) = resource.strip_prefix(fabric::OBJECT_PREFIX) {
            return self.can_read_id(ctx, id);
        }
        if resource == "session" && entry.record.actor.0 != ctx.subject {
            let sessions = Resource {
                id: ResourceId(crate::AUDIT_SESSIONS_RESOURCE.into()),
                marking: Marking {
                    level: ClassificationLevel::UNCLASSIFIED,
                    compartments: BTreeSet::new(),
                },
                allowed_purposes: [ctx.purpose.clone()].into_iter().collect(),
            };
            return self
                .fabric
                .governance()
                .permits(ctx, &sessions, crate::ACTION_READ_AUDIT);
        }
        true
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
            _ => EdgeError::internal(e.to_string()),
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

// ---- search-around ---------------------------------------------------------------

/// A link touching a node: (link type, source, target).
type Adjacent<'a> = (&'a LinkType, &'a ObjectInstance, &'a ObjectInstance);

/// The resource a single-seed search-around is authorised and audited on.
///
/// The seed has already passed [`can_read`], so no rule denies it by its object id
/// or by its type's id, and a permit on one of the two grants it. The audited
/// decision is taken on the object's own id (`object:<type>/<key>`) when a rule
/// permits that, and otherwise on its type's id (`object-type:<type>`), always under
/// the object's own marking and purposes so mandatory access control applies to the
/// seed. A policy that permits only `object-type:<type>` therefore walks from one
/// object of the type exactly as it reads that object, instead of refusing the walk.
/// [VLD-ANB-MIDGARD-0023] [VLD-ANB-MIDGARD-0094] [NFR-SEC-2]
fn seed_resource<L: LedgerStore>(
    gov: &Governance<L>,
    ctx: &PolicyContext,
    obj: &ObjectInstance,
) -> Resource {
    let instance = resource_of(obj);
    if gov.permits(ctx, &instance, "Read") {
        return instance;
    }
    Resource {
        id: fabric::type_resource_id(&obj.type_id),
        ..instance
    }
}

/// Every link touching `node`, in the order a search-around meets them: for each link
/// type in registry order, the node's outbound links (through `neighbors`) and then
/// its inbound ones (by scanning the link type's source objects). Edge order in a
/// response follows this order. [VLD-ANB-MIDGARD-0094]
fn adjacent_links<'a>(
    reg: &'a TypeRegistry,
    store: &'a dyn ObjectStore,
    node: &'a ObjectInstance,
) -> Vec<Adjacent<'a>> {
    let mut adjacent = Vec::new();
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
    adjacent
}

/// One search-around in progress: the nodes and edges admitted so far, in discovery
/// order, and the frontier the next hop walks from.
struct Walk {
    seen: BTreeSet<String>,
    edge_keys: BTreeSet<(String, String, String)>,
    nodes: Vec<Json>,
    edges: Vec<Json>,
    frontier: Vec<String>,
    truncated: bool,
    node_limit: usize,
}

impl Walk {
    fn new(node_limit: usize) -> Self {
        Self {
            seen: BTreeSet::new(),
            edge_keys: BTreeSet::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            frontier: Vec::new(),
            truncated: false,
            node_limit,
        }
    }

    /// Admit the starting objects as hop-0 nodes, at most `node_limit` of them; more
    /// than that marks the walk truncated.
    fn seed(&mut self, reg: &TypeRegistry, store: &dyn ObjectStore, roots: &[String]) {
        self.truncated = roots.len() > self.node_limit;
        for id in roots.iter().take(self.node_limit) {
            if let Some(o) = store.get_object(id) {
                if self.seen.insert(o.id.clone()) {
                    self.nodes.push(node_json(reg, o, 0));
                    self.frontier.push(o.id.clone());
                }
            }
        }
    }

    /// The clearance filter, applied to one link touching `node` on hop `hop`.
    ///
    /// The neighbour (the link's other end) is checked with `readable` first: one the
    /// caller may not read contributes neither a node nor an edge. A neighbour not yet
    /// seen when the node limit is reached marks the walk truncated and is skipped
    /// together with its edge. Otherwise the edge is recorded once, however often it
    /// is met, and a new neighbour joins the nodes and the next frontier.
    ///
    /// DESIGN DECISION: readability is an injected predicate rather than a
    /// [`Governance`], so the filter is unit-tested on its own; production passes a
    /// closure over [`can_read`], the same per-object check a fabric query applies.
    /// [VLD-ANB-MIDGARD-0094] [NFR-SEC-2]
    fn admit(
        &mut self,
        readable: &dyn Fn(&ObjectInstance) -> bool,
        reg: &TypeRegistry,
        node: &ObjectInstance,
        (lt, source, target): Adjacent<'_>,
        hop: u64,
    ) {
        let other = if source.id == node.id { target } else { source };
        if !readable(other) {
            return;
        }
        let is_new = !self.seen.contains(&other.id);
        if is_new && self.nodes.len() >= self.node_limit {
            self.truncated = true;
            return;
        }
        if self
            .edge_keys
            .insert((lt.id.clone(), source.id.clone(), target.id.clone()))
        {
            self.edges.push(Json::obj(vec![
                ("from", Json::s(&source.id)),
                ("to", Json::s(&target.id)),
                ("label", Json::s(&link_label(reg, lt))),
            ]));
        }
        if is_new {
            self.seen.insert(other.id.clone());
            self.nodes.push(node_json(reg, other, hop));
            self.frontier.push(other.id.clone());
        }
    }

    /// The search-around response body.
    fn into_json(self, seed: &str, seed_objects: usize, audit_seq: u64) -> Json {
        Json::obj(vec![
            ("seed", Json::s(seed)),
            ("nodes", Json::Arr(self.nodes)),
            ("edges", Json::Arr(self.edges)),
            ("truncated", Json::Bool(self.truncated)),
            (
                "nodeLimit",
                Json::Int(i64::try_from(self.node_limit).unwrap_or(i64::MAX)),
            ),
            (
                "seedObjects",
                Json::Int(i64::try_from(seed_objects).unwrap_or(i64::MAX)),
            ),
            ("auditId", Json::s(&audit_id(audit_seq))),
        ])
    }
}

// ---- projection helpers ---------------------------------------------------------

/// One object as a policy resource, under the fabric's scheme (`object:<type>/<key>`).
/// [VLD-ANB-MIDGARD-0023]
fn resource_of(obj: &ObjectInstance) -> Resource {
    fabric::object_resource(obj)
}

/// The per-object read check, exactly as the fabric's query engine applies it.
fn can_read<L: LedgerStore>(
    gov: &Governance<L>,
    ctx: &PolicyContext,
    obj: &ObjectInstance,
) -> bool {
    fabric::can_read_object(gov, ctx, obj)
}

fn audit_id(seq: u64) -> String {
    format!("AUD-{seq}")
}

/// An approval's state as the console shows it.
fn approval_status(a: &Approval, now: u64) -> &'static str {
    match &a.state {
        _ if is_expired(a, now) => "expired",
        ApprovalState::Pending => "pending",
        ApprovalState::Approved { .. } => "approved",
        ApprovalState::Rejected { .. } => "rejected",
        ApprovalState::Used { .. } => "used",
    }
}

/// Who decided an approval, once someone has.
fn approver_of(a: &Approval) -> Option<&str> {
    match &a.state {
        ApprovalState::Pending => None,
        ApprovalState::Approved { approver, .. }
        | ApprovalState::Rejected { approver }
        | ApprovalState::Used { approver } => Some(approver),
    }
}

/// Whether an approval has lapsed: pending past [`PENDING_TTL`], or approved and not
/// redeemed in time. [VLD-ANB-MIDGARD-0039]
fn is_expired(a: &Approval, now: u64) -> bool {
    match &a.state {
        ApprovalState::Pending => now >= a.requested.saturating_add(PENDING_TTL),
        ApprovalState::Approved { expires, .. } => now >= *expires,
        ApprovalState::Rejected { .. } | ApprovalState::Used { .. } => false,
    }
}

/// Whether an approval still counts against its requester: pending, or approved and
/// redeemable, and not expired. [VLD-ANB-MIDGARD-0039]
fn is_open(a: &Approval, now: u64) -> bool {
    matches!(
        a.state,
        ApprovalState::Pending | ApprovalState::Approved { .. }
    ) && !is_expired(a, now)
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

#[cfg(test)]
mod tests {
    use super::*;
    use fabric::Governance;
    use policy::{Clearance, DefaultPdp, Role};
    use pqc_agility::OsRng;

    const NOW: u64 = 1_800_000_000;
    const PURPOSE: &str = "operations";

    /// The development enclosure, with its human-gated actions and seeded aircraft.
    fn dev_enclosure() -> Enclosure {
        let governance =
            Governance::new(DefaultPdp::new(crate::dev_login_policy()), &mut OsRng).unwrap();
        let mut enclosure = Enclosure::new(governance);
        enclosure.seed_dev(PURPOSE, NOW).unwrap();
        enclosure
    }

    fn person(subject: &str, role: &str) -> PolicyContext {
        PolicyContext {
            subject: subject.into(),
            roles: [Role(role.into())].into_iter().collect(),
            clearance: Clearance {
                level: level_of("confidential").unwrap(),
                compartments: [Compartment("OPS-N".into())].into_iter().collect(),
            },
            purpose: Purpose(PURPOSE.into()),
        }
    }

    fn approval_id(json: &Json) -> String {
        json.str_field("id").expect("approval id").to_string()
    }

    /// A target is under a human gate while its approval is pending, approved and
    /// unexpired, or redeemed, and not once it is rejected or expired. [NFR-AI-1]
    /// [VLD-ANB-MIDGARD-0001] [AC-18.4]
    #[test]
    fn gated_targets_cover_live_and_redeemed_approvals() {
        const TARGET: &str = "aircraft/AC-2214";
        let operator = person("olivia", "operator");
        let approver = person("victor", "approver");
        let mut enclosure = dev_enclosure();
        assert!(enclosure.gated_targets(NOW).is_empty());

        // Pending, approved, then redeemed: protected throughout.
        let pending = enclosure
            .request_approval(&operator, "Ground aircraft", TARGET, NOW)
            .map(|j| approval_id(&j))
            .unwrap_or_else(|e| panic!("{}", e.body.encode()));
        assert!(enclosure.gated_targets(NOW).contains(TARGET), "pending");
        enclosure
            .decide_approval(&approver, &pending, true, NOW)
            .unwrap_or_else(|e| panic!("{}", e.body.encode()));
        assert!(enclosure.gated_targets(NOW).contains(TARGET), "approved");
        let params = [("target".to_string(), TARGET.to_string())].into();
        enclosure
            .apply_action(&operator, "Ground aircraft", params, Some(&pending), NOW)
            .unwrap_or_else(|e| panic!("{}", e.body.encode()));
        assert!(
            enclosure.gated_targets(NOW + APPROVAL_TTL).contains(TARGET),
            "redeemed, whenever it was"
        );

        // Rejected, or approved and expired: not protected.
        let mut enclosure = dev_enclosure();
        let rejected = enclosure
            .request_approval(&operator, "Ground aircraft", TARGET, NOW)
            .map(|j| approval_id(&j))
            .unwrap_or_else(|e| panic!("{}", e.body.encode()));
        enclosure
            .decide_approval(&approver, &rejected, false, NOW)
            .unwrap_or_else(|e| panic!("{}", e.body.encode()));
        assert!(enclosure.gated_targets(NOW).is_empty(), "rejected");
        let expiring = enclosure
            .request_approval(&operator, "Ground aircraft", TARGET, NOW)
            .map(|j| approval_id(&j))
            .unwrap_or_else(|e| panic!("{}", e.body.encode()));
        enclosure
            .decide_approval(&approver, &expiring, true, NOW)
            .unwrap_or_else(|e| panic!("{}", e.body.encode()));
        assert!(enclosure
            .gated_targets(NOW + APPROVAL_TTL - 1)
            .contains(TARGET));
        assert!(
            enclosure.gated_targets(NOW + APPROVAL_TTL).is_empty(),
            "an approval past its time protects nothing"
        );
    }
}

/// The search-around building blocks on their own: the clearance filter with an
/// injected readability predicate, link collection order, and seed resolution.
/// [VLD-ANB-MIDGARD-0094] [AC-43.2] [AC-43.3] [DD-1] [NFR-SEC-2]
#[cfg(test)]
mod search_around_tests {
    use super::*;
    use policy::{Action, Clearance, DefaultPdp, PolicySet, Role, Rule, RuleId};
    use pqc_agility::OsRng;

    const NOW: u64 = 1_800_000_000;
    const PURPOSE: &str = "operations";

    /// One object type, `thing`, and one link type, `thing` to `thing`.
    fn registry() -> TypeRegistry {
        let mut reg = TypeRegistry::new();
        reg.register_object_type(ObjectType {
            id: "thing".into(),
            api_name: "thing".into(),
            title: "Thing".into(),
            properties: vec![],
            interfaces: vec![],
        })
        .expect("object type registers");
        reg.register_link_type(LinkType {
            id: "rel".into(),
            api_name: "rel".into(),
            source_type: "thing".into(),
            target_type: "thing".into(),
            cardinality: Cardinality::ManyToMany,
        })
        .expect("link type registers");
        reg
    }

    /// An in-memory store holding `thing/<key>` for every key and the `rel` links
    /// given as (source key, target key).
    fn store(keys: &[&str], links: &[(&str, &str)]) -> InMemoryStore {
        let mut store = InMemoryStore::new();
        for key in keys {
            store.put_object(ObjectInstance {
                id: format!("thing/{key}"),
                type_id: "thing".into(),
                properties: BTreeMap::new(),
                marking: marking("open", &[]),
                allowed_purposes: [Purpose(PURPOSE.into())].into_iter().collect(),
            });
        }
        for (source, target) in links {
            store.put_link(LinkInstance {
                link_type: "rel".into(),
                source_id: format!("thing/{source}"),
                target_id: format!("thing/{target}"),
            });
        }
        store
    }

    fn get<'a>(store: &'a InMemoryStore, key: &str) -> &'a ObjectInstance {
        store
            .get_object(&format!("thing/{key}"))
            .unwrap_or_else(|| panic!("no thing/{key}"))
    }

    fn node_ids(walk: &Walk) -> Vec<&str> {
        walk.nodes
            .iter()
            .map(|n| n.str_field("id").expect("id"))
            .collect()
    }

    fn edge_ends(walk: &Walk) -> Vec<(&str, &str)> {
        walk.edges
            .iter()
            .map(|e| {
                (
                    e.str_field("from").expect("from"),
                    e.str_field("to").expect("to"),
                )
            })
            .collect()
    }

    #[test]
    fn admit_drops_an_unreadable_neighbour_and_records_no_edge_to_it() {
        let reg = registry();
        let store = store(&["A", "B"], &[("A", "B")]);
        let lt = reg.link_type("rel").expect("rel");
        let (a, b) = (get(&store, "A"), get(&store, "B"));
        let readable = |o: &ObjectInstance| o.id != "thing/B";

        let mut walk = Walk::new(10);
        walk.seed(&reg, &store, &["thing/A".to_string()]);
        walk.admit(&readable, &reg, a, (lt, a, b), 1);

        assert_eq!(node_ids(&walk), ["thing/A"]);
        assert!(walk.edges.is_empty(), "no edge to an unreadable neighbour");
        assert!(
            !walk.truncated,
            "a hidden neighbour does not truncate the walk"
        );
        assert_eq!(walk.frontier, ["thing/A"], "nor joins the frontier");
    }

    #[test]
    fn admit_marks_truncated_at_the_node_limit_and_keeps_edges_between_seen_nodes() {
        let reg = registry();
        let store = store(
            &["A", "B", "C", "D"],
            &[("A", "B"), ("A", "C"), ("A", "D"), ("B", "A")],
        );
        let lt = reg.link_type("rel").expect("rel");
        let [a, b, c, d] = ["A", "B", "C", "D"].map(|k| get(&store, k));
        let readable = |_: &ObjectInstance| true;

        let mut walk = Walk::new(2);
        walk.seed(&reg, &store, &["thing/A".to_string()]);
        // Three readable neighbours; only one fits beside the seed.
        walk.admit(&readable, &reg, a, (lt, a, b), 1);
        walk.admit(&readable, &reg, a, (lt, a, c), 1);
        walk.admit(&readable, &reg, a, (lt, a, d), 1);
        // A link between two admitted nodes, met twice.
        walk.admit(&readable, &reg, a, (lt, b, a), 1);
        walk.admit(&readable, &reg, a, (lt, b, a), 1);

        assert_eq!(node_ids(&walk), ["thing/A", "thing/B"]);
        assert!(walk.truncated);
        assert_eq!(
            edge_ends(&walk),
            [("thing/A", "thing/B"), ("thing/B", "thing/A")],
            "the cut neighbours bring no edge, and the repeated edge is recorded once"
        );
    }

    #[test]
    fn adjacent_links_finds_outbound_then_inbound_links_in_registry_order() {
        let reg = registry();
        let store = store(&["A", "B", "C"], &[("A", "B"), ("C", "A")]);

        let found: Vec<(&str, &str, &str)> = adjacent_links(&reg, &store, get(&store, "A"))
            .into_iter()
            .map(|(lt, s, t)| (lt.id.as_str(), s.id.as_str(), t.id.as_str()))
            .collect();

        assert_eq!(
            found,
            [("rel", "thing/A", "thing/B"), ("rel", "thing/C", "thing/A")]
        );
    }

    /// The demo operator's session context.
    fn operator() -> PolicyContext {
        let user = crate::demo_operator();
        PolicyContext {
            subject: "operator".into(),
            roles: user.roles,
            clearance: Clearance {
                level: user.clearance.level,
                compartments: user.clearance.compartments,
            },
            purpose: Purpose(PURPOSE.into()),
        }
    }

    /// An enclosure over the development data, enforcing `rules`.
    fn enclosure(rules: Vec<Rule>) -> Enclosure {
        let pdp = DefaultPdp::new(PolicySet::new(rules).expect("valid policy"));
        let governance = Governance::new(pdp, &mut OsRng).expect("governance keygen");
        let mut enclosure = Enclosure::new(governance);
        enclosure.seed_dev(PURPOSE, NOW).expect("dev data seeds");
        enclosure
    }

    fn read_rule(id: &str, selector: Selector, effect: RuleEffect) -> Rule {
        Rule {
            id: RuleId(id.into()),
            role: Role("operator".into()),
            action: Action("Read".into()),
            purpose: None,
            selector,
            effect,
        }
    }

    #[test]
    fn seed_roots_reports_an_unreadable_seed_as_unknown() {
        let mut rules = crate::dev_login_policy().rules().to_vec();
        rules.push(read_rule(
            "deny-seed",
            Selector::Resource(fabric::object_resource_id("aircraft/AC-2214")),
            RuleEffect::Deny,
        ));
        let mut enclosure = enclosure(rules);
        let ctx = operator();
        let before = enclosure.audit_len();

        let err = enclosure
            .seed_roots(&ctx, "aircraft/AC-2214")
            .expect_err("an unreadable seed is refused");
        assert_eq!(err.status, 404);
        assert!(err.body.encode().contains("unknown seed object"));
        let err = enclosure
            .search_around(&ctx, "aircraft/AC-2214", 1, None, NOW)
            .expect_err("and so is a walk from it");
        assert_eq!(err.status, 404);
        assert_eq!(
            enclosure.audit_len(),
            before,
            "no audit row reveals the seed"
        );
    }

    /// The follow-up WP-19 left for WP-43: a single-seed walk is authorised as the
    /// seed is read, on its object id or else its type's id, so a policy that permits
    /// only `object-type:<type>` walks from one object of that type.
    /// [VLD-ANB-MIDGARD-0023] [AC-19.2]
    #[test]
    fn a_type_scoped_permit_lets_a_walk_start_from_one_object_of_the_type() {
        let type_only = read_rule(
            "read-aircraft",
            Selector::Resource(fabric::type_resource_id("aircraft")),
            RuleEffect::Permit,
        );
        let mut enclosure = enclosure(vec![type_only]);
        let ctx = operator();

        let graph = enclosure
            .search_around(&ctx, "aircraft/AC-2214", 2, None, NOW)
            .unwrap_or_else(|e| panic!("{}: {}", e.status, e.body.encode()));
        let nodes: Vec<&str> = match graph.get("nodes") {
            Some(Json::Arr(nodes)) => nodes
                .iter()
                .map(|n| n.str_field("id").expect("id"))
                .collect(),
            other => panic!("no nodes: {other:?}"),
        };
        assert!(nodes.contains(&"aircraft/AC-2214"), "{nodes:?}");
        assert!(
            nodes.iter().all(|id| id.starts_with("aircraft/")),
            "only objects of the permitted type are reached: {nodes:?}"
        );
        let last = enclosure.fabric().audit_entries().last().expect("a row");
        assert_eq!(
            last.record.resource.0, "object-type:aircraft",
            "the decision is recorded on the id that permitted it"
        );
    }
}
