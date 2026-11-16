//! The demo enclosure: the signed dataset behind [`DataSource`]. `[AC-C-13]` `[FR-C-4]`
//! `[FR-C-5]` `[NFR-PQ-3]`
//!
//! It starts only from a dataset that verifies under the pinned anchor
//! ([`DemoEnclosure::open_pinned`]); any finding refuses it, and there is no fallback
//! data. Loading is strict and fails closed: every row marking is read with
//! [`MarkingLevel::parse`] (a row without one is refused, never read as `open`), every
//! row must decode as its collection's read model, and the local-suite data guard runs
//! over the whole body. State then lives in memory, starting from the dataset at each
//! launch; actions mutate it through registered handlers, each run once per action
//! object, each audited.

use std::collections::{BTreeMap, BTreeSet};

use attest::json::{parse, Json};
use attest::TrustAnchors;

use crate::consoles::check_item;
use crate::demo::audit::{self, SignatureStatus};
use crate::demo::{canon, dataset, guard};
use crate::marking::{clearance, may_see, MarkingLevel, RowMarking};
use crate::source::{action, DataSource, Delivered, Grant, Persona, SourceError};
use crate::sync::{Replica, SyncOp};
use crate::CORE_VERSION;

/// The demo-dataset anchor compiled into the core (`clients/demo/trust/demo-dataset.pub`),
/// so an app cannot be pointed at another key.
pub const PINNED_DATASET_ANCHOR: &str = include_str!("../../../demo/trust/demo-dataset.pub");

/// The dataset `format` this enclosure reads.
pub const FORMAT: &str = "midgard-demo-dataset";

/// A stored row.
#[derive(Debug, Clone)]
pub struct StoredRow {
    /// Row id.
    pub id: String,
    /// Marking.
    pub marking: RowMarking,
    /// The row as JSON.
    pub item: Json,
    /// For an audit row, its signature status; `None` for other types.
    pub signature: Option<SignatureStatus>,
}

/// A named collection of rows of one read-model type.
#[derive(Debug, Clone)]
pub struct Collection {
    /// The Rust read-model name, e.g. `AdminApproval`.
    pub type_name: String,
    /// The rows, in dataset order.
    pub rows: Vec<StoredRow>,
}

/// The mutable demo state handlers work on.
#[derive(Debug, Default)]
pub struct DemoState {
    /// Collections by name.
    pub collections: BTreeMap<String, Collection>,
    audit_seq: u64,
}

impl DemoState {
    /// The marking of `target` (`<collection>/<row id>`), if it names a row.
    pub fn marking_of(&self, target: &str) -> Option<&RowMarking> {
        let (coll, id) = target.split_once('/')?;
        self.collections
            .get(coll)?
            .rows
            .iter()
            .find(|r| r.id == id)
            .map(|r| &r.marking)
    }

    /// Append an audit row to `<console>.audit` (or `midgard.audit` when the console
    /// keeps no `AuditRecord` collection) and return its id. The row is marked at
    /// least `restricted`, and at least as high as the row it is about, so it never
    /// shows a reader the id of a row that reader may not see. It is unsigned: the
    /// device key signs run-time audit rows (Tasks A9/A10).
    pub fn audit(&mut self, console: &str, actor: &str, what: &str, resource: &str) -> String {
        self.audit_seq += 1;
        let id = format!("AUD-D{:04}", self.audit_seq);
        let secs = 15 * 3600 + self.audit_seq;
        let ts = format!(
            "{:02}:{:02}:{:02}",
            secs / 3600 % 24,
            secs / 60 % 60,
            secs % 60
        );
        let marking = match self.marking_of(resource) {
            Some(m) => RowMarking::new(
                m.level().max(MarkingLevel::Restricted),
                m.compartments().to_vec(),
            ),
            None => RowMarking::new(MarkingLevel::Restricted, Vec::<String>::new()),
        };
        let name = format!("{console}.audit");
        let key = match self.collections.get(&name) {
            Some(c) if c.type_name == audit::AUDIT_TYPE => name,
            _ => "midgard.audit".to_string(),
        };
        let status = SignatureStatus::Unsigned;
        let item = Json::Obj(vec![
            ("id".into(), Json::Str(id.clone())),
            ("actor".into(), Json::Str(actor.into())),
            ("action".into(), Json::Str(what.into())),
            ("resource".into(), Json::Str(resource.into())),
            ("ts".into(), Json::Str(ts)),
            ("pqSignature".into(), Json::Str(status.as_str().into())),
        ]);
        let coll = self.collections.entry(key).or_insert_with(|| Collection {
            type_name: audit::AUDIT_TYPE.into(),
            rows: Vec::new(),
        });
        coll.rows.insert(
            0,
            StoredRow {
                id: id.clone(),
                marking,
                item,
                signature: Some(status),
            },
        );
        id
    }
}

