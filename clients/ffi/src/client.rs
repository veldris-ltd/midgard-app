//! `MidgardClient`: the one object a shell holds. `[FR-C-1]` `[AC-C-13]` `[FR-C-4]`
//! `[NFR-PQ-3]`
//!
//! The shell keeps every file: the sealed root and the sealed device-audit key from
//! [`MidgardClient::enrol`] ([`SealedKeys`]), and each persona's state file from
//! [`MidgardClient::export_state`] with its generation. It hands them back as bytes;
//! all of them are sealed, and this crate opens them only in the core. The one file
//! this crate reads itself is the bundled dataset, verified under the anchors
//! compiled into the core (`DemoEnclosure::open_pinned`), and only when
//! [`ClientConfig::source`] is [`SourceChoice::Bundled`]. [`SourceChoice::Sample`]
//! runs on the release's own sample data compiled into the core (`SampleSource`,
//! owner decision 22 Nov 2026), and [`SourceChoice::None`] with no data source
//! (`LocalSource`, owner decision 19 Nov 2026): one `local` persona, every collection
//! empty, every action refused. A path that does not open or verify is an error,
//! never a fallback to another mode. `[FR-C-5]`

use std::sync::{Arc, Mutex, MutexGuard};

use midgard_client_core::demo::enclosure::DemoEnclosure;
use midgard_client_core::device_audit::DeviceAuditKey;
use midgard_client_core::keystore::sealed_root::SealedRootKeystore;
use midgard_client_core::keystore::Keystore;
use midgard_client_core::local_source::LocalSource;
use midgard_client_core::marking::RowMarking;
use midgard_client_core::sample_source::SampleSource;
use midgard_client_core::session::{self, Session};
use midgard_client_core::source::{
    DataSource, Delivered, Grant, Persona, Seal, SourceError, VerifiedDigest,
};
use midgard_client_inference::tier::{
    guard, heuristic, Connectivity, DeviceCapability, Heuristic, PrivacyPreference, TaskComplexity,
    Tier, TierContext,
};
use pqc_agility::backend::fips::provider;
use pqc_agility::OsRng;

use crate::key_root::{KeyRoot, Sealer};
use crate::rows::{typed_rows, ConsoleRow};
use crate::{ClientError, KeyValue};

/// The key id of every device root: a constant, so no device or persona name is in
/// the sealed blob.
const ROOT_KEY_ID: &str = "device-root";

/// What the client runs on: the shell's Info.plist `MidgardDataSource`. `[FR-C-5]`
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SourceChoice {
    /// No data source (`none`): one `local` persona, every collection empty.
    None,
    /// The signed release's own sample data, compiled into the core (`sample`).
    Sample,
    /// The bundled, signed demo dataset at `path` (`bundled`).
    Bundled {
        /// Path of the bundled `<release>.mgdemo`.
        path: String,
    },
}

/// How the shell configures the client.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ClientConfig {
    /// What the client runs on. `[FR-C-5]`
    pub source: SourceChoice,
    /// Stable per install.
    pub replica_id: String,
    /// The device, for the inference tier.
    pub device: DeviceInfo,
}

/// What the client runs on, for the banner. `[FR-C-5]` `[AC-C-13]`
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DataSourceKind {
    /// No data source: one `local` persona, every collection empty ("No data source
    /// connected").
    None,
    /// The bundled, signed demo dataset (the demonstration banner).
    Demo,
    /// The release's own sample data (the demonstration banner, as the web consoles
    /// show it in mock mode).
    Sample,
}

/// Device capability as the shell measures it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DeviceInfo {
    /// Physical memory, MiB.
    pub ram_mb: u32,
    /// A Neural Engine / GPU is present.
    pub has_accelerator: bool,
}

/// What enrolment makes, for the shell to store: both sealed, neither readable
/// without the platform key.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SealedKeys {
    /// The sealed device root (ML-KEM-1024, sealed under the data key the platform
    /// key root seals).
    pub root: Vec<u8>,
    /// The device-audit signing key, sealed to that root.
    pub device_audit: Vec<u8>,
}

/// A sealed state file and its generation. Write the file first, then keep the
/// generation for this persona.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ExportedState {
    /// The sealed file.
    pub file: Vec<u8>,
    /// Its generation.
    pub generation: u64,
}

