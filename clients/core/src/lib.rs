//! # midgard-client-core — one core, many shells
//!
//! This crate is the architectural spine of the client layer: **crypto, sync, and
//! storage implemented once** and composed behind one typed façade. Every one of
//! the seven targets links *this* crate and calls it for everything security- or
//! data-related; the shells add only presentation. A bug in the sensitive logic is
//! fixed here, once, and every platform inherits the fix. `[FR-C-1]` `[AC-C-1]`
//! `[KDD-1]`
//!
//! The concerns live in focused sub-crates, re-exported here so there is exactly
//! **one linkable core**:
//! - [`sync`] — offline-first CRDT ([`midgard_client_sync`]).
//! - [`store`] — local encrypted, clearance-scoped cache ([`midgard_client_store`]).
//! - [`keystore`] — device key custody + floors ([`midgard_client_keystore`]).
//! - [`enclosure`] — the PQ-mTLS client ([`midgard_client_enclosure`]).
//!
//! [`ClientCore`] ties them together: an edit becomes a CRDT op (sync), the object
//! snapshot is sealed into the local cache under a keystore-wrapped key (storage +
//! crypto), and above-clearance data is refused (policy). The façade deliberately
//! exposes **no key material, no merge internals, and no handshake primitive** — a
//! shell cannot reach around it, which is how KDD-1 holds structurally.

#![forbid(unsafe_code)]

pub use midgard_client_enclosure as enclosure;
pub use midgard_client_keystore as keystore;
pub use midgard_client_store as store;
pub use midgard_client_sync as sync;

use keystore::{Keystore, SecurityFloor, TrustRoot};
use policy::{dominates, Clearance, Marking};
use pqc_agility::{CryptoProvider, Rng};
use store::LocalStore;
use sync::{Replica, SyncOp};

mod cache;
mod error;
pub use cache::{ExportedState, ImportReport, CLOCK_OBJECT, DEVICE_AUDIT_OBJECT, PENDING_OBJECT};
pub use error::CoreError;
pub mod consoles;
pub mod demo;
pub mod device_audit;
pub mod local_source;
pub mod marking;
pub mod sample_source;
pub mod session;
pub mod source;

/// The version of the shared core every shell links. All client sub-crates share
/// the workspace version, so this single string identifies the core on every
/// target — the concrete meaning of "each links the same `midgard-client-core`
/// version". `[AC-C-1]`
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The composed client core: one CRDT replica over one encrypted, clearance-scoped
/// local cache. Constructed from a key-holding keystore; the extension's brokered
/// path uses the broker layer (Phase 8.d), not this façade.
///
/// Generic over the keystore, so a platform keystore (Secure Enclave, StrongBox,
/// TPM) whose floor derives from the platform plugs in with no change here; the
/// in-memory [`SoftwareKeystore`](keystore::SoftwareKeystore) reports only the
/// NonExtractable floor. `[FR-C-2]` `[VLD-ANB-MIDGARD-0056]`
pub struct ClientCore<K: Keystore + TrustRoot> {
    store: LocalStore<K>,
    replica: Replica,
    /// The next action number; persisted in the state file.
    next_action: u64,
    /// A random number drawn at the first action id of this launch.
    action_nonce: Option<u64>,
}

impl<K: Keystore + TrustRoot> ClientCore<K> {
    /// Open a core over a device keystore, for one persona (`scope`) at a session
    /// clearance. Fails closed if custody is below `min_floor`. The cache, in memory
    /// and in the state file, is that persona's alone; there is no unscoped core.
    /// `[FR-C-2]` `[AC-C-13]`
    pub fn open_scoped(
        provider: CryptoProvider,
        keystore: K,
        clearance: Clearance,
        min_floor: SecurityFloor,
        replica_id: impl Into<String>,
        scope: &str,
    ) -> Result<Self, CoreError> {
        let store = LocalStore::open_scoped(provider, keystore, clearance, min_floor, scope)?;
        Ok(Self {
            store,
            replica: Replica::new(replica_id),
            next_action: 1,
            action_nonce: None,
        })
    }

    /// The replica id (stable per device).
    pub fn replica_id(&self) -> &str {
        self.replica.id()
    }

    /// The persona this core's cache belongs to.
    pub fn scope(&self) -> &str {
        self.store.scope()
    }

    /// Re-scope to another persona or clearance (a persona switch). To another
    /// persona, everything cached and the CRDT view are dropped, whatever the new
    /// clearance. To the same persona at a clearance that does not dominate the old
    /// one, rows it does not dominate and the CRDT view are dropped (the view may
    /// hold fields of rows now above it); at one that does, nothing is dropped.
    /// Refused, with nothing changed, while ops are pending and the view would be
    /// dropped. Returns the core and how many cached rows were dropped. The clock
    /// and the action counter are kept. `[AC-C-13]`
    pub fn rescope(
        self,
        scope: &str,
        clearance: Clearance,
    ) -> Result<(Self, usize), (Self, CoreError)> {
        let old = self.store.clearance();
        let old = Marking {
            level: old.level,
            compartments: old.compartments.clone(),
        };
        let forget = scope != self.store.scope() || !dominates(&clearance, &old);
        if forget && self.replica.has_pending() {
            let n = self.replica.pending().len();
            return Err((self, CoreError::PendingOps(n)));
        }
        let Self {
            store,
            mut replica,
            next_action,
            action_nonce,
        } = self;
        if forget {
            replica.forget();
        }
        let (store, dropped) = store.rescope(scope, clearance);
        let core = Self {
            store,
            replica,
            next_action,
            action_nonce,
        };
        Ok((core, dropped))
    }

