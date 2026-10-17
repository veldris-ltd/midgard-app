/* Heimdall product slice — operational fusion (common operating picture).
 *
 * One product-scoped gateway interface (`HeimdallGateway`) plus an in-memory
 * mock (`HeimdallMock`) and its compact, deterministic fixtures. All symbols are
 * product-prefixed so they coexist with the other product slices under the
 * shared `@midgard/sdk` barrel.
 *
 * Shared governance types are reused from `../types` (Marking levels, the policy
 * context, and the ML-DSA-signed `AuditRecord` that carries decision lineage).
 * The human-gate error is the same class the Midgard fabric throws, so gate
 * flows read identically across products.
 *
 * The mock models the enclosure closely enough to exercise every screen: PQ
 * source-signature verification on feeds (ML-DSA / SLH-DSA), classification +
 * markings on entities/tracks, marking-filtered picture sync, human-gated action
 * workflows that emit signed audit ids, and a live decision-lineage stream. No
 * network, no persistence — swap for a real bridge behind this same interface. */

import type { AuditRecord, MarkingLevel, PolicyContext } from '../types';
import { HumanGateRequiredError } from '../mock/MockGateway';
import { mockGeoTracks, mockHistory } from './heimdall-mock-geo';

// ---- shared enums -----------------------------------------------------------

/** Live-state key shared by feeds, sites and workflows (maps 1:1 to StatusPill). */
export type HeimdallState = 'online' | 'secure' | 'healthy' | 'offline' | 'warn' | 'danger' | 'gated';

/** Track quality for a fused entity. */
export type HeimdallTrackState = 'stable' | 'anomalous';

/** Post-quantum source-signature scheme a feed authenticates each frame with. */
export type HeimdallSignatureScheme = 'ML-DSA' | 'SLH-DSA';

/** Picture layers the operator can toggle. `feeds` is the reference layout's sensor
 *  coverage overlay; the others are Heimdall's wire layers (air, sea, land, space,
 *  events, other) under the console's names. [FR-H-1] */
export type HeimdallLayerId = 'air' | 'maritime' | 'ground' | 'space' | 'events' | 'other' | 'feeds';

// ---- domain types (product-prefixed) ----------------------------------------

/** A live sensor/data feed fused into the common picture. `auth === false`
 *  means the PQ source-signature check failed and the feed is quarantined. */
export interface HeimdallFeed {
  name: string;
  kind: string;
  rate: string;
  latency: string;
  weight: 'high' | 'medium' | 'low';
  /** PQ source-signature verified (per-frame ML-DSA / SLH-DSA). */
  auth: boolean;
  scheme: HeimdallSignatureScheme;
  status: HeimdallState;
  statusLabel: string;
}

/** A tracked, classified entity in the fusion catalogue (Entity Browser). */
export interface HeimdallEntity {
  id: string;
  /** Classification label, e.g. AIRCRAFT / VESSEL / GROUND UNIT / UNKNOWN. */
  cls: string;
  /** Classification confidence, 0–100. */
  conf: number;
  /** Number of corroborating feeds. */
  feeds: number;
  track: HeimdallTrackState;
  firstSeen: string;
  marking: MarkingLevel;
  compartment: string | null;
  status: HeimdallState;
  statusLabel: string;
}

/** A ranked, cited, advisory option for improving/handling a track (AI is
 *  advisory only — never executes; a human authorises via a workflow). */
export interface HeimdallOption {
  rank: number;
  title: string;
  why: string;
  cites: string[];
}

/** A positioned track on the operating-picture canvas (blueprint-grid region).
 *  Coordinates are percentages within the picture region; `hdg` in degrees. */
export interface HeimdallTrack {
  id: string;
  cls: string;
  /** Classification confidence, 0–1. */
  conf: number;
  /** Horizontal position, 0–100 (% of picture region). */
  x: number;
  /** Vertical position, 0–100 (% of picture region). */
  y: number;
  /** Shown geographic position (dead-reckoned where the detail says it is estimated;
   *  the reported one is in `detail`). Live and replayed tracks only. [FR-P3-3] */
  lat?: number;
  lon?: number;
  layer: HeimdallLayerId;
  track: HeimdallTrackState;
  /** Heading in degrees (0 = north). */
  hdg: number;
  /** Speed in knots. */
  spd: number;
  alt: string;
  feeds: number;
  /** Ranked advisory options attached to a focused/anomalous track. */
  options?: HeimdallOption[];
  /** Altitude in feet, when the feed reports it (live tracks only). */
  altFt?: number;
  /** Reported positions seen so far, oldest first, in the same 0–100 % space (live only). */
  trail?: { x: number; y: number }[];
  /** What the feed reported for this track (live only). */
  detail?: HeimdallTrackDetail;
  /** Reported positions seen so far, oldest first, geographic (live only): the
   *  client-side trail, used when the gateway has no history to serve. [FR-P3-3] */
  geoTrail?: HeimdallHistoryPoint[];
}