/// What a state import loaded.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ImportReport {
    /// Cached rows and indexes loaded.
    pub kept: u32,
    /// Cached rows and indexes dropped (above the current clearance).
    pub dropped: u32,
    /// The file held unsent actions this clearance may not read: they are lost.
    /// Tell the user.
    pub pending_dropped: bool,
    /// The file's generation.
    pub generation: u64,
}

/// A persona the sign-in screen lists.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PersonaInfo {
    /// Id.
    pub id: String,
    /// Display name.
    pub display: String,
    /// Role word.
    pub role: String,
    /// `CONFIDENTIAL // OPS-N, SIG`.
    pub clearance_label: String,
    /// Policy roles.
    pub roles: Vec<String>,
}

/// A collection the source serves.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CollectionInfo {
    /// `<console>.<name>`.
    pub name: String,
    /// The read-model type ([`crate::RowItem`] variant).
    pub row_type: String,
}

/// Marking level for chips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MarkingLevelChip {
    /// open
    Open,
    /// internal
    Internal,
    /// restricted
    Restricted,
    /// confidential
    Confidential,
}

/// A row's marking chip.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MarkingChip {
    /// Level (fixed colour).
    pub level: MarkingLevelChip,
    /// Full label.
    pub label: String,
}

/// Sync phase for the chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SyncPhase {
    /// Nothing pending.
    Synced,
    /// Pending and reachable.
    Syncing,
    /// Unreachable.
    Offline,
    /// Last sync had a conflict.
    Conflict,
}

/// The sync chip.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SyncChip {
    /// Phase.
    pub phase: SyncPhase,
    /// Ops queued.
    pub pending: u32,
    /// Unix seconds of the last sync in this session.
    pub last_sync: Option<u64>,
}

/// Inference tier for the chip (`clients/inference/src/tier.rs` `Tier`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum InferenceTier {
    /// Tier 1.
    OnDevice,
    /// Tier 2.
    Remote,
    /// Tier 3.
    Hybrid,
    /// Off.
    Disabled,
}

/// The inference-tier chip.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TierChip {
    /// Tier.
    pub tier: InferenceTier,
    /// Why, in a phrase.
    pub reason: String,
}

/// The custody chip.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CustodyChip {
    /// The keystore's honest floor (`SecurityFloor` name): `NonExtractable` for the
    /// sealed root (owner decision 5).
    pub floor: String,
    /// Whether the root is sealed by a hardware-held platform key.
    pub hardware_wrapped: bool,
}

/// All chips a screen header shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StatusChips {
    /// Sync.
    pub sync: SyncChip,
    /// Inference tier.
    pub tier: TierChip,
    /// Custody.
    pub custody: CustodyChip,
}

/// After unlock or switch.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SessionInfo {
    /// Who.
    pub persona: PersonaInfo,
    /// Chips.
    pub chips: StatusChips,
}

/// An action to queue.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ActionInput {
    /// `<console>.<verb>`.
    pub verb: String,
    /// `<collection>/<row id>`.
    pub target: String,
    /// String parameters.
    pub params: Vec<KeyValue>,
}

/// An action's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ActionState {
    /// Queued or in flight.
    Queued,
    /// Applied.
    Applied,
    /// Refused.
    Refused,
    /// Conflict.
    Conflict,
    /// Unknown here.
    Unknown,
}

/// An action's status for the screen.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ActionStatus {
    /// Id.
    pub action_id: String,
    /// State.
    pub state: ActionState,
    /// Reason when refused or in conflict.
    pub reason: Option<String>,
    /// Audit id when applied.
    pub audit_id: Option<String>,
    /// Canonical JSON result when applied.
    pub result: Option<String>,
}

/// A sync's report.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SyncReport {
    /// Ops pushed.
    pub pushed: u32,
    /// Ops merged.
    pub merged: u32,
    /// Chips after.
    pub chips: StatusChips,
}

/// What serves the session: the signed demo dataset, the release's sample data, or
/// no data at all. Delegates every call, the seal included, so the demo's in-core
/// audit checks still reach the session. `[FR-C-5]` `[NFR-PQ-3]`
enum Source {
    Demo(Box<DemoEnclosure>),
    Sample(Box<SampleSource>),
    Local(LocalSource),
}

