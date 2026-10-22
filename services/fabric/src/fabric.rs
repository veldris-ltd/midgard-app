//! The Object Fabric facade (OFS): the typed query/action surface products depend
//! on. It ties the semantic registry, the indexed store, and the kinetic function
//! runtime together, and routes **all** enforcement and audit through the single
//! [`governance::Governance`] spine — the fabric never re-implements policy
//! evaluation or audit signing (KDD-1). This is the Rust SDK surface mirroring the
//! `ObjectFabric` proto service. [DD-1] [DD-6] [NFR-SEC-2] [NFR-AI-1]

use std::collections::{BTreeMap, BTreeSet};

use audit_writer::{InMemoryLedger, LedgerId, LedgerStore, SealedEntry};
use governance::{GovContext, Governance};
use policy::{ClassificationLevel, Marking, Resource, ResourceId};
use pqc_agility::VerifyingKey;

use crate::action::{Effect, FunctionRuntime};
use crate::error::FabricError;
use crate::object::ObjectInstance;
use crate::query::{run_query_counted, Query};
use crate::store::{ObjectStore, StoreOp};
use crate::types::TypeRegistry;
use crate::value::Value;

/// The result of a query: the authorised objects plus the audit sequence number.
#[derive(Debug, Clone)]
pub struct QueryResponse {
    /// Authorised, marking-filtered results.
    pub results: Vec<ObjectInstance>,
    /// How many readable objects matched, before `max_results` cut them.
    pub matched: usize,
    /// The audit entry recording the read decision.
    pub audit_seq: u64,
}

/// The result of applying an action.
#[derive(Debug, Clone)]
pub struct ApplyActionResponse {
    /// The audit entry recording the action.
    pub audit_seq: u64,
    /// Whether the mutation was applied.
    pub applied: bool,
}

/// The Object Fabric service, generic over the storage backend and over the audit
/// ledger its governance spine writes to. The ledger defaults to the in-memory one;
/// a service that must keep lineage across restarts passes a governance built over
/// a durable, KMS-sealed `FileLedger`. [AC-M-4] [RC-09]
pub struct Fabric<S: ObjectStore, L: LedgerStore = InMemoryLedger> {
    registry: TypeRegistry,
    store: S,
    runtime: FunctionRuntime,
    governance: Governance<L>,
}

impl<S: ObjectStore, L: LedgerStore> Fabric<S, L> {
    /// Assemble a fabric over a pre-built governance spine (which owns the policy
    /// set and the PQ-signed audit ledger).
    pub fn new(
        registry: TypeRegistry,
        store: S,
        runtime: FunctionRuntime,
        governance: Governance<L>,
    ) -> Self {
        Self {
            registry,
            store,
            runtime,
            governance,
        }
    }

