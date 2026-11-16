//! The data-source seam between a session and whatever serves it. `[FR-C-5]`
//! `[AC-C-13]` `[FR-C-4]`
//!
//! The demo enclosure (`crate::demo::enclosure`) implements it over the signed
//! dataset; a gateway client over `midgard_client_enclosure::EnclosureClient`
//! implements it later. A source applies clearance itself: above-clearance rows are
//! delivered withheld, without id, body or compartments, mirroring "marking
//! enforcement remains authoritative in the enclosure" (`clients/store`). The store's
//! own refusal (`StoreError::AboveClearance`) stays as the client-side floor.
//!
//! What a source does not hide: the names of its collections, and how many rows of
//! a collection are withheld and at what level, are visible to every persona (as in
//! the web consoles). See `docs/security/THREAT_MODEL.md`, residual risks.

use std::fmt;

use attest::json::{parse, Json};
use policy::Clearance;
use pqc_agility::sha256;

use crate::consoles::decode::{DecodeError, FromJson};
use crate::demo::audit::SignatureStatus;
use crate::marking::{MarkingLevel, RowMarking, Visible};

/// Field names of an action object (`action:<replica>-<n>`). `[FR-C-4]`
pub mod action {
    /// Object id prefix.
    pub const PREFIX: &str = "action:";
    /// `<console>.<verb>`, e.g. `admin.approve`.
    pub const VERB: &str = "verb";
    /// `<collection>/<row id>` the action is on.
    pub const TARGET: &str = "target";
    /// Canonical JSON object of string parameters.
    pub const PARAMS: &str = "params";
    /// The persona id that made it.
    pub const BY: &str = "by";
    /// Set by the source: `applied`, `refused` or `conflict`.
    pub const STATUS: &str = "status";
    /// Set by the source: why it was refused or in conflict.
    pub const REASON: &str = "reason";
    /// Set by the source: the audit record id.
    pub const AUDIT: &str = "audit";
    /// Set by the source: canonical JSON result.
    pub const RESULT: &str = "result";
    /// The fields a client writes.
    pub const CLIENT_FIELDS: [&str; 4] = [VERB, TARGET, PARAMS, BY];
    /// The fields only the source writes; a source ignores a client's write to them.
    pub const SOURCE_FIELDS: [&str; 4] = [STATUS, REASON, AUDIT, RESULT];
}

/// A demonstration identity: who signs in, at what clearance.
#[derive(Debug, Clone, PartialEq)]
pub struct Persona {
    /// Stable id, e.g. `k.rao`.
    pub id: String,
    /// Display name.
    pub display: String,
    /// `analyst`, `operator`, `approver` or `release-manager`.
    pub role: String,
    /// Clearance level.
    pub clearance: MarkingLevel,
    /// Held compartments.
    pub compartments: Vec<String>,
    /// Policy roles.
    pub roles: Vec<String>,
}

/// What a source grants a signed-in persona. Made only by a source
/// ([`DataSource::grant`]); the fields are private so a caller cannot raise its own
/// clearance, and a source still re-reads the persona's clearance on every call.
#[derive(Debug, Clone, PartialEq)]
pub struct Grant {
    persona: Persona,
    clearance: Clearance,
}

impl Grant {
    /// A grant for `persona` at `clearance` (sources only).
    pub(crate) fn new(persona: Persona, clearance: Clearance) -> Self {
        Self { persona, clearance }
    }

    /// The persona granted.
    pub fn persona(&self) -> &Persona {
        &self.persona
    }

    /// The clearance on the policy lattice.
    pub fn clearance(&self) -> &Clearance {
        &self.clearance
    }
}

/// One row as a source delivers it.
#[derive(Debug, Clone, PartialEq)]
pub enum Delivered {
    /// A row the grant dominates: canonical JSON body.
    Row {
        /// Row id within the collection.
        id: String,
        /// Its marking.
        marking: RowMarking,
        /// Canonical JSON of the row.
        body: Vec<u8>,
        /// For an audit row (`AuditRecord`), what its signature check found; `None`
        /// for every other row type. A screen shows a check mark only for
        /// [`SignatureStatus::Verified`] or [`SignatureStatus::Device`].
        ///
        /// What a source says here is a claim, not a check. The session believes
        /// [`SignatureStatus::Verified`] for a row only when
        /// [`DataSource::in_core_verified`] gives that row's [`VerifiedDigest`] and
        /// the digest matches the row exactly as delivered: only a source that
        /// checked the row inside this crate (the demo enclosure, under the pinned
        /// demo-audit anchor) can give one, and a row changed after that check no
        /// longer matches. Otherwise the row is `none`. No source may say
        /// [`SignatureStatus::Device`]: the session decides that itself.
        signature: Option<SignatureStatus>,
    },
    /// A row above the grant: its level only.
    Withheld {
        /// The level withheld.
        level: MarkingLevel,
    },
}