impl DataSource for Source {
    fn personas(&self) -> Vec<Persona> {
        match self {
            Source::Demo(s) => s.personas(),
            Source::Sample(s) => s.personas(),
            Source::Local(s) => s.personas(),
        }
    }

    fn collections(&self) -> Vec<(String, String)> {
        match self {
            Source::Demo(s) => s.collections(),
            Source::Sample(s) => s.collections(),
            Source::Local(s) => s.collections(),
        }
    }

    fn grant(&self, persona_id: &str) -> Result<Grant, SourceError> {
        match self {
            Source::Demo(s) => s.grant(persona_id),
            Source::Sample(s) => s.grant(persona_id),
            Source::Local(s) => s.grant(persona_id),
        }
    }

    fn fetch(&self, grant: &Grant, collection: &str) -> Result<Vec<Delivered>, SourceError> {
        match self {
            Source::Demo(s) => s.fetch(grant, collection),
            Source::Sample(s) => s.fetch(grant, collection),
            Source::Local(s) => s.fetch(grant, collection),
        }
    }

    fn exchange(&mut self, grant: &Grant, ops: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, SourceError> {
        match self {
            Source::Demo(s) => s.exchange(grant, ops),
            Source::Sample(s) => s.exchange(grant, ops),
            Source::Local(s) => s.exchange(grant, ops),
        }
    }

    fn takes_actions(&self) -> bool {
        match self {
            Source::Demo(s) => s.takes_actions(),
            Source::Sample(s) => s.takes_actions(),
            Source::Local(s) => s.takes_actions(),
        }
    }

    fn online(&self) -> bool {
        match self {
            Source::Demo(s) => s.online(),
            Source::Sample(s) => s.online(),
            Source::Local(s) => s.online(),
        }
    }

    fn set_online(&mut self, online: bool) {
        match self {
            Source::Demo(s) => s.set_online(online),
            Source::Sample(s) => s.set_online(online),
            Source::Local(s) => s.set_online(online),
        }
    }

    fn in_core_verified(
        &self,
        seal: &Seal,
        grant: &Grant,
        collection: &str,
        id: &str,
    ) -> Option<VerifiedDigest> {
        match self {
            Source::Demo(s) => s.in_core_verified(seal, grant, collection, id),
            Source::Sample(s) => s.in_core_verified(seal, grant, collection, id),
            Source::Local(s) => s.in_core_verified(seal, grant, collection, id),
        }
    }
}

type Live = Session<SealedRootKeystore, Source>;

/// A signed-in session and the custody facts read from its keystore before it moved
/// into the session (the keystore's internals are not reachable after that).
struct Open {
    session: Live,
    custody: CustodyChip,
}

enum State {
    Locked(Box<Source>),
    /// An unlock is waiting on the platform prompt, with the client's lock released:
    /// every other call sees a locked client (never a blocked one).
    Unlocking,
    Open(Box<Open>),
    /// While a transition holds the lock, or after one could not reload the source
    /// or was cut short by a panic in the platform side (reads as locked; `unlock`
    /// and `reset_demo` reload it).
    Gone,
}

/// Marks the client `Gone` if an unlock is unwound (a panic in the platform side)
/// while it waits on the prompt, so the client is never stuck `Unlocking`.
struct UnlockGuard<'a> {
    state: &'a Mutex<State>,
    armed: bool,
}

impl Drop for UnlockGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if matches!(&*st, State::Unlocking) {
                *st = State::Gone;
            }
        }
    }
}