    /// The type registry (mutable, for provisioning schema).
    pub fn registry_mut(&mut self) -> &mut TypeRegistry {
        &mut self.registry
    }
    /// The type registry.
    pub fn registry(&self) -> &TypeRegistry {
        &self.registry
    }
    /// The store (mutable, for seeding objects/links).
    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }
    /// The store.
    pub fn store(&self) -> &S {
        &self.store
    }
    /// The function runtime (mutable, for registering action functions).
    pub fn runtime_mut(&mut self) -> &mut FunctionRuntime {
        &mut self.runtime
    }
    /// The governance spine (for external audit verification).
    pub fn governance(&self) -> &Governance<L> {
        &self.governance
    }
    /// The governance spine, mutably — so a caller in front of the fabric (the HTTP
    /// edge) can authorise its own admission decisions into the **same** PQ-signed
    /// ledger, keeping one lineage rather than two. [AC-M-4]
    pub fn governance_mut(&mut self) -> &mut Governance<L> {
        &mut self.governance
    }

    /// The sealed audit entries so far.
    pub fn audit_entries(&self) -> &[SealedEntry] {
        self.governance.audit_entries()
    }
    /// The audit verifying key.
    pub fn audit_verifying_key(&self) -> &VerifyingKey {
        self.governance.audit_verifying_key()
    }
    /// The audit ledger id.
    pub fn audit_ledger_id(&self) -> &LedgerId {
        self.governance.audit_ledger_id()
    }

    /// Run a typed query. The query is authorised and audited once through
    /// governance; each returned object is additionally marking-filtered. A denied
    /// query is audited and returns [`FabricError::Denied`]. [NFR-SEC-2] [AC-M-4]
    pub fn query(
        &mut self,
        ctx: &GovContext,
        q: &Query,
        now: u64,
    ) -> Result<QueryResponse, FabricError> {
        let type_resource = Resource {
            id: ResourceId(q.object_type.clone()),
            marking: Marking {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: BTreeSet::new(),
            },
            allowed_purposes: [ctx.purpose.clone()].into_iter().collect(),
        };
        let decision = self
            .governance
            .authorize(ctx, &type_resource, "Read", now)
            .map_err(|e| FabricError::Audit(e.to_string()))?;
        if !decision.permitted {
            return Err(FabricError::Denied);
        }
        let (results, matched) =
            run_query_counted(&self.registry, &self.store, &self.governance, ctx, q)?;
        Ok(QueryResponse {
            results,
            matched,
            audit_seq: decision.audit_seq,
        })
    }

    /// Apply a validated, function-backed, governed action. Validates parameters
    /// against the typed schema, authorises + audits through governance (deny by
    /// default), runs the bound function, and applies its effects through the
    /// store (the write funnel keeps indexes current). [NFR-SEC-2] [NFR-AI-1]
    pub fn apply_action(
        &mut self,
        ctx: &GovContext,
        action_type_id: &str,
        raw_params: &BTreeMap<String, String>,
        now: u64,
    ) -> Result<ApplyActionResponse, FabricError> {
        let at = self
            .registry
            .action_type(action_type_id)
            .ok_or_else(|| FabricError::UnknownActionType(action_type_id.to_string()))?
            .clone();

        // Validate + parse parameters against the typed action schema.
        let mut params: BTreeMap<String, Value> = BTreeMap::new();
        for p in &at.parameters {
            match raw_params.get(&p.api_name) {
                Some(s) => {
                    let v =
                        Value::parse(p.value_type, s).ok_or_else(|| FabricError::BadParamType {
                            param: p.api_name.clone(),
                            expected: p.value_type,
                        })?;
                    params.insert(p.api_name.clone(), v);
                }
                None if p.required => {
                    return Err(FabricError::MissingRequiredParam(p.api_name.clone()));
                }
                None => {}
            }
        }

        // Authorise + audit through governance (role/marking/purpose, deny by
        // default). Role gating for the action is a policy rule on its api name.
        let resource = Resource {
            id: ResourceId(at.id.clone()),
            marking: at.marking.clone(),
            allowed_purposes: at.allowed_purposes.clone(),
        };
        let decision = self
            .governance
            .authorize(ctx, &resource, &at.api_name, now)
            .map_err(|e| FabricError::Audit(e.to_string()))?;
        if !decision.permitted {
            return Err(FabricError::Denied);
        }

        // Run the bound function to compute effects, then apply them.
        let effects = {
            let func = self
                .runtime
                .get(&at.function_id)
                .ok_or_else(|| FabricError::UnknownFunction(at.function_id.clone()))?;
            func.apply(&params, &self.store)?
        };
        // All of an action's effects are made durable together, or none is: the
        // decision is already in the ledger, and an error tells the caller the action
        // did not happen.
        self.store.try_apply_all(
            effects
                .into_iter()
                .map(|effect| match effect {
                    Effect::PutObject(obj) => StoreOp::PutObject(obj),
                    Effect::PutLink(link) => StoreOp::PutLink(link),
                })
                .collect(),
        )?;

        Ok(ApplyActionResponse {
            audit_seq: decision.audit_seq,
            applied: true,
        })
    }
}