    /// A new action object id, `action:<replica>-<nonce>-<n>`: `n` counts up and is
    /// persisted in the state file; `nonce` is a random 64-bit number drawn once per
    /// launch. The counter alone would repeat after a state file is discarded
    /// (tampered, rolled back, reset), and a source answers a reused id with the
    /// decision it already made; the nonce makes that repetition negligible. The id
    /// is not a secret: a source keys decisions by persona as well. `[FR-C-4]`
    pub fn new_action_id(&mut self, rng: &mut dyn Rng) -> String {
        let nonce = *self.action_nonce.get_or_insert_with(|| {
            let mut b = [0u8; 8];
            rng.fill(&mut b);
            u64::from_le_bytes(b)
        });
        let n = self.next_action;
        self.next_action += 1;
        format!(
            "{}{}-{nonce:016x}-{n}",
            source::action::PREFIX,
            self.replica.id()
        )
    }

    /// Edit a field of an object. Works offline: the edit becomes a queued CRDT op
    /// and the object's current snapshot is sealed into the local cache. Refused
    /// (nothing mutated) if `marking` exceeds the session clearance. `[FR-C-4]`
    /// `[AC-C-13]`
    pub fn edit_field(
        &mut self,
        object: &str,
        field: &str,
        value: Vec<u8>,
        marking: Marking,
        rng: &mut dyn Rng,
    ) -> Result<SyncOp, CoreError> {
        // The core's own objects are not edited from outside.
        if object.starts_with(store::RESERVED_PREFIX) {
            return Err(CoreError::Reserved(object.escape_debug().to_string()));
        }
        // Pre-check clearance so a refusal leaves the CRDT untouched.
        if !dominates(self.store.clearance(), &marking) {
            return Err(CoreError::from(store::StoreError::AboveClearance));
        }
        let op = self.replica.set(object, field, value);
        let snapshot = encode_object(&self.replica, object);
        self.store.put(object, marking, &snapshot, rng)?;
        Ok(op)
    }

    /// The current value of a field from the in-memory CRDT view.
    pub fn read_field(&self, object: &str, field: &str) -> Option<Vec<u8>> {
        self.replica.get(object, field).map(|b| b.to_vec())
    }

    /// Recover an object's sealed snapshot from the encrypted local cache. Proves
    /// the storage + crypto path end-to-end (decrypts under the keystore key).
    pub fn load_cached(&self, object: &str) -> Result<Vec<u8>, CoreError> {
        Ok(self.store.get(object)?)
    }

    /// The ops queued locally and not yet synced to the enclosure.
    pub fn pending_ops(&self) -> &[SyncOp] {
        self.replica.pending()
    }

    /// Take the queued ops to push on reconnect, clearing the queue.
    pub fn drain_pending(&mut self) -> Vec<SyncOp> {
        self.replica.drain_pending()
    }

    /// Merge remote ops received from the enclosure relay (conflict-free).
    pub fn merge_remote(&mut self, ops: &[SyncOp]) {
        self.replica.merge_all(ops.iter());
    }

    /// The object ids the session clearance may render.
    pub fn visible_objects(&self) -> Vec<String> {
        self.store.visible_ids()
    }

    // ---- binding surface (UniFFI / wasm-bindgen friendly) -------------------
    //
    // The shells link this crate through UniFFI (Apple/Android) and wasm-bindgen
    // (web/extension). Those bindings cannot carry a borrowed `&[SyncOp]` or the
    // CRDT `SyncOp` type across the FFI boundary, and — per KDD-1 — a shell must
    // never touch merge internals. So the ops cross as **opaque owned byte blobs**:
    // the shell drains them, ships them to the enclosure relay, and feeds returned
    // blobs back, never inspecting them. These two methods are the FFI-shaped
    // counterparts of `drain_pending` / `merge_remote`. [RT-58] [FR-C-1] [KDD-1]

    /// Take the queued ops as opaque encoded blobs (clearing the queue) — the
    /// FFI-crossable form of [`drain_pending`](ClientCore::drain_pending).
    pub fn drain_pending_encoded(&mut self) -> Vec<Vec<u8>> {
        self.replica
            .drain_pending()
            .iter()
            .map(SyncOp::encode)
            .collect()
    }

    /// Merge remote ops received as opaque encoded blobs. **Fails closed** if any
    /// blob is malformed — nothing is applied unless every op decodes. The
    /// FFI-crossable form of [`merge_remote`](ClientCore::merge_remote).
    pub fn merge_remote_encoded(&mut self, ops: &[Vec<u8>]) -> Result<(), CoreError> {
        let decoded: Vec<SyncOp> = ops
            .iter()
            .map(|b| SyncOp::decode(b))
            .collect::<Result<_, _>>()?;
        self.replica.merge_all(decoded.iter());
        Ok(())
    }
}

/// Canonically encode an object's live fields from the CRDT snapshot — the bytes
/// sealed into the local cache. Length-prefixed and deterministic.
fn encode_object(replica: &Replica, object: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for ((obj, field), value) in replica.snapshot() {
        if obj != object {
            continue;
        }
        out.extend_from_slice(&(field.len() as u32).to_le_bytes());
        out.extend_from_slice(field.as_bytes());
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(&value);
    }
    out
}