/// The client a shell holds: locked or signed in.
#[derive(uniffi::Object)]
pub struct MidgardClient {
    config: ClientConfig,
    key_root: Arc<dyn KeyRoot>,
    /// The source's personas and collections, read once: they do not change, and
    /// the sign-in screen needs them whatever the state.
    personas: Vec<Persona>,
    collections: Vec<(String, String)>,
    state: Mutex<State>,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn persona_info(p: &Persona) -> PersonaInfo {
    PersonaInfo {
        id: p.id.clone(),
        display: p.display.clone(),
        role: p.role.clone(),
        clearance_label: RowMarking::new(p.clearance, p.compartments.iter().cloned()).label(),
        roles: p.roles.clone(),
    }
}

/// The configured source. None: a fresh [`LocalSource`], with no file read. Sample:
/// a fresh [`SampleSource`] over the data compiled into the core, no file read
/// either. Bundled: read and verify the dataset against the anchors compiled into
/// the core, failing closed (never falling back to another source). A read error
/// names the file only: the full path holds the app's container and the user name,
/// and error text can reach the screen or a log. [FR-C-9] [FR-C-5]
fn load_source(config: &ClientConfig) -> Result<Source, ClientError> {
    let path = match &config.source {
        SourceChoice::None => return Ok(Source::Local(LocalSource::new())),
        SourceChoice::Sample => return Ok(Source::Sample(Box::new(SampleSource::new()?))),
        SourceChoice::Bundled { path } => path,
    };
    let file = std::fs::read(path).map_err(|e| {
        let name = std::path::Path::new(path).file_name().map_or_else(
            || "(no file name)".into(),
            |n| n.to_string_lossy().into_owned(),
        );
        ClientError::NotFound {
            what: format!("dataset {name}: {}", e.kind()),
        }
    })?;
    Ok(Source::Demo(Box::new(DemoEnclosure::open_pinned(&file)?)))
}

impl MidgardClient {
    fn state(&self) -> Result<MutexGuard<'_, State>, ClientError> {
        self.state.lock().map_err(|_| ClientError::Internal {
            detail: "client state poisoned".into(),
        })
    }

    fn chips(&self, open: &Open) -> StatusChips {
        let s = &open.session;
        let st = s.sync_status();
        let online = s.source().online();
        // Owner decision 6: no on-device model ships, so offline is "AI off".
        let ctx = TierContext {
            capability: DeviceCapability {
                ram_mb: self.config.device.ram_mb,
                has_accelerator: self.config.device.has_accelerator,
                on_device_model: false,
            },
            connectivity: if online {
                Connectivity::Online
            } else {
                Connectivity::Offline
            },
            allow_egress: true,
            privacy: PrivacyPreference::Standard,
            complexity: TaskComplexity::Light,
        };
        let proposed = match heuristic(&ctx) {
            Heuristic::Decided(t) => t,
            Heuristic::Ambiguous => Tier::Remote,
        };
        let (tier, reason) = match guard(proposed, &ctx) {
            Tier::OnDevice => (InferenceTier::OnDevice, "on-device"),
            Tier::Remote => (InferenceTier::Remote, "enclosure models"),
            Tier::Hybrid => (InferenceTier::Hybrid, "on-device draft, enclosure check"),
            Tier::Disable => (InferenceTier::Disabled, "AI off"),
        };
        StatusChips {
            sync: SyncChip {
                phase: match st.phase {
                    session::SyncPhase::Synced => SyncPhase::Synced,
                    session::SyncPhase::Syncing => SyncPhase::Syncing,
                    session::SyncPhase::Offline => SyncPhase::Offline,
                    session::SyncPhase::Conflict => SyncPhase::Conflict,
                },
                pending: count(st.pending),
                last_sync: st.last_sync,
            },
            tier: TierChip {
                tier,
                reason: reason.to_string(),
            },
            custody: open.custody.clone(),
        }
    }

    fn info(&self, open: &Open) -> SessionInfo {
        SessionInfo {
            persona: persona_info(open.session.grant().persona()),
            chips: self.chips(open),
        }
    }

    fn with_open<T>(
        &self,
        f: impl FnOnce(&mut Open) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        let mut st = self.state()?;
        match &mut *st {
            State::Open(o) => f(o),
            _ => Err(ClientError::Locked),
        }
    }
}

#[uniffi::export]
impl MidgardClient {
    /// Load the configured source (verifying a bundled dataset against the pinned
    /// anchors) and start locked.
    #[uniffi::constructor]
    pub fn new(config: ClientConfig, key_root: Arc<dyn KeyRoot>) -> Result<Arc<Self>, ClientError> {
        let source = load_source(&config)?;
        Ok(Arc::new(Self {
            config,
            key_root,
            personas: source.personas(),
            collections: source.collections(),
            state: Mutex::new(State::Locked(Box::new(source))),
        }))
    }