/// A parsed action.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionRequest {
    /// The action object id.
    pub id: String,
    /// `<console>.<verb>`.
    pub verb: String,
    /// `<collection>/<row id>`.
    pub target: String,
    /// String parameters.
    pub params: Vec<(String, String)>,
    /// The persona that made it.
    pub by: String,
}

/// What a handler decided.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Done; the result is returned to the client.
    Applied(Json),
    /// Refused, with a reason the screen shows.
    Refused(String),
    /// The target changed under the action (decided twice, stale version).
    Conflict(String),
}

/// An action handler.
pub type Handler = fn(&mut DemoState, &Grant, &ActionRequest) -> Outcome;

/// The demo enclosure.
pub struct DemoEnclosure {
    release: String,
    personas: Vec<Persona>,
    state: DemoState,
    replica: Replica,
    online: bool,
    handlers: BTreeMap<&'static str, Handler>,
}

fn bad(what: impl std::fmt::Display) -> SourceError {
    SourceError::Malformed(format!("dataset body: {what}"))
}

/// The pairs of `v`, refusing any key outside `known` and any of `required` missing.
fn strict<'a>(
    v: &'a Json,
    what: &str,
    required: &[&str],
    optional: &[&str],
) -> Result<&'a [(String, Json)], SourceError> {
    let Json::Obj(pairs) = v else {
        return Err(bad(format!("{what} is not an object")));
    };
    if let Some((k, _)) = pairs
        .iter()
        .find(|(k, _)| !required.contains(&k.as_str()) && !optional.contains(&k.as_str()))
    {
        return Err(bad(format!("{what}: unknown key {k:?}")));
    }
    if let Some(k) = required
        .iter()
        .find(|k| !pairs.iter().any(|(p, _)| p == *k))
    {
        return Err(bad(format!("{what}: missing {k:?}")));
    }
    Ok(pairs)
}

fn str_of(v: &Json, key: &str, what: &str) -> Result<String, SourceError> {
    v.get(key)
        .and_then(Json::as_str)
        .map(str::to_string)
        .ok_or_else(|| bad(format!("{what}.{key} is not a string")))
}

fn strings_of(v: &Json, key: &str, what: &str) -> Result<Vec<String>, SourceError> {
    v.get(key)
        .and_then(Json::as_array)
        .ok_or_else(|| bad(format!("{what}.{key} is not an array")))?
        .iter()
        .map(|x| {
            x.as_str()
                .map(str::to_string)
                .ok_or_else(|| bad(format!("{what}.{key} holds a non-string")))
        })
        .collect()
}

fn level_of(v: &Json, key: &str, what: &str) -> Result<MarkingLevel, SourceError> {
    let word = str_of(v, key, what)?;
    MarkingLevel::parse(&word).ok_or_else(|| bad(format!("{what}.{key}: unknown level {word:?}")))
}

/// A row marking `{"level": <word>, "compartments": [<string>…]}`, nothing else.
fn marking_of(v: &Json, what: &str) -> Result<RowMarking, SourceError> {
    strict(v, what, &["level", "compartments"], &[])?;
    Ok(RowMarking::new(
        level_of(v, "level", what)?,
        strings_of(v, "compartments", what)?,
    ))
}

