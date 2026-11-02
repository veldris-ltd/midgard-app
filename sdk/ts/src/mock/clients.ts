/* @midgard/sdk/mock · Clients product slice: the in-memory `ClientMock` and its
 * deterministic fixtures, behind the `Clients` gateway interface in
 * ../products/clients.ts. Reached only through `@midgard/sdk/mock`, which a console
 * loads by dynamic import in mock mode, so no http build bundles it.
 * [VLD-ANB-MIDGARD-0086] [AC-25.2] */

import type { MarkingLevel, PolicyContext } from '../types';
import type {
  ClientObject,
  ClientPendingOp,
  ClientSyncPhase,
  ClientSyncState,
  ClientKeyCustody,
  ClientTier,
  ClientGateway,
} from '../products/clients';

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
