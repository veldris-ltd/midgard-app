/* @midgard/sdk/mock · Heimdall product slice: the in-memory `HeimdallMock` and its
 * deterministic fixtures, behind the `Heimdall` gateway interface in
 * ../products/heimdall.ts. Reached only through `@midgard/sdk/mock`, which a console
 * loads by dynamic import in mock mode, so no http build bundles it.
 * [VLD-ANB-MIDGARD-0086] [AC-25.2] */

import type { AuditRecord, PolicyContext } from '../types';
import { HumanGateRequiredError } from '../errors';
import type {
  HeimdallFeed,
  HeimdallEntity,
  HeimdallOption,
  HeimdallTrack,
  HeimdallLayer,
  HeimdallWorkflow,
  HeimdallDecision,
  HeimdallSyncSite,
  HeimdallPicture,
  HeimdallGateway,
} from '../products/heimdall';

// ---- fixtures (compact, deterministic) --------------------------------------

const FEEDS: HeimdallFeed[] = [
  { name: 'NORTH-2', kind: 'radar', rate: '4 Hz', latency: '0.2s', weight: 'high', auth: true, scheme: 'ML-DSA', status: 'online', statusLabel: 'Live' },
  { name: 'MAR-RAD-1', kind: 'maritime radar', rate: '1 Hz', latency: '0.4s', weight: 'high', auth: true, scheme: 'ML-DSA', status: 'online', statusLabel: 'Live' },
  { name: 'ADS-B-EU', kind: 'transponder', rate: 'stream', latency: '0.1s', weight: 'medium', auth: true, scheme: 'SLH-DSA', status: 'online', statusLabel: 'Live' },
  { name: 'SAT-IR-7', kind: 'satellite IR', rate: '1/30s', latency: '22s', weight: 'medium', auth: true, scheme: 'ML-DSA', status: 'healthy', statusLabel: 'Nominal' },
  { name: 'GND-OBS-4', kind: 'ground observer', rate: 'manual', latency: '2m', weight: 'low', auth: true, scheme: 'SLH-DSA', status: 'warn', statusLabel: 'Stale 8m' },
  { name: 'LEGACY-L16', kind: 'datalink bridge', rate: '1 Hz', latency: '0.9s', weight: 'medium', auth: false, scheme: 'ML-DSA', status: 'danger', statusLabel: 'Auth failed' },
];

const ENTITIES: HeimdallEntity[] = [
  { id: 'E-0871', cls: 'UNKNOWN', conf: 54, feeds: 2, track: 'anomalous', firstSeen: '13:41', marking: 'confidential', compartment: 'OPS-N', status: 'warn', statusLabel: 'Anomalous' },
  { id: 'E-2214', cls: 'AIRCRAFT', conf: 97, feeds: 4, track: 'stable', firstSeen: '06:02', marking: 'confidential', compartment: 'OPS-N', status: 'online', statusLabel: 'Tracked' },
  { id: 'E-2231', cls: 'AIRCRAFT', conf: 91, feeds: 3, track: 'stable', firstSeen: '07:15', marking: 'confidential', compartment: 'OPS-N', status: 'online', statusLabel: 'Tracked' },
  { id: 'E-0442', cls: 'VESSEL', conf: 88, feeds: 2, track: 'stable', firstSeen: '04:48', marking: 'restricted', compartment: null, status: 'online', statusLabel: 'Tracked' },
  { id: 'E-1108', cls: 'GROUND UNIT', conf: 93, feeds: 3, track: 'stable', firstSeen: '09:20', marking: 'confidential', compartment: 'OPS-N', status: 'online', statusLabel: 'Tracked' },
];

const OPTIONS: HeimdallOption[] = [
  { rank: 1, title: 'Re-task sensor feed NORTH-2 to track E-0871', why: 'Highest expected classification gain (+0.31 confidence) within 12 min.', cites: ['SensorFeed/NORTH-2', 'Entity/E-0871'] },
  { rank: 2, title: 'Cross-cue maritime radar sweep', why: 'Covers ambiguity if E-0871 descends below FL100.', cites: ['Feed/MAR-RAD-1'] },
  { rank: 3, title: 'Hold, continue passive fusion', why: 'No kinetic urgency; anomaly may resolve within current track window.', cites: ['Track/E-0871·history'] },
];

const TRACKS: HeimdallTrack[] = [
  { id: 'E-2214', cls: 'AIRCRAFT', conf: 0.97, x: 22, y: 30, layer: 'air', track: 'stable', hdg: 78, spd: 460, alt: 'FL340', feeds: 4 },
  { id: 'E-2231', cls: 'AIRCRAFT', conf: 0.91, x: 38, y: 18, layer: 'air', track: 'stable', hdg: 132, spd: 430, alt: 'FL360', feeds: 3 },
  { id: 'E-0442', cls: 'VESSEL', conf: 0.88, x: 62, y: 66, layer: 'maritime', track: 'stable', hdg: 221, spd: 14, alt: 'SURFACE', feeds: 2 },
  { id: 'E-0871', cls: 'UNKNOWN', conf: 0.54, x: 74, y: 34, layer: 'air', track: 'anomalous', hdg: 197, spd: 388, alt: 'FL212 ↓', feeds: 2, options: OPTIONS },
  { id: 'E-1108', cls: 'GROUND UNIT', conf: 0.93, x: 45, y: 74, layer: 'ground', track: 'stable', hdg: 12, spd: 3, alt: 'GND', feeds: 3 },
];