    /// What the client runs on: no data source, the sample data or the demo dataset.
    /// Fixed by the configuration, whatever the state.
    pub fn data_source_kind(&self) -> DataSourceKind {
        match self.config.source {
            SourceChoice::None => DataSourceKind::None,
            SourceChoice::Sample => DataSourceKind::Sample,
            SourceChoice::Bundled { .. } => DataSourceKind::Demo,
        }
    }

    /// The personas the sign-in screen lists.
    pub fn personas(&self) -> Vec<PersonaInfo> {
        self.personas.iter().map(persona_info).collect()
    }

    /// The collections the source serves, with their read-model types.
    pub fn collections(&self) -> Vec<CollectionInfo> {
        self.collections
            .iter()
            .map(|(name, row_type)| CollectionInfo {
                name: name.clone(),
                row_type: row_type.clone(),
            })
            .collect()
    }

    /// First use: make the device root, sealed through the platform key root (no
    /// prompt), and the device-audit key sealed to it. Store both; neither is
    /// readable without the platform key.
    pub fn enrol(&self) -> Result<SealedKeys, ClientError> {
        let sealer = Sealer(self.key_root.clone());
        let (ks, root) = SealedRootKeystore::create(&provider(), ROOT_KEY_ID, &sealer, &mut OsRng)?;
        let (_device, device_audit) = DeviceAuditKey::create(&ks, &mut OsRng)?;
        Ok(SealedKeys { root, device_audit })
    }

    /// Unlock with the stored keys (one prompt, through the key root) and sign
    /// `persona_id` in. The cache is empty: load the persona's state file with
    /// [`import_state`](Self::import_state). An unknown persona is refused before
    /// the prompt; on any error the client stays locked.
    ///
    /// The client's lock is not held while the platform side prompts: other calls
    /// meanwhile see a locked client. A `reset_demo` during the prompt wins (this
    /// unlock then returns `Locked`); a `lock` during it does nothing. Call this off
    /// the main thread, and never call the client from inside the [`KeyRoot`].
    pub fn unlock(&self, keys: SealedKeys, persona_id: String) -> Result<SessionInfo, ClientError> {
        let src = {
            let mut st = self.state()?;
            match &*st {
                State::Locked(_) => {}
                State::Gone => *st = State::Locked(Box::new(load_source(&self.config)?)),
                State::Unlocking => return Err(ClientError::UnlockInProgress),
                State::Open(_) => return Err(ClientError::AlreadyUnlocked),
            }
            if let State::Locked(src) = &*st {
                src.grant(&persona_id)?;
            }
            let State::Locked(src) = std::mem::replace(&mut *st, State::Unlocking) else {
                unreachable!("made Locked above, under the same lock")
            };
            src
        };

        // The prompt, with the client's lock released.
        let mut guard = UnlockGuard {
            state: &self.state,
            armed: true,
        };
        let opened = (|| -> Result<_, ClientError> {
            let ks = SealedRootKeystore::open(&keys.root, &Sealer(self.key_root.clone()))?;
            let device = DeviceAuditKey::open(&ks, &keys.device_audit)?;
            let custody = CustodyChip {
                floor: format!("{:?}", ks.floor()),
                hardware_wrapped: ks.hardware_wrapped(),
            };
            Ok((ks, device, custody))
        })();
        guard.armed = false;

        let mut st = self.state()?;
        if !matches!(&*st, State::Unlocking) {
            // Reset while the prompt was up: that state stands; drop this one.
            return Err(ClientError::Locked);
        }
        let (ks, device, custody) = match opened {
            Ok(o) => o,
            Err(e) => {
                *st = State::Locked(src);
                return Err(e);
            }
        };
        match Session::open(ks, device, *src, &persona_id, &self.config.replica_id) {
            Ok(session) => {
                let open = Open { session, custody };
                let info = self.info(&open);
                *st = State::Open(Box::new(open));
                Ok(info)
            }
            Err(e) => {
                // The source moved into the failed session: load it again so the
                // client stays usable, locked. If that fails too, the original
                // error is the one reported (the next unlock retries the load).
                *st = match load_source(&self.config) {
                    Ok(src) => State::Locked(Box::new(src)),
                    Err(_) => State::Gone,
                };
                Err(e.into())
            }
        }
    }

