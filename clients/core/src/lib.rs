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

use keystore::{SecurityFloor, SoftwareKeystore};
use policy::{dominates, Clearance, Marking};
use pqc_agility::{CryptoProvider, Rng};
use store::LocalStore;
use sync::{Replica, SyncOp};

mod error;
pub use error::CoreError;

/// The version of the shared core every shell links. All client sub-crates share
/// the workspace version, so this single string identifies the core on every
/// target — the concrete meaning of "each links the same `midgard-client-core`
/// version". `[AC-C-1]`
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The composed client core: one CRDT replica over one encrypted, clearance-scoped
/// local cache. Constructed from a key-holding keystore; the extension's brokered
/// path uses the broker layer (Phase 8.d), not this façade.
pub struct ClientCore {
    store: LocalStore<SoftwareKeystore>,
    replica: Replica,
}

impl ClientCore {
    /// Open a core over a device keystore and a session clearance. Fails closed if
    /// custody is below `min_floor`.
    pub fn open(
        provider: CryptoProvider,
        keystore: SoftwareKeystore,
        clearance: Clearance,
        min_floor: SecurityFloor,
        replica_id: impl Into<String>,
    ) -> Result<Self, CoreError> {
        let store = LocalStore::open(provider, keystore, clearance, min_floor)?;
        Ok(Self {
            store,
            replica: Replica::new(replica_id),
        })
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