/** The reported attributes behind a live track. Times are unix seconds. */
export interface HeimdallTrackDetail {
  /** Heimdall's entity key (the ICAO 24-bit address for ADS-B). */
  key: string;
  /** Heimdall's entity kind, e.g. air-track, air-balloon, space-satellite,
   *  event-earthquake, event-flood. [FR-P3-3] */
  kind: string;
  /** A display name where the kind has one (satellite name, flood area, quake place). */
  name?: string;
  callsign?: string;
  squawk?: string;
  category?: string;
  onGround: boolean;
  lat: number;
  lon: number;
  /** Metres above mean sea level where known (negative: an earthquake's depth). */
  altM?: number;
  /** Earthquake magnitude. */
  magnitude?: number;
  /** Flood warning severity, 1 (most severe) to 4. */
  severityLevel?: number;
  /** A crewed space station (highlighted on the globe). */
  station?: boolean;
  /** A ship's AIS type in words (cargo, tanker, passenger…). */
  shipType?: string;
  /** An airport's METAR flight category: VFR, MVFR, IFR or LIFR. */
  flightCategory?: string;
  /** An airport's runways, each [lat, lon, lat, lon, width in metres] (centre line ends). */
  runways?: [number, number, number, number, number][];
  /** Kind-specific reported attributes for the inspector, in display order. */
  facts?: { label: string; value: string }[];
  /** Classification label and the rule rationale, when a rule matched. */
  label?: string;
  rationale?: string;
  sources: string[];
  observations: number;
  firstSeen: number;
  lastSeen: number;
  /** Seconds the shown position is moved forward from the last report (0 = reported). */
  estimatedSecs: number;
}

/** A lat/lon box the canvas maps onto 0–100 %. */
export interface HeimdallRegion {
  west: number;
  east: number;
  south: number;
  north: number;
}

/** A toggleable picture layer. */
export interface HeimdallLayer {
  id: HeimdallLayerId;
  label: string;
  active: boolean;
}

/** A human-gated action workflow: AI ranks options, a named human authorises,
 *  the authorisation + basis + outcome become one signed decision record. */
export interface HeimdallWorkflow {
  id: string;
  title: string;
  stage: string;
  owner: string;
  /** Number of cited options that were generated. */
  opts: number;
  age: string;
  status: HeimdallState;
  statusLabel: string;
  /** Consequential actions require a human gate before execution. */
  humanGated: boolean;
}

/** A signed, replayable decision record (from evidence to outcome). */
export interface HeimdallDecision {
  id: string;
  action: string;
  authoriser: string;
  basis: string;
  outcome: string;
  when: string;
  signature: string;
}

/** A site the common picture replicates to, marking-filtered per clearance. */
export interface HeimdallSyncSite {
  name: string;
  role: 'primary' | 'replica' | 'air-gap';
  lag: string;
  entities: string;
  markings: string;
  status: HeimdallState;
  statusLabel: string;
}

/** The fused common operating picture: toggleable layers, positioned tracks and
 *  the classified entity catalogue behind them. */
export interface HeimdallPicture {
  layers: HeimdallLayer[];
  tracks: HeimdallTrack[];
  entities: HeimdallEntity[];
  /** Set when positions are geographic: the box the canvas spans (linear in lat/lon).
   *  Absent for the reference layout, whose positions are not to map scale. */
  region?: HeimdallRegion;
  /** When the picture was last read from the gateway (unix ms). */
  fetchedAt?: number;
  /** Every positioned entity worldwide, whatever `region` is (the 3D globe draws these;
   *  `tracks` keeps only those inside the region). Same objects and ids as `tracks`
   *  where both hold an entity; x/y may fall outside 0–100. [FR-P3-3] */
  geo?: HeimdallTrack[];
}