    /// Switch persona without a prompt. Refused while actions are unsent (sync
    /// first) and for an unknown persona; either way the session is unchanged.
    /// Nothing of the previous persona is kept: export its state first.
    pub fn switch_persona(&self, persona_id: String) -> Result<SessionInfo, ClientError> {
        let mut st = self.state()?;
        if !matches!(&*st, State::Open(_)) {
            return Err(ClientError::Locked);
        }
        let State::Open(open) = std::mem::replace(&mut *st, State::Gone) else {
            unreachable!("checked above, under the same lock")
        };
        let Open { session, custody } = *open;
        let (open, result) = match session.switch_persona(&persona_id) {
            Ok(session) => (Open { session, custody }, Ok(())),
            Err((session, e)) => (Open { session, custody }, Err(e)),
        };
        let info = self.info(&open);
        *st = State::Open(Box::new(open));
        result.map(|()| info).map_err(Into::into)
    }

    /// The persona signed in.
    pub fn session_persona(&self) -> Result<PersonaInfo, ClientError> {
        self.with_open(|o| Ok(persona_info(o.session.grant().persona())))
    }

    /// Lock: drop the session and with it the unlocked root and device key. Export
    /// the state first to keep it.
    pub fn lock(&self) -> Result<(), ClientError> {
        let mut st = self.state()?;
        if matches!(&*st, State::Open(_)) {
            let State::Open(open) = std::mem::replace(&mut *st, State::Gone) else {
                unreachable!("checked above, under the same lock")
            };
            *st = State::Locked(Box::new(open.session.into_source()));
        }
        Ok(())
    }

    /// Whether locked.
    pub fn is_locked(&self) -> bool {
        !matches!(self.state().as_deref(), Ok(State::Open(_)))
    }

    /// The header chips.
    pub fn status_chips(&self) -> Result<StatusChips, ClientError> {
        self.with_open(|o| Ok(self.chips(o)))
    }

    /// Simulate going offline (or back online).
    pub fn set_offline(&self, offline: bool) -> Result<StatusChips, ClientError> {
        self.with_open(|o| {
            o.session.set_online(!offline);
            Ok(self.chips(o))
        })
    }

    /// Push queued actions and merge the answer. Export the state afterwards.
    pub fn sync_now(&self) -> Result<SyncReport, ClientError> {
        self.with_open(|o| {
            let r = o.session.sync()?;
            Ok(SyncReport {
                pushed: count(r.pushed),
                merged: count(r.merged),
                chips: self.chips(o),
            })
        })
    }

    /// A collection's rows: shown rows typed, withheld rows by level only. Online
    /// they are fetched and cached; offline they come from the sealed cache.
    pub fn collection_rows(&self, collection: String) -> Result<Vec<ConsoleRow>, ClientError> {
        self.with_open(|o| {
            let row_type = o
                .session
                .source()
                .collections()
                .into_iter()
                .find(|(name, _)| *name == collection)
                .map(|(_, t)| t)
                .ok_or_else(|| ClientError::UnknownCollection {
                    collection: collection.clone(),
                })?;
            typed_rows(&mut o.session, &row_type, &collection)
        })
    }

    /// Queue an action on a row this persona has read. Its marking comes from the
    /// persona's cached delivery, never from the shell. With no data source every
    /// action is `Refused` ("no data source connected") and nothing is queued.
    pub fn request_action(&self, action: ActionInput) -> Result<String, ClientError> {
        self.with_open(|o| {
            if matches!(o.session.source(), Source::Local(_)) {
                return Err(SourceError::NoSource.into());
            }
            let params: Vec<(String, String)> = action
                .params
                .into_iter()
                .map(|kv| (kv.key, kv.value))
                .collect();
            Ok(o.session
                .request_action(&action.verb, &action.target, &params)?)
        })
    }

