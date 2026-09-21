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

// ---- fixtures (deterministic; no Date.now()/random at module load) ---------

const DEFAULT_POLICY: PolicyContext = {
  subject: 'a.holt',
  purpose: 'field-operations',
  clearance: 'confidential',
  roles: ['operator', 'field-analyst'],
};

/** Marking ordering for clearance-scoped visibility (higher = more sensitive). */
const MARKING_RANK: Record<MarkingLevel, number> = {
  open: 0,
  internal: 1,
  restricted: 2,
  confidential: 3,
};

const OBJECTS: ClientObject[] = [
  {
    id: 'AC-2214',
    type: 'Aircraft',
    title: 'AC-2214 · Airbus A320',
    marking: 'confidential',
    fields: {
      tail_id: 'AC-2214',
      callsign: 'KRY214',
      status: 'diverted',
      position: '53.42 N · 6.27 W · FL120',
      operator: 'Kryo Air',
      note: 'unscheduled check · Dublin',
    },
  },
  {
    id: 'ME-88121',
    type: 'Maintenance Event',
    title: 'ME-88121 · Unscheduled check',
    marking: 'restricted',
    fields: {
      aircraft: 'AC-2214',
      severity: 'medium',
      status: 'open',
      opened: '14:02',
      assignee: 'unassigned',
    },
  },
  {
    id: 'RT-0471',
    type: 'Route',
    title: 'RT-0471 · CDG → DUB',
    marking: 'internal',
    fields: {
      origin: 'CDG',
      destination: 'DUB',
      status: 'active',
      distance_nm: '425',
    },
  },
  {
    id: 'CR-1180',
    type: 'Crew',
    title: 'CR-1180 · Duty roster',
    marking: 'internal',
    fields: {
      captain: 'M. Iyer',
      first_officer: 'L. Novak',
      status: 'on-duty',
      clearance: 'internal',
    },
  },
  {
    id: 'SF-North',
    type: 'Sensor Feed',
    title: 'SF-North · Fused picture',
    marking: 'confidential',
    fields: {
      region: 'OPS-N',
      tracks: '3',
      freshness_s: '184',
      status: 'cached',
    },
  },
  {
    id: 'NOTAM-9',
    type: 'Notice',
    title: 'NOTAM-9 · Airspace advisory',
    marking: 'open',
    fields: {
      region: 'EGTT',
      status: 'active',
      until: '18:00Z',
    },
  },
];

// ---- mock -------------------------------------------------------------------

export interface ClientMockOptions {
  /** Simulated round-trip latency in ms (default 140). */
  latencyMs?: number;
  policy?: PolicyContext;
  /** Start offline (default false). */
  offline?: boolean;
}

/**
 * In-memory implementation of `ClientGateway`.
 *
 * Deterministic (no `Date.now()`/`Math.random()` at module load): the device
 * clock advances from a fixed seed and op ids increment from a fixed base, so
 * every shell renders reproducibly. Models the client core closely enough to
 * exercise the full offline-first flow — local edits queue, the queue grows
 * offline, and a reconnect drains it with a conflict-free merge.
 */
export class ClientMock implements ClientGateway {
  private readonly latency: number;
  private readonly ctx: PolicyContext;
  private readonly cache: Map<string, ClientObject>;
  private queue: ClientPendingOp[] = [];
  private offline: boolean;
  private opSeq = 0;
  private clock = 14 * 3600 + 32 * 60 + 8; // seconds since midnight, seeds "14:32:08"
  private lastSync = '14:31:44';

  constructor(opts: ClientMockOptions = {}) {
    this.latency = opts.latencyMs ?? 140;
    this.ctx = opts.policy ?? DEFAULT_POLICY;
    this.offline = opts.offline ?? false;
    this.cache = new Map(OBJECTS.map((o) => [o.id, { ...o, fields: { ...o.fields } }]));
  }

  policy(): PolicyContext {
    return this.ctx;
  }

  // ---- helpers -------------------------------------------------------------

  private wait<T>(value: T): Promise<T> {
    return new Promise((resolve) => setTimeout(() => resolve(value), this.latency));
  }

