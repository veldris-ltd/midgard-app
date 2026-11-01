//! The **action-type catalogue**: which action types a deployment allows, and
//! whether each is consequential. The class of an action is a deployment-owned fact
//! looked up here, never something the proposer asserts. [FR-H-5] [NFR-AI-1]
//! [VLD-ANB-MIDGARD-0045]
//!
//! The [`WorkflowEngine`](crate::engine::WorkflowEngine) refuses to propose a type
//! the catalogue does not list, and the
//! [`DispatchActuator`](crate::actuator::DispatchActuator) re-checks the type and
//! its class before invoking an effector. An empty catalogue refuses everything
//! (fail closed).
//!
//! ## File format
//! A hosting service loads its catalogue with [`ActionTypeCatalog::parse`] from a
//! plain-text file of one `<action_type> consequential|informational` pair per
//! line. Blank lines and lines starting with `#` are ignored; a type listed twice
//! is refused. The compose sample ships `deploy/compose/heimdall-actions.catalog`
//! for the demo action types. No running service hosts the workflow engine yet, so
//! nothing loads that file today; it is the file such a service is to load.

use std::collections::BTreeMap;

use crate::error::WorkflowError;

/// Whether an action type changes real-world or system state.
///
/// Every execution needs a recorded human decision whatever the class; the class is
/// kept on the record for display, filtering and the actuator's re-check, not as a
/// way round the human gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActionClass {
    /// Mutates real-world or system state (cue a sensor, dispatch an asset).
    Consequential,
    /// Records or annotates without acting on the world (annotate a track).
    Informational,
}

impl ActionClass {
    /// The catalogue-file keyword for this class.
    pub fn label(self) -> &'static str {
        match self {
            ActionClass::Consequential => "consequential",
            ActionClass::Informational => "informational",
        }
    }
}

impl std::fmt::Display for ActionClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// The deployment's mapping of action type to [`ActionClass`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionTypeCatalog(BTreeMap<String, ActionClass>);

impl ActionTypeCatalog {
    /// An empty catalogue, which refuses every action type.
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// List `action_type` with `class` (builder). A later call for the same type
    /// replaces the earlier one; [`ActionTypeCatalog::parse`] refuses duplicates.
    pub fn with(mut self, action_type: impl Into<String>, class: ActionClass) -> Self {
        self.0.insert(action_type.into(), class);
        self
    }

    /// The class of `action_type`, or `None` if the deployment does not allow it.
    pub fn class_of(&self, action_type: &str) -> Option<ActionClass> {
        self.0.get(action_type).copied()
    }

    /// Whether the catalogue lists no action types (and so refuses everything).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The listed action types and their classes, in type order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, ActionClass)> {
        self.0.iter().map(|(t, c)| (t.as_str(), *c))
    }

    /// Parse a catalogue file: one `<action_type> consequential|informational` per
    /// line, blank lines and `#` comments ignored. A malformed line, an unknown
    /// class or a duplicated type fails with [`WorkflowError::Catalogue`] naming the
    /// line.
    pub fn parse(text: &str) -> Result<Self, WorkflowError> {
        let mut out = BTreeMap::new();
        for (n, line) in text.lines().enumerate() {
            let lineno = n + 1;
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut words = line.split_whitespace();
            let (Some(action_type), Some(class), None) = (words.next(), words.next(), words.next())
            else {
                return Err(WorkflowError::Catalogue(format!(
                    "line {lineno}: expected '<action_type> consequential|informational'"
                )));
            };
            let class = match class {
                "consequential" => ActionClass::Consequential,
                "informational" => ActionClass::Informational,
                other => {
                    return Err(WorkflowError::Catalogue(format!(
                        "line {lineno}: unknown class '{other}' (expected consequential or informational)"
                    )))
                }
            };
            if out.insert(action_type.to_string(), class).is_some() {
                return Err(WorkflowError::Catalogue(format!(
                    "line {lineno}: action type '{action_type}' is listed twice"
                )));
            }
        }
        Ok(Self(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // [AC-64.1] [VLD-ANB-MIDGARD-0045]
    #[test]
    fn parse_reads_the_line_format() {
        let c = ActionTypeCatalog::parse(
            "# demo action types\n\ncue-sensor consequential\n  dispatch-asset   consequential  \nannotate-track informational\n",
        )
        .unwrap();
        assert_eq!(c.class_of("cue-sensor"), Some(ActionClass::Consequential));
        assert_eq!(
            c.class_of("dispatch-asset"),
            Some(ActionClass::Consequential)
        );
        assert_eq!(
            c.class_of("annotate-track"),
            Some(ActionClass::Informational)
        );
        assert_eq!(c.class_of("self-destruct"), None);
        assert_eq!(c.entries().count(), 3);
    }

    // [AC-64.1] [VLD-ANB-MIDGARD-0045]
    #[test]
    fn parse_refuses_duplicates() {
        let err = ActionTypeCatalog::parse("cue-sensor consequential\ncue-sensor informational\n")
            .unwrap_err();
        assert!(
            matches!(&err, WorkflowError::Catalogue(m) if m.contains("line 2") && m.contains("twice")),
            "{err:?}"
        );
    }

    // [AC-64.1] [VLD-ANB-MIDGARD-0045]
    #[test]
    fn parse_refuses_malformed_lines_and_unknown_classes() {
        for bad in [
            "cue-sensor\n",
            "cue-sensor consequential extra\n",
            "cue-sensor maybe\n",
        ] {
            assert!(
                matches!(
                    ActionTypeCatalog::parse(bad),
                    Err(WorkflowError::Catalogue(_))
                ),
                "accepted {bad:?}"
            );
        }
    }

    // [AC-64.2] [VLD-ANB-MIDGARD-0045]
    #[test]
    fn an_empty_catalogue_lists_nothing() {
        let c = ActionTypeCatalog::parse("# nothing here\n\n").unwrap();
        assert!(c.is_empty());
        assert_eq!(c.class_of("cue-sensor"), None);
        assert!(ActionTypeCatalog::new().is_empty());
    }

    // The sample the compose stack ships parses and lists the demo action types.
    // [AC-64.1] [VLD-ANB-MIDGARD-0045]
    #[test]
    fn the_shipped_compose_catalogue_parses() {
        let c = ActionTypeCatalog::parse(include_str!(
            "../../../deploy/compose/heimdall-actions.catalog"
        ))
        .unwrap();
        let entries: Vec<(&str, ActionClass)> = c.entries().collect();
        assert_eq!(
            entries,
            vec![
                ("annotate-track", ActionClass::Informational),
                ("cue-sensor", ActionClass::Consequential),
                ("dispatch-asset", ActionClass::Consequential),
            ]
        );
    }
}