    /// An action's state.
    pub fn action_status(&self, action_id: String) -> Result<ActionStatus, ClientError> {
        self.with_open(|o| {
            let (state, reason, audit_id, result) = match o.session.action_state(&action_id) {
                session::ActionState::Queued => (ActionState::Queued, None, None, None),
                session::ActionState::Applied { audit, result } => {
                    (ActionState::Applied, None, Some(audit), Some(result))
                }
                session::ActionState::Refused { reason } => {
                    (ActionState::Refused, Some(reason), None, None)
                }
                session::ActionState::Conflict { reason } => {
                    (ActionState::Conflict, Some(reason), None, None)
                }
                session::ActionState::Unknown => (ActionState::Unknown, None, None, None),
            };
            Ok(ActionStatus {
                action_id,
                state,
                reason,
                audit_id,
                result,
            })
        })
    }

    /// The persona's state as a sealed file. Write it (atomically), then keep its
    /// generation for this persona.
    pub fn export_state(&self) -> Result<ExportedState, ClientError> {
        self.with_open(|o| {
            let e = o.session.export_state()?;
            Ok(ExportedState {
                file: e.file,
                generation: e.generation,
            })
        })
    }

    /// Load the persona's state file. `min_generation` is the generation kept for
    /// this persona. On any error the cache is empty: discard the file; the next
    /// read refetches. `Stale` is a rollback.
    pub fn import_state(
        &self,
        file: Vec<u8>,
        min_generation: u64,
    ) -> Result<ImportReport, ClientError> {
        self.with_open(|o| {
            let r = o.session.import_state(&file, min_generation)?;
            Ok(ImportReport {
                kept: count(r.kept),
                dropped: count(r.dropped),
                pending_dropped: r.pending_dropped,
                generation: r.generation,
            })
        })
    }