const LAYERS: HeimdallLayer[] = [
  { id: 'air', label: 'Air', active: true },
  { id: 'maritime', label: 'Maritime', active: true },
  { id: 'ground', label: 'Ground', active: true },
  { id: 'feeds', label: 'Feed coverage', active: false },
];

const SYNC_SITES: HeimdallSyncSite[] = [
  { name: 'SITE-ALPHA (this)', role: 'primary', lag: '—', entities: '1,204', markings: 'C // OPS-N', status: 'online', statusLabel: 'Authoring' },
  { name: 'SITE-BRAVO', role: 'replica', lag: '0.8s', entities: '1,204', markings: 'C // OPS-N', status: 'secure', statusLabel: 'In sync' },
  { name: 'SITE-CHARLIE', role: 'replica', lag: '1.2s', entities: '988', markings: 'RESTRICTED', status: 'secure', statusLabel: 'Filtered sync' },
  { name: 'SITE-DELTA-AG', role: 'air-gap', lag: 'courier', entities: '941', markings: 'RESTRICTED', status: 'offline', statusLabel: 'Snapshot 2d' },
];

const WORKFLOWS: HeimdallWorkflow[] = [
  { id: 'W-0912', title: 'Re-task sensor feed NORTH-2 → E-0871', stage: 'awaiting authorisation', owner: 'k.osei', opts: 3, age: '4m', status: 'gated', statusLabel: 'Human gate', humanGated: true },
  { id: 'W-0913', title: 'Cross-cue maritime radar sweep on E-0871', stage: 'awaiting authorisation', owner: 'k.osei', opts: 2, age: '2m', status: 'gated', statusLabel: 'Human gate', humanGated: true },
  { id: 'W-0911', title: 'Ground aircraft AC-1180', stage: 'executing', owner: 'j.benjamin', opts: 2, age: '18m', status: 'online', statusLabel: 'Executing', humanGated: false },
  { id: 'W-0910', title: 'Close maintenance event ME-3391', stage: 'complete', owner: 'k.osei', opts: 1, age: '2h', status: 'secure', statusLabel: 'Complete', humanGated: false },
];

const DECISIONS: HeimdallDecision[] = [
  { id: 'D-4472', action: 'Re-task NORTH-2 → E-0871', authoriser: 'k.osei', basis: '3 options · opt 01', outcome: 'conf 54→86%', when: 'today 14:39', signature: 'ML-DSA ✓' },
  { id: 'D-4468', action: 'Ground aircraft AC-1180', authoriser: 'k.osei', basis: '1 option · manual', outcome: 'grounded · ME opened', when: 'today 12:11', signature: 'ML-DSA ✓' },
  { id: 'D-4461', action: 'Cross-cue maritime sweep', authoriser: 'j.benjamin', basis: '2 options · opt 02', outcome: 'no contact · closed', when: 'yesterday', signature: 'ML-DSA ✓' },
];

const AUDIT_SEED: AuditRecord[] = [
  { id: 'D-4472', actor: 'k.osei', action: 'Authorise · Re-task NORTH-2 → E-0871', resource: 'workflow/W-0912', pqSignature: 'ML-DSA ✓', ts: '14:39:02' },
  { id: 'D-4471', actor: 'svc:heimdall/fusion', action: 'Write · entity projection', resource: 'entity/E-0871', pqSignature: 'ML-DSA ✓', ts: '14:38:41' },
  { id: 'D-4470', actor: 'svc:heimdall/sync', action: 'Replicate · delta stream', resource: 'site/SITE-BRAVO', pqSignature: 'ML-DSA ✓', ts: '14:38:12' },
  { id: 'D-4469', actor: 'agent:mimir/fusion-01', action: 'Rank · advisory options', resource: 'entity/E-0871', pqSignature: 'ML-DSA ✓', ts: '14:37:55' },
  { id: 'D-4468', actor: 'k.osei', action: 'Authorise · Ground aircraft AC-1180', resource: 'workflow/W-0910', pqSignature: 'ML-DSA ✓', ts: '12:11:03' },
];

