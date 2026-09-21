//! The **kinetic** layer: functions and the effects they produce. An
//! [`ActionFunction`] is pure business logic — given typed parameters and a
//! read-only view of the store, it returns the [`Effect`]s to apply. It never
//! mutates the store directly; the executor ([`crate::fabric::Fabric`]) validates,
//! authorises, applies, and audits. This keeps effects inspectable (the basis for
//! preview-before-commit and reversibility). [DD-1] [NFR-AI-1]

use std::collections::BTreeMap;

use crate::error::FabricError;
use crate::object::{LinkInstance, ObjectInstance};
use crate::store::ObjectStore;
use crate::value::Value;

/// A state change an action wishes to make.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Insert or replace an object.
    PutObject(ObjectInstance),
    /// Insert a link.
    PutLink(LinkInstance),
}

/// Business logic bound to an action type. Pure w.r.t. the store: it may *read*
/// to compute effects but returns them rather than applying them.
///
/// This is also the seam for Mimir-backed functions (an LLM/rule call): such a
/// function would consult Mimir and emit effects, still gated and audited by the
/// executor. `Send + Sync` so the runtime can be shared across gateway threads.
pub trait ActionFunction: Send + Sync {
    /// Compute the effects for `params` against the current `store`.
    fn apply(
        &self,
        params: &BTreeMap<String, Value>,
        store: &dyn ObjectStore,
    ) -> Result<Vec<Effect>, FabricError>;
}

/// A registry of action functions keyed by `function_id`.
#[derive(Default)]
pub struct FunctionRuntime {
    functions: BTreeMap<String, Box<dyn ActionFunction>>,
}

impl FunctionRuntime {
    /// An empty runtime.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a function under an id (as referenced by `ActionType.function_id`).
    pub fn register(&mut self, id: &str, f: Box<dyn ActionFunction>) {
        self.functions.insert(id.to_string(), f);
    }

    /// Look up a function.
    pub fn get(&self, id: &str) -> Option<&dyn ActionFunction> {
        self.functions.get(id).map(|b| b.as_ref())
    }
}