    /// "Reset demo": lock and reload the dataset, so the demo's own state (actions
    /// applied, audit rows appended) starts again; with no data source, lock with a
    /// fresh `LocalSource` (no file is read); with the sample data, a fresh
    /// `SampleSource`. The shell deletes its files and its
    /// platform key itself.
    ///
    /// Also the way back from a client whose lock was poisoned by a panic: the
    /// poison is cleared and a fresh locked state installed.
    pub fn reset_demo(&self) -> Result<(), ClientError> {
        let source = load_source(&self.config)?;
        let mut st = self.state.lock().unwrap_or_else(|p| {
            self.state.clear_poison();
            p.into_inner()
        });
        *st = State::Locked(Box::new(source));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_root::KeyRootError;
    use attest::TrustAnchors;

    const BODY: &str = include_str!("../../core/tests/fixtures/session_body.json");

    struct Plain;
    impl KeyRoot for Plain {
        fn seal(&self, k: Vec<u8>) -> Result<Vec<u8>, KeyRootError> {
            // Not a seal, but not the key in the clear either: enough for the guard.
            Ok(k.iter().map(|b| b ^ 0x5a).chain([0u8; 16]).collect())
        }
        fn unseal(&self, s: Vec<u8>) -> Result<Vec<u8>, KeyRootError> {
            Ok(s[..32].iter().map(|b| b ^ 0x5a).collect())
        }
        fn hardware_backed(&self) -> bool {
            false
        }
    }

    fn config() -> ClientConfig {
        ClientConfig {
            source: SourceChoice::Bundled {
                path: "unused".into(),
            },
            replica_id: "unit".into(),
            device: DeviceInfo {
                ram_mb: 8192,
                has_accelerator: false,
            },
        }
    }

    fn client_over(body: &str) -> MidgardClient {
        let json = attest::json::parse(body).unwrap();
        let e = DemoEnclosure::from_body("test", &json, &TrustAnchors::new()).unwrap();
        MidgardClient {
            config: config(),
            key_root: Arc::new(Plain),
            personas: e.personas(),
            collections: e.collections(),
            state: Mutex::new(State::Locked(Box::new(Source::Demo(Box::new(e))))),
        }
    }

    #[test]
    fn unsent_actions_lost_on_import_reach_the_shell() {
        let a = client_over(BODY);
        let keys = a.enrol().unwrap();
        a.unlock(keys.clone(), "k.osei".into()).unwrap();
        a.collection_rows("midgard.audit".into()).unwrap();
        a.set_offline(true).unwrap();
        a.request_action(ActionInput {
            verb: "demo.ping".into(),
            target: "midgard.audit/AUD-1".into(),
            params: vec![],
        })
        .unwrap();
        let saved = a.export_state().unwrap();

        // The same persona, now at a lower clearance: the queue cannot be read.
        let lower = BODY.replace(
            r#""clearance":"confidential","compartments":["OPS-N","SIG"]"#,
            r#""clearance":"restricted","compartments":[]"#,
        );
        assert_ne!(lower, BODY);
        let b = client_over(&lower);
        b.unlock(keys, "k.osei".into()).unwrap();
        let report = b.import_state(saved.file, saved.generation).unwrap();
        assert!(report.pending_dropped);
        assert_eq!(b.status_chips().unwrap().sync.pending, 0);
    }

    fn local_client() -> MidgardClient {
        MidgardClient {
            config: ClientConfig {
                source: SourceChoice::None,
                ..config()
            },
            key_root: Arc::new(Plain),
            personas: LocalSource::new().personas(),
            collections: LocalSource::new().collections(),
            state: Mutex::new(State::Locked(Box::new(Source::Local(LocalSource::new())))),
        }
    }

    /// A state file for persona `local` that carries an unsent action (made over a
    /// dataset that defines `local` at `open`) imports with nothing queued in local
    /// mode: no op could ever drain there. The demo keeps it. [FR-C-4] [FR-C-5]
    #[test]
    fn local_mode_drops_unsent_actions_on_import() {
        let body = BODY
            .replace(
                r#"{"id":"k.rao","display":"K. Rao","role":"analyst","clearance":"restricted","compartments":[],"roles":["analyst"]}"#,
                r#"{"id":"local","display":"Local user","role":"operator","clearance":"open","compartments":[],"roles":[]}"#,
            )
            .replace(
                r#"{"id":"AUD-1","marking":{"level":"internal""#,
                r#"{"id":"AUD-1","marking":{"level":"open""#,
            );
        assert!(body.contains(r#""id":"local""#) && body.contains(r#""level":"open""#));
        let demo = client_over(&body);
        let keys = demo.enrol().unwrap();
        demo.unlock(keys.clone(), "local".into()).unwrap();
        demo.collection_rows("midgard.audit".into()).unwrap();
        demo.set_offline(true).unwrap();
        demo.request_action(ActionInput {
            verb: "demo.ping".into(),
            target: "midgard.audit/AUD-1".into(),
            params: vec![],
        })
        .unwrap();
        let saved = demo.export_state().unwrap();

        // The demo requeues it.
        let again = client_over(&body);
        again.unlock(keys.clone(), "local".into()).unwrap();
        let r = again
            .import_state(saved.file.clone(), saved.generation)
            .unwrap();
        assert!(!r.pending_dropped);
        assert_eq!(again.status_chips().unwrap().sync.pending, 4);

        // Local mode drops it, and says so.
        let local = local_client();
        local.unlock(keys, "local".into()).unwrap();
        let r = local.import_state(saved.file, saved.generation).unwrap();
        assert!(r.pending_dropped);
        assert_eq!(local.status_chips().unwrap().sync.pending, 0);
        let report = local.sync_now().unwrap();
        assert_eq!((report.pushed, report.merged), (0, 0));
    }

    #[test]
    fn reset_demo_recovers_a_poisoned_client() {
        let c = Arc::new(client_over(BODY));
        let c2 = c.clone();
        let _ = std::thread::spawn(move || {
            let _held = c2.state.lock().unwrap();
            panic!("poison the client's lock");
        })
        .join();
        assert!(c.state.is_poisoned());
        assert!(matches!(
            c.status_chips(),
            Err(ClientError::Internal { .. })
        ));
        // reset_demo reloads the dataset from its path: point it at a real one.
        let mut c = Arc::try_unwrap(c).ok().unwrap();
        c.config.source = SourceChoice::Bundled {
            path: format!(
                "{}/../demo/datasets/{}.mgdemo",
                env!("CARGO_MANIFEST_DIR"),
                midgard_client_core::CORE_VERSION
            ),
        };
        c.reset_demo().unwrap();
        assert!(!c.state.is_poisoned());
        assert!(c.is_locked());
        let keys = c.enrol().unwrap();
        c.unlock(keys, "k.rao".into()).unwrap();
    }
}