/// Why a source refused.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceError {
    /// The source is unreachable (or the demo is simulating that).
    Offline,
    /// No such persona.
    UnknownPersona(String),
    /// No such collection.
    UnknownCollection(String),
    /// A pushed op blob, or the data behind the source, did not decode.
    Malformed(String),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::Offline => write!(f, "offline"),
            SourceError::UnknownPersona(p) => write!(f, "unknown persona {p}"),
            SourceError::UnknownCollection(c) => write!(f, "unknown collection {c}"),
            SourceError::Malformed(d) => write!(f, "malformed: {d}"),
        }
    }
}

impl std::error::Error for SourceError {}

/// Handed by the session to [`DataSource::in_core_verified`]. Only this crate can
/// make one, so code outside it cannot ask a source (its own or one it wraps) for a
/// row's [`VerifiedDigest`]; it can only pass on a seal the session gave it.
/// `[NFR-PQ-3]` `[AC-C-13]`
#[derive(Debug)]
pub struct Seal(());

impl Seal {
    /// A seal (the session only).
    pub(crate) fn new() -> Self {
        Self(())
    }
}

/// The digest of one audit row as an in-core source verified it: SHA-256 of
/// [`signed_bytes`](crate::demo::audit::signed_bytes) over the collection, id,
/// marking and item (with `pqSignature` reading `verified`). Only this crate can make
/// one; a copy matches only the very row it was made for. `[NFR-PQ-3]` `[AC-C-13]`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedDigest([u8; 32]);

impl VerifiedDigest {
    /// The digest of `item` at `collection/id` marked `marking`.
    pub(crate) fn of(collection: &str, id: &str, marking: &RowMarking, item: &Json) -> Self {
        let item = crate::demo::audit::with_status(item, SignatureStatus::Verified);
        Self(sha256(&crate::demo::audit::signed_bytes(
            collection, id, marking, &item,
        )))
    }
}

/// A source of console data for a session.
pub trait DataSource {
    /// The personas that may sign in.
    fn personas(&self) -> Vec<Persona>;
    /// The collections served, as `(name, read-model type)`, in name order.
    fn collections(&self) -> Vec<(String, String)>;
    /// Sign `persona_id` in: the clearance and roles it is granted.
    ///
    /// Not an authentication step. The demo enclosure grants any of its personas to
    /// whoever asks (the persona picker), because the demo has no users; a gateway
    /// source must authenticate the caller (PQ-mTLS identity, platform assertion)
    /// before it grants anything, and grant only the persona that identity holds.
    fn grant(&self, persona_id: &str) -> Result<Grant, SourceError>;
    /// The rows of `collection`, clearance-filtered for `grant`.
    fn fetch(&self, grant: &Grant, collection: &str) -> Result<Vec<Delivered>, SourceError>;
    /// Push opaque op blobs; receive the source's ops to merge. Idempotent.
    fn exchange(&mut self, grant: &Grant, ops: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, SourceError>;
    /// Whether the source is reachable.
    fn online(&self) -> bool;
    /// Simulate (demo) or record (gateway) reachability.
    fn set_online(&mut self, online: bool);
    /// The [`VerifiedDigest`] of audit row `collection/id`, if this source checked
    /// it inside the core and `grant` may see it. Called only by the session, with a
    /// [`Seal`] nothing outside the crate can make. The default, and the only answer
    /// code outside the crate can give of its own, is `None`: the session then
    /// shows the row as `none`. A wrapper that passes the seal on to an in-core
    /// source gets that source's digests, which match only the rows as it verified
    /// them.
    fn in_core_verified(
        &self,
        _seal: &Seal,
        _grant: &Grant,
        _collection: &str,
        _id: &str,
    ) -> Option<VerifiedDigest> {
        None
    }
}

/// Decode delivered rows as the read model `T`, keeping each row's visibility.
pub fn decode_rows<T: FromJson>(rows: Vec<Delivered>) -> Result<Vec<Visible<T>>, DecodeError> {
    rows.into_iter()
        .map(|d| match d {
            Delivered::Withheld { level } => Ok(Visible::Withheld { level }),
            Delivered::Row {
                id, marking, body, ..
            } => {
                let path = format!("$[{id}]");
                let text = std::str::from_utf8(&body).map_err(|_| DecodeError {
                    path: path.clone(),
                    reason: "not UTF-8".into(),
                })?;
                let v = parse(text).map_err(|reason| DecodeError {
                    path: path.clone(),
                    reason,
                })?;
                Ok(Visible::Shown {
                    item: T::from_json(&v, &path)?,
                    id,
                    marking,
                })
            }
        })
        .collect()
}