/** Rotating pool used to synthesise "live" decision-lineage records. */
const AUDIT_LIVE_POOL: Omit<AuditRecord, 'id' | 'ts'>[] = [
  { actor: 'svc:heimdall/fusion', action: 'Write · track update', resource: 'track/E-2231', pqSignature: 'ML-DSA ✓' },
  { actor: 'agent:mimir/fusion-01', action: 'Classify · rules + inference', resource: 'entity/E-0442', pqSignature: 'ML-DSA ✓' },
  { actor: 'svc:heimdall/feed', action: 'Verify · PQ source signature', resource: 'feed/NORTH-2', pqSignature: 'ML-DSA ✓' },
  { actor: 'svc:heimdall/sync', action: 'Replicate · marking-filtered', resource: 'site/SITE-CHARLIE', pqSignature: 'ML-DSA ✓' },
  { actor: 'k.osei', action: 'Query · decision lineage', resource: 'lineage/*', pqSignature: 'ML-DSA ✓' },
];

const DEFAULT_POLICY: PolicyContext = {
  subject: 'k.osei',
  purpose: 'operations',
  clearance: 'confidential',
  roles: ['operator', 'watch-officer'],
};

export interface HeimdallMockOptions {
  /** Simulated round-trip latency in ms (default 180). */
  latencyMs?: number;
  policy?: PolicyContext;
}

/**
 * In-memory implementation of the `HeimdallGateway` seam. Deterministic — no
 * `Date.now()` / random at construction — so the console renders reproducibly.
 */
export class HeimdallMock implements HeimdallGateway {
  private readonly latency: number;
  private readonly ctx: PolicyContext;
  private audit: AuditRecord[];
  private seq = 4472; // last seeded decision id (D-4472)
  private poolIdx = 0;
  private clock = 14 * 3600 + 39 * 60 + 2; // seconds since midnight, seeds "14:39:02"
  private listeners = new Set<(r: AuditRecord) => void>();
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(opts: HeimdallMockOptions = {}) {
    this.latency = opts.latencyMs ?? 180;
    this.ctx = opts.policy ?? DEFAULT_POLICY;
    this.audit = AUDIT_SEED.map((r) => ({ ...r }));
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

  private nextId(): string {
    this.seq += 1;
    return `D-${this.seq}`;
  }

  private emit(rec: Omit<AuditRecord, 'id' | 'ts'>): AuditRecord {
    const record: AuditRecord = { ...rec, id: this.nextId(), ts: this.stamp() };
    this.audit = [record, ...this.audit].slice(0, 200);
    this.listeners.forEach((fn) => fn(record));
    return record;
  }

  // ---- picture -------------------------------------------------------------

  getPicture(): Promise<HeimdallPicture> {
    return this.wait({
      layers: LAYERS.map((l) => ({ ...l })),
      tracks: TRACKS.map((t) => ({ ...t, options: t.options?.map((o) => ({ ...o, cites: [...o.cites] })) })),
      entities: ENTITIES.map((e) => ({ ...e })),
    });
  }

  listFeeds(): Promise<HeimdallFeed[]> {
    return this.wait(FEEDS.map((f) => ({ ...f })));
  }

  listEntities(): Promise<HeimdallEntity[]> {
    return this.wait(ENTITIES.map((e) => ({ ...e })));
  }

  listSyncSites(): Promise<HeimdallSyncSite[]> {
    return this.wait(SYNC_SITES.map((s) => ({ ...s })));
  }

  // ---- decide --------------------------------------------------------------

  listWorkflows(): Promise<HeimdallWorkflow[]> {
    return this.wait(WORKFLOWS.map((w) => ({ ...w })));
  }

  async authoriseWorkflow(id: string, humanGateToken?: string): Promise<{ auditId: string; applied: boolean }> {
    const wf = WORKFLOWS.find((w) => w.id === id);
    if (wf?.humanGated && !humanGateToken) {
      throw new HumanGateRequiredError(wf.title);
    }
    const audit = this.emit({
      actor: this.ctx.subject,
      action: `Authorise · ${wf?.title ?? id}`,
      resource: `workflow/${id}`,
      pqSignature: 'ML-DSA ✓',
    });
    return this.wait({ auditId: audit.id, applied: true });
  }

  listDecisions(): Promise<HeimdallDecision[]> {
    return this.wait(DECISIONS.map((d) => ({ ...d })));
  }

  // ---- lineage -------------------------------------------------------------

  getAudit(limit = 50): Promise<AuditRecord[]> {
    return this.wait(this.audit.slice(0, limit).map((r) => ({ ...r })));
  }

  auditStream(onRecord: (record: AuditRecord) => void): () => void {
    this.listeners.add(onRecord);
    if (!this.timer) {
      this.timer = setInterval(() => {
        const next = AUDIT_LIVE_POOL[this.poolIdx % AUDIT_LIVE_POOL.length];
        this.poolIdx += 1;
        this.emit(next);
      }, 4200);
    }
    return () => {
      this.listeners.delete(onRecord);
      if (this.listeners.size === 0 && this.timer) {
        clearInterval(this.timer);
        this.timer = null;
      }
    };
  }
}