  private stamp(): string {
    this.clock = (this.clock + 1 + Math.floor(this.latency / 100)) % 86400;
    const h = Math.floor(this.clock / 3600);
    const m = Math.floor((this.clock % 3600) / 60);
    const s = this.clock % 60;
    const p = (n: number) => String(n).padStart(2, '0');
    return `${p(h)}:${p(m)}:${p(s)}`;
  }

  private visible(): ClientObject[] {
    const ceiling = MARKING_RANK[this.ctx.clearance];
    return OBJECTS.filter((o) => MARKING_RANK[o.marking] <= ceiling).map((o) => {
      const live = this.cache.get(o.id) ?? o;
      return { ...live, fields: { ...live.fields } };
    });
  }

  private clone(o: ClientObject): ClientObject {
    return { ...o, fields: { ...o.fields } };
  }

  private state(phase: ClientSyncPhase): ClientSyncState {
    return { phase, pending: this.queue.length, lastSync: this.lastSync };
  }

  // ---- reads ---------------------------------------------------------------

  visibleObjects(): Promise<ClientObject[]> {
    return this.wait(this.visible());
  }

  open(id: string): Promise<ClientObject> {
    const found = this.cache.get(id);
    if (!found || MARKING_RANK[found.marking] > MARKING_RANK[this.ctx.clearance]) {
      return Promise.reject(new Error(`Object "${id}" not visible at clearance ${this.ctx.clearance}.`));
    }
    return this.wait(this.clone(found));
  }

  readField(id: string, field: string): Promise<string> {
    const found = this.cache.get(id);
    return this.wait(found?.fields[field] ?? '');
  }

  // ---- offline-first writes ------------------------------------------------

  editField(id: string, field: string, value: string): Promise<{ queued: ClientPendingOp }> {
    // Local-first: the edit lands in the device cache immediately, then a
    // conflict-free op is appended to the outbound queue. Never blocks on net.
    const obj = this.cache.get(id);
    if (obj) obj.fields[field] = value;
    this.opSeq += 1;
    const op: ClientPendingOp = {
      id: `OP-${1200 + this.opSeq}`,
      objectId: id,
      field,
      value,
      ts: this.stamp(),
    };
    this.queue = [...this.queue, op];
    return this.wait({ queued: { ...op } });
  }

  pendingOps(): Promise<ClientPendingOp[]> {
    return this.wait(this.queue.map((o) => ({ ...o })));
  }

  sync(): Promise<{ drained: number; merged: number; state: ClientSyncState }> {
    if (this.offline) {
      // Sync is blocked while offline; the queue keeps its ops.
      return this.wait({ drained: 0, merged: 0, state: this.state('offline') });
    }
    const drained = this.queue.length;
    this.queue = [];
    this.lastSync = this.stamp();
    // CRDT merge is conflict-free: everything drained merges cleanly.
    return this.wait({ drained, merged: drained, state: this.state('synced') });
  }

  syncState(): Promise<ClientSyncState> {
    if (this.offline) return this.wait(this.state('offline'));
    if (this.queue.length > 0) return this.wait(this.state('syncing'));
    return this.wait(this.state('synced'));
  }

  // ---- posture -------------------------------------------------------------

  keyCustody(): Promise<ClientKeyCustody> {
    return this.wait({
      store: 'keychain',
      sealed: true,
      cipher: 'AES-256-GCM',
      dataKey: 'ML-KEM-1024',
      floor: 'hardware · non-exportable',
    });
  }

  inferenceTier(): Promise<{ tier: ClientTier; reason: string }> {
    if (this.offline) {
      return this.wait({
        tier: 'on-device',
        reason: 'offline · enclosure unreachable · local model only · data stays on device',
      });
    }
    return this.wait({
      tier: 'hybrid',
      reason: 'local draft · enclosure verifies + cites live lineage · tier is automatic, override is policy-bound',
    });
  }

  // ---- connectivity --------------------------------------------------------

  setOffline(offline: boolean): void {
    this.offline = offline;
  }
}
