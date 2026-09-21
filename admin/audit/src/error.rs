//! Admin-audit errors. Fail closed: if an event cannot be encoded or sealed, the
//! action is not recorded and the caller must treat it as un-auditable (and so
//! must not proceed).

/// A failure recording an admin audit event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminAuditError {
    /// The `who` (actor) field was empty.
    MissingWho,
    /// The `what` (action) field was empty.
    MissingWhat,
    /// A field contained the reserved unit-separator byte used by the encoding, or
    /// an approver was `Some("")`.
    InvalidField {
        /// Which field was invalid.
        field: &'static str,
    },
    /// The underlying audit ledger rejected the entry.
    Ledger(String),
}

impl std::fmt::Display for AdminAuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdminAuditError::MissingWho => write!(f, "audit event missing 'who' (actor)"),
            AdminAuditError::MissingWhat => write!(f, "audit event missing 'what' (action)"),
            AdminAuditError::InvalidField { field } => {
                write!(f, "audit event field '{field}' is invalid")
            }
            AdminAuditError::Ledger(e) => write!(f, "audit ledger: {e}"),
        }
    }
}

impl std::error::Error for AdminAuditError {}