// ---- history (replay) -------------------------------------------------------

/** One recorded position: [unix s, lat, lon, metres AMSL or null]. */
export type HeimdallHistoryPoint = [number, number, number, number | null];

/** The recorded path of one entity, points sorted by time. `id` is the entity key
 *  (`HeimdallTrackDetail.key`), not the display id. */
export interface HeimdallHistoryTrack {
  id: string;
  kind: string;
  layer: HeimdallLayerId;
  points: HeimdallHistoryPoint[];
}

/** Recorded positions over a time window (`GET /heimdall/history`). */
export interface HeimdallHistory {
  from: number;
  to: number;
  /** The server returned fewer points than it holds (raise `step` or narrow the window). */
  truncated: boolean;
  tracks: HeimdallHistoryTrack[];
}

export interface HeimdallHistoryQuery {
  /** Window start and end, unix seconds. */
  from: number;
  to: number;
  layer?: HeimdallLayerId;
  /** One entity key. */
  entity?: string;
  /** Down-sample to at most one point per `step` seconds per track (0 = every point). */
  step?: number;
}

/** The gateway has no history to serve (not deployed yet, or its store is down). */
export class HeimdallHistoryUnavailableError extends Error {
  constructor(message = 'history unavailable') {
    super(message);
    this.name = 'HeimdallHistoryUnavailableError';
  }
}

// ---- gateway seam -----------------------------------------------------------

/**
 * The single transport seam for the Heimdall console. Every screen talks to this
 * interface — never a concrete client — so the picture source can be swapped
 * (mock today, an HTTP/gRPC-web bridge over the enclosure's PQ-mTLS crates
 * tomorrow) without touching UI code.
 *
 * The key flow is `authoriseWorkflow`: a consequential action that is
 * human-gated. Called without a gate token for a gated workflow it throws
 * `HumanGateRequiredError` (the same class the Midgard fabric throws); with a
 * token it emits a PQ-signed `AuditRecord` and returns its id.
 */
export interface HeimdallGateway {
  /** The active policy context (subject/purpose/clearance/roles) for this session. */
  policy(): PolicyContext;

  // Picture ------------------------------------------------------------------
  /** The fused common operating picture (layers + positioned tracks + entities). */
  getPicture(): Promise<HeimdallPicture>;
  /** Registered feeds with PQ source-signature verification state. */
  listFeeds(): Promise<HeimdallFeed[]>;
  /** The classified entity catalogue. */
  listEntities(): Promise<HeimdallEntity[]>;
  /** Sites the picture replicates to (marking-filtered per site clearance). */
  listSyncSites(): Promise<HeimdallSyncSite[]>;
  /** Recorded positions over a window, for replay and trails. Rejects with
   *  `HeimdallHistoryUnavailableError` when the gateway has none. [FR-P3-7] */
  getHistory(q: HeimdallHistoryQuery): Promise<HeimdallHistory>;

  // Decide -------------------------------------------------------------------
  /** Human-gated action workflows (queue / approvals / execution). */
  listWorkflows(): Promise<HeimdallWorkflow[]>;
  /** Authorise a workflow. Gated workflows require a `humanGateToken`; the mock
   *  rejects gated calls without one. Returns a PQ-signed audit id. */
  authoriseWorkflow(id: string, humanGateToken?: string): Promise<{ auditId: string; applied: boolean }>;
  /** Signed, replayable decision records. */
  listDecisions(): Promise<HeimdallDecision[]>;

  // Lineage ------------------------------------------------------------------
  /** Snapshot of the most recent decision-lineage records (newest first). */
  getAudit(limit?: number): Promise<AuditRecord[]>;
  /** Subscribe to the live decision lineage; returns an unsubscribe function. */
  auditStream(onRecord: (record: AuditRecord) => void): () => void;
}

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
  { id: 'space', label: 'Space', active: true },
  { id: 'events', label: 'Events', active: true },
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
      // The globe's worldwide entities move on fixed, deterministic paths in time, so
      // the picture and the history agree. [FR-P3-3]
      geo: mockGeoTracks(TRACKS, Date.now() / 1000),
    });
  }

  /** History synthesised from the same paths the picture is drawn from. [FR-P3-7] */
  getHistory(q: HeimdallHistoryQuery): Promise<HeimdallHistory> {
    return this.wait(mockHistory(TRACKS, q, Date.now() / 1000));
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