fn persona_of(p: &Json, i: usize) -> Result<Persona, SourceError> {
    let what = format!("personas[{i}]");
    strict(
        p,
        &what,
        &[
            "id",
            "display",
            "role",
            "clearance",
            "compartments",
            "roles",
        ],
        &[],
    )?;
    Ok(Persona {
        id: str_of(p, "id", &what)?,
        display: str_of(p, "display", &what)?,
        role: str_of(p, "role", &what)?,
        clearance: level_of(p, "clearance", &what)?,
        compartments: strings_of(p, "compartments", &what)?,
        roles: strings_of(p, "roles", &what)?,
    })
}

fn collection_of(
    name: &str,
    c: &Json,
    audit_anchors: &TrustAnchors,
) -> Result<Collection, SourceError> {
    strict(c, name, &["type", "rows"], &[])?;
    let type_name = str_of(c, "type", name)?;
    let is_audit = type_name == audit::AUDIT_TYPE;
    let mut seen = BTreeSet::new();
    let rows = c
        .get("rows")
        .and_then(Json::as_array)
        .ok_or_else(|| bad(format!("{name}.rows is not an array")))?
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let what = format!("{name}.rows[{i}]");
            let optional: &[&str] = if is_audit {
                &[audit::ROW_SIG_FIELD]
            } else {
                &[]
            };
            strict(r, &what, &["id", "marking", "item"], optional)?;
            let id = str_of(r, "id", &what)?;
            if !seen.insert(id.clone()) {
                return Err(bad(format!("{name}: row id {id:?} twice")));
            }
            let marking = marking_of(r.get("marking").expect("required"), &what)?;
            let item = r.get("item").expect("required").clone();
            check_item(&type_name, &item, &format!("{what}.item")).map_err(bad)?;
            let signature = match (is_audit, r.get(audit::ROW_SIG_FIELD)) {
                (false, _) => None,
                (true, None) => Some(SignatureStatus::Unsigned),
                (true, Some(sig)) => {
                    let sig = sig.as_str().ok_or_else(|| {
                        bad(format!("{what}.{} is not a string", audit::ROW_SIG_FIELD))
                    })?;
                    Some(
                        audit::verify_row(name, &id, &marking, &item, sig, audit_anchors)
                            .map_err(|e| bad(format!("{what}: {e}")))?,
                    )
                }
            };
            Ok(StoredRow {
                id,
                marking,
                item,
                signature,
            })
        })
        .collect::<Result<Vec<_>, SourceError>>()?;
    Ok(Collection { type_name, rows })
}

impl DemoEnclosure {
    /// Verify `file` under the anchor compiled into the core
    /// ([`PINNED_DATASET_ANCHOR`]) for this core's release, then load it. Every
    /// finding is an `Err`, and the caller has nothing to fall back on.
    pub fn open_pinned(file: &[u8]) -> Result<Self, dataset::DatasetError> {
        Self::open_pinned_for(file, CORE_VERSION)
    }

    /// [`DemoEnclosure::open_pinned`] for `release`.
    pub fn open_pinned_for(file: &[u8], release: &str) -> Result<Self, dataset::DatasetError> {
        let anchors = TrustAnchors::parse(PINNED_DATASET_ANCHOR)
            .map_err(|e| dataset::DatasetError::Malformed(format!("pinned anchor: {e}")))?;
        // No demo-audit key exists yet, so no audit anchor is pinned: a dataset whose
        // rows carry audit signatures is refused until one is.
        Self::open(file, &anchors, &TrustAnchors::new(), release)
    }

    /// Verify `file` against the dataset `anchors` for `release`, then load it, with
    /// audit-row signatures checked under `audit_anchors`.
    pub fn open(
        file: &[u8],
        anchors: &TrustAnchors,
        audit_anchors: &TrustAnchors,
        release: &str,
    ) -> Result<Self, dataset::DatasetError> {
        let ds = dataset::verify(file, anchors, release)?;
        Self::from_body(&ds.release, &ds.body, audit_anchors)
            .map_err(|e| dataset::DatasetError::Body(e.to_string()))
    }

