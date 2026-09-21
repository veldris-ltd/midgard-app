//! The type registry (OMS) — the **semantic** schema. It defines object types,
//! link types, action types, interfaces, and shared property/value types, and
//! validates them on registration so the fabric can never hold an inconsistent
//! ontology (the mitigation for "ontology sprawl"). [DD-1]

use std::collections::{BTreeMap, BTreeSet};

use policy::{Marking, Purpose};

use crate::error::FabricError;
use crate::value::ValueType;

/// A typed property declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyType {
    /// Stable api name (the property key).
    pub api_name: String,
    /// The declared value type.
    pub value_type: ValueType,
    /// Whether the property must be present on every instance.
    pub required: bool,
}

impl PropertyType {
    /// Convenience constructor.
    pub fn new(api_name: &str, value_type: ValueType, required: bool) -> Self {
        Self {
            api_name: api_name.to_string(),
            value_type,
            required,
        }
    }
}

/// Link cardinality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    /// One source ↔ one target.
    OneToOne,
    /// One source → many targets.
    OneToMany,
    /// Many ↔ many.
    ManyToMany,
}

/// A reusable set of properties an object type can claim to implement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    /// Interface api name.
    pub api_name: String,
    /// Properties the interface requires.
    pub properties: Vec<PropertyType>,
}

/// The schema of a real-world entity/event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectType {
    /// Stable id.
    pub id: String,
    /// Api name.
    pub api_name: String,
    /// Human title.
    pub title: String,
    /// Declared properties.
    pub properties: Vec<PropertyType>,
    /// Interfaces this type implements (by api name).
    pub interfaces: Vec<String>,
}

impl ObjectType {
    /// Look up a property declaration by api name.
    pub fn property(&self, api_name: &str) -> Option<&PropertyType> {
        self.properties.iter().find(|p| p.api_name == api_name)
    }
}

/// The schema of a typed relationship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkType {
    /// Stable id.
    pub id: String,
    /// Api name.
    pub api_name: String,
    /// Source object-type id.
    pub source_type: String,
    /// Target object-type id.
    pub target_type: String,
    /// Cardinality.
    pub cardinality: Cardinality,
}

/// The schema of a permitted mutation — the **kinetic** counterpart of an object
/// type. An action is function-backed, governed (role/marking/purpose), and
/// audited when applied. [NFR-SEC-2] [NFR-AI-1]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionType {
    /// Stable id.
    pub id: String,
    /// Api name (also the policy action name).
    pub api_name: String,
    /// Typed parameters.
    pub parameters: Vec<PropertyType>,
    /// The function that computes the action's effects.
    pub function_id: String,
    /// Minimum marking required to invoke (mandatory access control).
    pub marking: Marking,
    /// Purposes under which the action may be invoked. Role gating is expressed
    /// as policy rules on the action's api name (evaluated by the governance PDP).
    pub allowed_purposes: BTreeSet<Purpose>,
}

/// The registry of all types. Registration is validated and fails closed.
#[derive(Debug, Default)]
pub struct TypeRegistry {
    interfaces: BTreeMap<String, Interface>,
    object_types: BTreeMap<String, ObjectType>,
    link_types: BTreeMap<String, LinkType>,
    action_types: BTreeMap<String, ActionType>,
}

fn no_duplicate_props(props: &[PropertyType]) -> Result<(), FabricError> {
    let mut seen = BTreeSet::new();
    for p in props {
        if !seen.insert(&p.api_name) {
            return Err(FabricError::DuplicateProperty(p.api_name.clone()));
        }
    }
    Ok(())
}

impl TypeRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an interface.
    pub fn register_interface(&mut self, iface: Interface) -> Result<(), FabricError> {
        no_duplicate_props(&iface.properties)?;
        if self.interfaces.contains_key(&iface.api_name) {
            return Err(FabricError::DuplicateType(iface.api_name));
        }
        self.interfaces.insert(iface.api_name.clone(), iface);
        Ok(())
    }

    /// Register an object type. Validates unique id, unique properties, and that
    /// every claimed interface exists and its properties are present with the
    /// same value type.
    pub fn register_object_type(&mut self, ot: ObjectType) -> Result<(), FabricError> {
        if self.object_types.contains_key(&ot.id) {
            return Err(FabricError::DuplicateType(ot.id));
        }
        no_duplicate_props(&ot.properties)?;
        for iface_name in &ot.interfaces {
            let iface = self
                .interfaces
                .get(iface_name)
                .ok_or_else(|| FabricError::UnknownInterface(iface_name.clone()))?;
            for req in &iface.properties {
                let ok = ot
                    .property(&req.api_name)
                    .is_some_and(|p| p.value_type == req.value_type);
                if !ok {
                    return Err(FabricError::MissingInterfaceProperty {
                        object_type: ot.id.clone(),
                        interface: iface_name.clone(),
                        property: req.api_name.clone(),
                    });
                }
            }
        }
        self.object_types.insert(ot.id.clone(), ot);
        Ok(())
    }

    /// Register a link type. Both endpoints must be existing object types.
    pub fn register_link_type(&mut self, lt: LinkType) -> Result<(), FabricError> {
        if self.link_types.contains_key(&lt.id) {
            return Err(FabricError::DuplicateType(lt.id));
        }
        if !self.object_types.contains_key(&lt.source_type) {
            return Err(FabricError::InvalidLinkEndpoints(lt.source_type));
        }
        if !self.object_types.contains_key(&lt.target_type) {
            return Err(FabricError::InvalidLinkEndpoints(lt.target_type));
        }
        self.link_types.insert(lt.id.clone(), lt);
        Ok(())
    }

    /// Register an action type. Validates unique id and unique parameter names.
    pub fn register_action_type(&mut self, at: ActionType) -> Result<(), FabricError> {
        if self.action_types.contains_key(&at.id) {
            return Err(FabricError::DuplicateType(at.id));
        }
        no_duplicate_props(&at.parameters)?;
        self.action_types.insert(at.id.clone(), at);
        Ok(())
    }

    /// Look up an object type by id.
    pub fn object_type(&self, id: &str) -> Option<&ObjectType> {
        self.object_types.get(id)
    }
    /// Iterate all registered object types in deterministic (id) order. The OAG
    /// retriever reads this to build the ontology summary it grounds against.
    pub fn object_types(&self) -> impl Iterator<Item = &ObjectType> {
        self.object_types.values()
    }
    /// Look up a link type by id.
    pub fn link_type(&self, id: &str) -> Option<&LinkType> {
        self.link_types.get(id)
    }
    /// Iterate all registered link types in deterministic (id) order.
    pub fn link_types(&self) -> impl Iterator<Item = &LinkType> {
        self.link_types.values()
    }
    /// Look up an action type by id.
    pub fn action_type(&self, id: &str) -> Option<&ActionType> {
        self.action_types.get(id)
    }
    /// Look up an interface by api name.
    pub fn interface(&self, api_name: &str) -> Option<&Interface> {
        self.interfaces.get(api_name)
    }
}
