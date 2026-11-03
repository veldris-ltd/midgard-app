/* @midgard/sdk · Client platforms slice.
 *
 * Models `midgard-client-core`: the single shared Rust core that every client
 * shell (iOS, Android, Linux-mobile, macOS, Windows, Linux-desktop, web WASM,
 * and the minimal-privilege browser extension) links. The core owns crypto,
 * storage and offline-first CRDT sync so the per-platform shells stay thin.
 *
 * One interface (`ClientGateway`) + one in-memory implementation (`ClientMock`)
 * + compact deterministic fixtures. All symbols are `Client`-prefixed so they
 * live alongside the other product slices in the shared `@midgard/sdk` barrel.
 * Shared governance primitives are imported from `../types`.
 *
 * The offline-first model, verbatim:
 *   - `editField` writes to the local cache and QUEUES a conflict-free pending
 *     op locally (never blocks on the network).                       `[FR-C-4]`
 *   - `sync` drains the queue against the enclosure and merges without
 *     conflicts (CRDT), returning drained/merged counts.              `[FR-C-4]`
 *   - `setOffline(true)` blocks sync; edits keep queuing and the queue grows.
 *   - The local store is PQ-sealed: AES-256-GCM under an ML-KEM-1024 data key,
 *     custodied by the platform keystore.                             `[NFR-PQ-2]`
 *   - Visibility is clearance-scoped: nothing above policy clearance is cached
 *     or rendered.                                                     `[AC-C-13]`
 *   - Inference tiers on-device / hybrid / remote; going offline forces the
 *     on-device tier so the client degrades gracefully.               `[FR-C-6]`
 *
 * The mock and its fixtures live in ../mock/clients.ts and are exported from
 * `@midgard/sdk/mock` only, so a console's main bundle never holds them.
 * [VLD-ANB-MIDGARD-0086] [AC-25.2]
 */

import type { MarkingLevel, PolicyContext } from '../types';

// ---- domain types (Client-prefixed) ----------------------------------------

/** A fabric object as cached on the device, marking-filtered by clearance. */
export interface ClientObject {
  id: string;
  /** Object type name, e.g. "Aircraft". */
  type: string;
  /** Display title, e.g. "AC-2214 · Airbus A320". */
  title: string;
  /** Editable property values keyed by field name. */
  fields: Record<string, string>;
  /** Row-level classification marking. */
  marking: MarkingLevel;
}

/** A locally-queued CRDT operation awaiting a drain to the enclosure. */
export interface ClientPendingOp {
  id: string;
  objectId: string;
  field: string;
  value: string;
  /** Display timestamp (HH:MM:SS) as stamped by the device clock. */
  ts: string;
}

/** Coarse phase of the offline-first sync engine. */
export type ClientSyncPhase = 'synced' | 'syncing' | 'offline' | 'conflict';

/** The sync engine's current state: phase + locally-queued op count + last drain. */
export interface ClientSyncState {
  phase: ClientSyncPhase;
  /** Ops queued locally, not yet drained. */
  pending: number;
  /** Display timestamp of the last successful conflict-free merge. */
  lastSync: string;
}

/** Platform keystore that custodies the local-store data key. */
export type ClientKeyStore = 'keychain' | 'keystore' | 'tpm' | 'secure-service' | 'webcrypto';

/** Where the device's keys live + whether the local store is PQ-sealed. `[NFR-PQ-2]` */
export interface ClientKeyCustody {
  store: ClientKeyStore;
  /** Local store sealed (AES-256-GCM under an ML-KEM-1024 data key). */
  sealed: boolean;
  /** Symmetric cipher over the local store. */
  cipher: string;
  /** PQ KEM wrapping the data key. */
  dataKey: string;
  /** Hardware-backing floor of the keystore, mono display. */
  floor: string;
}

/** AI inference tier that served (or would serve) a client result. `[FR-C-6]` */
export type ClientTier = 'on-device' | 'hybrid' | 'remote';

// ---- gateway seam -----------------------------------------------------------

/**
 * The single transport seam for the client shells.
 *
 * Every shell talks to this interface — never a concrete client — so the data
 * source can be swapped for the real `midgard-client-core` over the enclosure's
 * PQ-mTLS crates without touching UI code. Today the only implementation is
 * `ClientMock`.
 */
export interface ClientGateway {
  /** The active policy context (subject, purpose, clearance, roles). */
  policy(): PolicyContext;

  // reads ---------------------------------------------------------------------
  /** Objects cached on the device, filtered to policy clearance. `[AC-C-13]` */
  visibleObjects(): Promise<ClientObject[]>;
  /** Open one object by id (clearance-checked). */
  open(id: string): Promise<ClientObject>;
  /** Read a single field's current (locally-reflected) value. */
  readField(id: string, field: string): Promise<string>;

  // offline-first writes ------------------------------------------------------
  /** Edit a field: writes locally and QUEUES a conflict-free pending op. `[FR-C-4]` */
  editField(id: string, field: string, value: string): Promise<{ queued: ClientPendingOp }>;
  /** The locally-queued ops awaiting a drain. */
  pendingOps(): Promise<ClientPendingOp[]>;
  /** Drain the queue and merge conflict-free against the enclosure. Blocked
   *  while offline (returns an unchanged offline state, drained 0). `[FR-C-4]` */
  sync(): Promise<{ drained: number; merged: number; state: ClientSyncState }>;
  /** The sync engine's current state. */
  syncState(): Promise<ClientSyncState>;

  // posture -------------------------------------------------------------------
  /** Key custody + local-store seal readout. `[NFR-PQ-2]` */
  keyCustody(): Promise<ClientKeyCustody>;
  /** The inference tier that would serve now, with a reason. `[FR-C-6]` */
  inferenceTier(): Promise<{ tier: ClientTier; reason: string }>;

  // connectivity --------------------------------------------------------------
  /** Toggle the simulated network. Offline blocks sync and grows the queue. */
  setOffline(offline: boolean): void;
}