    /// Load a body. Production code reaches this only through [`DemoEnclosure::open`],
    /// after the signature check; tests use it with an unsigned body. The body is
    /// still checked in full: format, version, release, the local-suite data guard,
    /// personas, markings, row types and audit-row signatures.
    pub fn from_body(
        release: &str,
        body: &Json,
        audit_anchors: &TrustAnchors,
    ) -> Result<Self, SourceError> {
        strict(
            body,
            "body",
            &["collections", "personas", "format", "version", "release"],
            &["inputs"],
        )?;
        if body.get("format").and_then(Json::as_str) != Some(FORMAT) {
            return Err(bad("not a demo dataset"));
        }
        if body.get("version") != Some(&Json::Num("1".into())) {
            return Err(bad("unsupported version"));
        }
        if body.get("release").and_then(Json::as_str) != Some(release) {
            return Err(bad(format!("not for release {release}")));
        }
        if let Err(found) = guard::check(body) {
            return Err(bad(format!(
                "local-suite data guard: {} finding(s)",
                found.len()
            )));
        }
        let personas = body
            .get("personas")
            .and_then(Json::as_array)
            .ok_or_else(|| bad("personas is not an array"))?
            .iter()
            .enumerate()
            .map(|(i, p)| persona_of(p, i))
            .collect::<Result<Vec<_>, _>>()?;
        let mut ids = BTreeSet::new();
        if let Some(p) = personas.iter().find(|p| !ids.insert(p.id.as_str())) {
            return Err(bad(format!("persona {:?} twice", p.id)));
        }
        let mut state = DemoState::default();
        let Some(Json::Obj(colls)) = body.get("collections") else {
            return Err(bad("collections is not an object"));
        };
        for (name, c) in colls {
            state
                .collections
                .insert(name.clone(), collection_of(name, c, audit_anchors)?);
        }
        let mut e = Self {
            release: release.to_string(),
            personas,
            state,
            replica: Replica::new("enclosure"),
            online: true,
            handlers: BTreeMap::new(),
        };
        e.register("demo.ping", |_, _, req| {
            Outcome::Applied(Json::Obj(vec![(
                "pong".into(),
                Json::Str(req.target.clone()),
            )]))
        });
        Ok(e)
    }

    /// Register (or replace) the handler for `verb`.
    pub fn register(&mut self, verb: &'static str, handler: Handler) {
        self.handlers.insert(verb, handler);
    }

    /// The release the dataset is for.
    pub fn release(&self) -> &str {
        &self.release
    }

    fn request(&self, id: &str) -> Option<ActionRequest> {
        let field = |f: &str| {
            self.replica
                .get(id, f)
                .and_then(|b| std::str::from_utf8(b).ok())
                .map(str::to_string)
        };
        let params = match parse(&field(action::PARAMS)?).ok()? {
            Json::Obj(pairs) => pairs
                .into_iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
                .collect(),
            _ => return None,
        };
        Some(ActionRequest {
            id: id.to_string(),
            verb: field(action::VERB)?,
            target: field(action::TARGET)?,
            params,
            by: field(action::BY)?,
        })
    }

    /// Decide one complete action for `grant`.
    fn decide(&mut self, grant: &Grant, req: &ActionRequest) -> Outcome {
        if req.by != grant.persona().id {
            return Outcome::Refused("an action is made only by the persona signed in".into());
        }
        // The target must be a row the grant may see. A hidden row is refused exactly
        // as a missing one is, so the answer does not tell a reader it exists.
        match self.state.marking_of(&req.target) {
            Some(m) if may_see(grant.clearance(), m) => {}
            _ => return Outcome::Refused("no such target".into()),
        }
        match self.handlers.get(req.verb.as_str()) {
            Some(h) => h(&mut self.state, grant, req),
            None => Outcome::Refused(format!("no such action: {}", req.verb)),
        }
    }
}

