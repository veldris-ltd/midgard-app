//! The no-source mode: what a session runs on when the app ships with no data.
//! `[FR-C-5]` `[AC-C-13]` `[FR-C-4]`
//!
//! Owner decision (19 Nov 2026): the apps ship with no dataset and no feed. With no
//! source the core signs in one built-in persona, [`LOCAL_PERSONA`] ("Local user",
//! role `operator`, clearance `open`, no compartments, no policy roles): the lowest
//! grant there is, so this mode can never show more than the data allows. Every
//! collection in [`LOCAL_COLLECTIONS`] exists and is empty; any action is refused with
//! [`SourceError::NoSource`] ("no data source connected") and nothing is kept. The
//! offline flag still toggles, so the offline chip works.
//!
//! Nothing here reads a file or a key: there is nothing to verify and nothing to trust.

use crate::marking::{clearance, MarkingLevel};
use crate::source::{DataSource, Delivered, Grant, Persona, SourceError};

/// The id of the one persona this mode signs in.
pub const LOCAL_PERSONA: &str = "local";

/// The collections this mode lists, as `(name, read-model type)` in name order.
/// Empty until the typed collection table is generated (Phase C, Task 1); every
/// collection listed here is served empty. `[FR-C-5]`
pub const LOCAL_COLLECTIONS: &[(&str, &str)] = &[];

/// The source with no data behind it.
#[derive(Debug, Clone)]
pub struct LocalSource {
    online: bool,
}

impl Default for LocalSource {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalSource {
    /// A fresh source, online.
    pub fn new() -> Self {
        Self { online: true }
    }

    fn persona() -> Persona {
        Persona {
            id: LOCAL_PERSONA.into(),
            display: "Local user".into(),
            role: "operator".into(),
            clearance: MarkingLevel::Open,
            compartments: Vec::new(),
            roles: Vec::new(),
        }
    }
}

impl DataSource for LocalSource {
    fn personas(&self) -> Vec<Persona> {
        vec![Self::persona()]
    }

    fn collections(&self) -> Vec<(String, String)> {
        LOCAL_COLLECTIONS
            .iter()
            .map(|(n, t)| (n.to_string(), t.to_string()))
            .collect()
    }

    fn grant(&self, persona_id: &str) -> Result<Grant, SourceError> {
        if persona_id != LOCAL_PERSONA {
            return Err(SourceError::UnknownPersona(persona_id.into()));
        }
        let p = Self::persona();
        let c = clearance(p.clearance, p.compartments.iter().cloned());
        Ok(Grant::new(p, c))
    }

    fn fetch(&self, grant: &Grant, collection: &str) -> Result<Vec<Delivered>, SourceError> {
        if !self.online {
            return Err(SourceError::Offline);
        }
        // As every source does: a grant is honoured only for a persona this source
        // knows.
        self.grant(&grant.persona().id)?;
        if LOCAL_COLLECTIONS.iter().any(|(n, _)| *n == collection) {
            Ok(Vec::new())
        } else {
            Err(SourceError::UnknownCollection(collection.into()))
        }
    }

    fn exchange(&mut self, grant: &Grant, ops: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, SourceError> {
        if !self.online {
            return Err(SourceError::Offline);
        }
        self.grant(&grant.persona().id)?;
        // Nothing is decoded or kept: there is nothing to apply an action to.
        if ops.is_empty() {
            Ok(Vec::new())
        } else {
            Err(SourceError::NoSource)
        }
    }

    fn online(&self) -> bool {
        self.online
    }

    fn set_online(&mut self, online: bool) {
        self.online = online;
    }
}
