//! Fabric errors — every variant is a fail-closed refusal. [DD-1]

use crate::value::ValueType;

/// Errors from schema registration, query, and action execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FabricError {
    /// No such object type.
    UnknownObjectType(String),
    /// No such link type.
    UnknownLinkType(String),
    /// No such action type.
    UnknownActionType(String),
    /// No such interface.
    UnknownInterface(String),
    /// No function is registered for an action's `function_id`.
    UnknownFunction(String),
    /// A type/link/action id was registered twice.
    DuplicateType(String),
    /// A link type references a non-existent source/target object type.
    InvalidLinkEndpoints(String),
    /// An object type declares an interface but omits one of its properties.
    MissingInterfaceProperty {
        /// The object type.
        object_type: String,
        /// The interface it claims.
        interface: String,
        /// The property it is missing.
        property: String,
    },
    /// An object type or action declares the same property name twice.
    DuplicateProperty(String),
    /// A stored object value does not match the declared property type.
    PropertyTypeMismatch {
        /// The property name.
        property: String,
        /// The declared type.
        expected: ValueType,
    },
    /// A required action parameter was absent.
    MissingRequiredParam(String),
    /// An action parameter did not parse as its declared type.
    BadParamType {
        /// The parameter name.
        param: String,
        /// The declared type.
        expected: ValueType,
    },
    /// The policy point denied the query or action (deny-by-default).
    Denied,
    /// Writing audit lineage failed.
    Audit(String),
    /// The durable object store failed to persist or recover (I/O, seal/unseal,
    /// or a malformed/tampered on-disk record). Fail closed. [RC-10]
    Store(String),
}

impl std::fmt::Display for FabricError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use FabricError::*;
        match self {
            UnknownObjectType(s) => write!(f, "unknown object type: {s}"),
            UnknownLinkType(s) => write!(f, "unknown link type: {s}"),
            UnknownActionType(s) => write!(f, "unknown action type: {s}"),
            UnknownInterface(s) => write!(f, "unknown interface: {s}"),
            UnknownFunction(s) => write!(f, "unknown function: {s}"),
            DuplicateType(s) => write!(f, "duplicate type id: {s}"),
            InvalidLinkEndpoints(s) => write!(f, "link references unknown endpoint type: {s}"),
            MissingInterfaceProperty {
                object_type,
                interface,
                property,
            } => write!(
                f,
                "object type {object_type} omits interface {interface} property {property}"
            ),
            DuplicateProperty(s) => write!(f, "duplicate property: {s}"),
            PropertyTypeMismatch { property, expected } => {
                write!(f, "property {property} is not {}", expected.as_str())
            }
            MissingRequiredParam(s) => write!(f, "missing required parameter: {s}"),
            BadParamType { param, expected } => {
                write!(f, "parameter {param} is not {}", expected.as_str())
            }
            Denied => write!(f, "denied by policy"),
            Audit(s) => write!(f, "audit error: {s}"),
            Store(s) => write!(f, "object store error: {s}"),
        }
    }
}

impl std::error::Error for FabricError {}