impl DataSource for DemoEnclosure {
    fn personas(&self) -> Vec<Persona> {
        self.personas.clone()
    }

    fn collections(&self) -> Vec<(String, String)> {
        self.state
            .collections
            .iter()
            .map(|(n, c)| (n.clone(), c.type_name.clone()))
            .collect()
    }

    fn grant(&self, persona_id: &str) -> Result<Grant, SourceError> {
        let p = self
            .personas
            .iter()
            .find(|p| p.id == persona_id)
            .ok_or_else(|| SourceError::UnknownPersona(persona_id.into()))?;
        Ok(Grant::new(
            p.clone(),
            clearance(p.clearance, p.compartments.iter().cloned()),
        ))
    }

    fn fetch(&self, grant: &Grant, collection: &str) -> Result<Vec<Delivered>, SourceError> {
        if !self.online {
            return Err(SourceError::Offline);
        }
        // Re-read the persona's clearance from this source's own table: a grant is
        // honoured only for a persona this source knows, at the clearance it holds.
        let grant = self.grant(&grant.persona().id)?;
        let c = self
            .state
            .collections
            .get(collection)
            .ok_or_else(|| SourceError::UnknownCollection(collection.into()))?;
        Ok(c.rows
            .iter()
            .map(|r| {
                if !may_see(grant.clearance(), &r.marking) {
                    return Delivered::Withheld {
                        level: r.marking.level(),
                    };
                }
                let body = match r.signature {
                    Some(status) => canon::encode(&audit::with_status(&r.item, status)),
                    None => canon::encode(&r.item),
                };
                Delivered::Row {
                    id: r.id.clone(),
                    marking: r.marking.clone(),
                    body: body.into_bytes(),
                    signature: r.signature,
                }
            })
            .collect())
    }

    fn exchange(&mut self, grant: &Grant, ops: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, SourceError> {
        if !self.online {
            return Err(SourceError::Offline);
        }
        let grant = self.grant(&grant.persona().id)?;
        let decoded: Vec<SyncOp> = ops
            .iter()
            .map(|b| SyncOp::decode(b))
            .collect::<Result<_, _>>()
            .map_err(|e| SourceError::Malformed(e.to_string()))?;
        // Only a client's own fields of action objects are taken: a client cannot
        // write a status, audit id or result, nor touch other objects here.
        let taken: Vec<&SyncOp> = decoded
            .iter()
            .filter(|o| {
                o.object.starts_with(action::PREFIX)
                    && action::CLIENT_FIELDS.contains(&o.field.as_str())
            })
            .collect();
        self.replica.merge_all(taken.iter().copied());
        let mut ids: Vec<String> = taken.iter().map(|o| o.object.clone()).collect();
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            if self.replica.get(&id, action::STATUS).is_some() {
                continue; // decided already: a re-push changes nothing
            }
            let Some(req) = self.request(&id) else {
                continue; // incomplete: wait for its other fields
            };
            let (status, reason, result) = match self.decide(&grant, &req) {
                Outcome::Applied(r) => ("applied", String::new(), canon::encode(&r)),
                Outcome::Refused(why) => ("refused", why, "{}".into()),
                Outcome::Conflict(why) => ("conflict", why, "{}".into()),
            };
            let console = req.verb.split('.').next().unwrap_or("midgard").to_string();
            let audit = self.state.audit(
                &console,
                &req.by,
                &format!("{} · {}", status, req.verb),
                &req.target,
            );
            self.replica
                .set(&id, action::STATUS, status.as_bytes().to_vec());
            self.replica.set(&id, action::REASON, reason.into_bytes());
            self.replica.set(&id, action::AUDIT, audit.into_bytes());
            self.replica.set(&id, action::RESULT, result.into_bytes());
        }
        Ok(self
            .replica
            .drain_pending()
            .iter()
            .map(SyncOp::encode)
            .collect())
    }

    fn online(&self) -> bool {
        self.online
    }

    fn set_online(&mut self, online: bool) {
        self.online = online;
    }
}
