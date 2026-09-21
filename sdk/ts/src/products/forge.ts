/* @midgard/sdk · Forge product slice.
 *
 * Forge is the data-operations plane: typed source connectors (inbound pull
 * only), versioned datasets with sealed lineage, low-code + pro-code transforms,
 * expectations/drift health gates, schedules, and the projection layer that maps
 * curated datasets into Midgard fabric object/link types.
 *
 * One interface (`ForgeGateway`) + one in-memory implementation (`ForgeMock`) +
 * compact deterministic fixtures. All symbols are `Forge`-prefixed so they can
 * live alongside the other product slices in the shared `@midgard/sdk` barrel.
 * Shared governance primitives are imported from `../types`. */

import type { AuditRecord, MarkingLevel, PolicyContext } from '../types';

// ---- pipelines (flagship builder / DAG) ------------------------------------

export type ForgeNodeKind = 'source' | 'transform' | 'dataset' | 'projection';
export type ForgeNodeStatus = 'healthy' | 'warn' | 'online';

/** A node on the pipeline DAG. `x`,`y` are canvas grid units (column,row). */
export interface ForgePipelineNode {
  id: string;
  kind: ForgeNodeKind;
  label: string;
  /** DS Icon name. */
  icon: string;
  x: number;
  y: number;
  status: ForgeNodeStatus;
  /** Mono sub-line, e.g. "JDBC · pull 5m". */
  meta: string;
}

export interface ForgePipelineEdge {
  from: string;
  to: string;
}

/** A sealed build in the pipeline's version history. */
export interface ForgePipelineVersion {
  version: string;
  branch: string;
  ts: string;
  status: 'OK' | 'FAIL';
}

/** A Forge pipeline: a DAG of sources → transforms → dataset → projection,
 *  versioned and reproducibly rebuildable. */
export interface ForgePipeline {
  id: string;
  name: string;
  /** Mono context suffix for the shell, e.g. "FLIGHT_TRACKS_EU". */
  context: string;
  branch: string;
  currentVersion: string;
  /** Pass ratio for the gating expectations, e.g. "12/12". */
  expectations: string;
  marking: MarkingLevel;
  compartment: string | null;
  nodes: ForgePipelineNode[];
  edges: ForgePipelineEdge[];
  versions: ForgePipelineVersion[];
  /** Advisory, local-inference build hint (preview-before-commit). `[FR-F-7]` */
  suggestion: string;
}

/** One staged step returned by a build. */
export interface ForgeBuildStep {
  /** Left-aligned stage keyword, e.g. "resolve". */
  stage: string;
  /** Detail line, e.g. "sources 2/2 · PQ-mTLS OK". */
  detail: string;
  status: 'ok' | 'warn';
}

/** The staged result of `runPipeline` — the build the operator watches run. */
export interface ForgeBuildResult {
  runId: string;
  version: string;
  durationLabel: string;
  steps: ForgeBuildStep[];
}

// ---- connectors -------------------------------------------------------------

export type ForgeConnectorState = 'healthy' | 'online' | 'warn' | 'offline';

/** A typed source connector. Inbound pull only, PQ-mTLS, no egress. */
export interface ForgeConnector {
  id: string;
  name: string;
  /** Transport class, e.g. "JDBC", "Stream", "SFTP drop". */
  kind: string;
  /** Cadence / direction, e.g. "pull · 5m". */
  cadence: string;
  datasetsFed: number;
  volume: string;
  state: ForgeConnectorState;
  stateLabel: string;
  transport: string;
  egress: string;
  schema: string;
  /** Non-null when the connector needs operator attention. */
  action: string | null;
}

// ---- datasets ---------------------------------------------------------------

export interface ForgeDatasetDiffLine {
  text: string;
  tone: 'add' | 'remove' | 'change';
}

export interface ForgeLineageHop {
  icon: string;
  label: string;
}

/** A versioned, sealed dataset with branch, diff, and lineage. */
export interface ForgeDataset {
  id: string;
  name: string;
  version: string;
  branch: string;
  rows: string;
  columns: number;
  marking: MarkingLevel;
  state: 'secure' | 'healthy' | 'warn';
  stateLabel: string;
  diff: ForgeDatasetDiffLine[];
  lineage: ForgeLineageHop[];
}

// ---- data health ------------------------------------------------------------

/** An expectation / drift monitor that gates projection. */
export interface ForgeHealthCheck {
  id: string;
  dataset: string;
  check: string;
  kind: string;
  passRate: string;
  /** Numeric pass rate (0–100) for meters. */
  passValue: number;
  trend: string;
  state: 'secure' | 'healthy' | 'warn';
  stateLabel: string;
  projectionGate: 'CLEAR' | 'BLOCKS';
  /** Advisory, local-inference diagnosis when drifting. */
  diagnosis: string | null;
}

// ---- schedules --------------------------------------------------------------

/** A batch or streaming pipeline schedule/trigger. */
export interface ForgeSchedule {
  id: string;
  pipeline: string;
  mode: 'batch' | 'stream';
  trigger: string;
  last: string;
  next: string;
  state: 'healthy' | 'online' | 'warn';
  stateLabel: string;
  backfill: string;
  concurrency: string;
  onFailure: string;
  /** Non-null when a run needs attention (e.g. retrying). */
  note: string | null;
}

// ---- transform IDE ----------------------------------------------------------

/** The pro-code IDE view model: files on the branch + the open file's source. */
export interface ForgeTransform {
  id: string;
  branch: string;
  nextVersion: string;
  filesCount: number;
  expectations: string;
  files: string[];
  activeFile: string;
  /** Source lines of the active file. */
  code: string[];
  /** [label, value] contract checks. */
  contractChecks: Array<[string, string]>;
}

export interface ForgeDiffHunk {
  op: '+' | '-' | '~';
  text: string;
}

/** A previewed transform result — sandboxed, nothing committed. */
export interface ForgeTransformDiff {
  summary: string;
  rowsIn: number;
  rowsOut: number;
  expectations: string;
  durationLabel: string;
  hunks: ForgeDiffHunk[];
}

export interface ForgePreviewResult {
  diff: ForgeTransformDiff;
}

export interface ForgeApplyResult {
  auditId: string;
  version: string;
  applied: boolean;
}

// ---- object projection ------------------------------------------------------

export interface ForgeProjectionMapping {
  id: string;
  column: string;
  to: string;
  kind: 'key' | 'property' | 'link' | 'struct' | 'governance';
  note: string;
}

/** A mapping of dataset columns onto Midgard fabric objects/links/markings. */
export interface ForgeProjection {
  id: string;
  name: string;
  source: string;
  target: string;
  objects: string;
  syncLag: string;
  columnsMapped: string;
  linkTypes: number;
  unmarkedRows: number;
  marking: MarkingLevel;
  compartment: string | null;
  mappings: ForgeProjectionMapping[];
}

// ---- gateway seam -----------------------------------------------------------

/** Error thrown when a state-mutating apply is attempted without a human-gate
 *  token. Mirrors the fabric's `HumanGateRequiredError`. `[NFR-AI-1]` */
export class ForgeHumanGateRequiredError extends Error {
  constructor(subject: string) {
    super(`Human gate required to commit "${subject}"`);
    this.name = 'ForgeHumanGateRequiredError';
  }
}

/**
 * The single transport seam for the Forge console.
 *
 * Every screen talks to this interface — never a concrete client — so the data
 * source can be swapped for a real browser-reachable bridge (a runnable Rust
 * service over the enclosure's PQ-mTLS crates) without touching UI code. Today
 * the only implementation is `ForgeMock`.
 */
export interface ForgeGateway {
  /** The active policy context for this session. */
  policy(): PolicyContext;

  // one primary read per screen ----------------------------------------------
  listPipelines(): Promise<ForgePipeline[]>;
  listConnectors(): Promise<ForgeConnector[]>;
  listDatasets(): Promise<ForgeDataset[]>;
  listHealthChecks(): Promise<ForgeHealthCheck[]>;
  listSchedules(): Promise<ForgeSchedule[]>;
  getTransform(): Promise<ForgeTransform>;
  listProjections(): Promise<ForgeProjection[]>;

  // key flows -----------------------------------------------------------------
  /** Run a build for a pipeline; returns the staged steps the operator watches. */
  runPipeline(id: string): Promise<ForgeBuildResult>;
  /** Preview a natural-language / SQL transform as a diff. Non-destructive;
   *  the result is applied only after a human authorises it. */
  previewTransform(sql: string): Promise<ForgePreviewResult>;
  /** Commit a previewed transform. Gated: throws `ForgeHumanGateRequiredError`
   *  without a `humanGateToken`. Emits an ML-DSA-signed audit record. */
  applyTransform(sql: string, humanGateToken?: string): Promise<ForgeApplyResult>;

  // governance ----------------------------------------------------------------
  getAudit(limit?: number): Promise<AuditRecord[]>;
  /** Subscribe to the live audit lineage; returns an unsubscribe function. */
  auditStream(onRecord: (record: AuditRecord) => void): () => void;
}

// ---- fixtures (deterministic; no Date.now()/random at module load) ---------

const PIPELINES: ForgePipeline[] = [
  {
    id: 'flight_tracks_eu',
    name: 'flight_tracks_eu',
    context: 'FLIGHT_TRACKS_EU',
    branch: 'main',
    currentVersion: 'v41',
    expectations: '12/12',
    marking: 'confidential',
    compartment: 'OPS-N',
    nodes: [
      { id: 'src1', kind: 'source', label: 'flights_raw', icon: 'database', x: 0, y: 0, status: 'healthy', meta: 'JDBC · pull 5m' },
      { id: 'src2', kind: 'source', label: 'sensor_stream', icon: 'radio-tower', x: 0, y: 1, status: 'healthy', meta: 'Stream · live' },
      { id: 't1', kind: 'transform', label: 'clean_flights', icon: 'workflow', x: 1, y: 0, status: 'healthy', meta: 'Low-code · 6 steps' },
      { id: 't2', kind: 'transform', label: 'fuse_positions', icon: 'code', x: 1, y: 1, status: 'warn', meta: 'Pro-code · Python' },
      { id: 'ds', kind: 'dataset', label: 'flight_tracks_eu', icon: 'table', x: 2, y: 0.5, status: 'healthy', meta: 'v41 · branch main' },
      { id: 'proj', kind: 'projection', label: 'Aircraft objects', icon: 'boxes', x: 3, y: 0.5, status: 'healthy', meta: '→ Midgard fabric' },
    ],
    edges: [
      { from: 'src1', to: 't1' },
      { from: 'src2', to: 't2' },
      { from: 't1', to: 'ds' },
      { from: 't2', to: 'ds' },
      { from: 'ds', to: 'proj' },
    ],
    versions: [
      { version: 'v41', branch: 'main', ts: '14:02', status: 'OK' },
      { version: 'v40', branch: 'main', ts: '12:40', status: 'OK' },
      { version: 'v39', branch: 'dev/fuse-fix', ts: '11:18', status: 'FAIL' },
    ],
    suggestion: 'Expectation alt_ft < 60000 fails on 0.4% of rows in fuse_positions. Suggest a bounds filter before the join.',
  },
  {
    id: 'sensor_fusion_live',
    name: 'sensor_fusion_live',
    context: 'SENSOR_FUSION_LIVE',
    branch: 'main',
    currentVersion: 'v12',
    expectations: '9/9',
    marking: 'restricted',
    compartment: null,
    nodes: [
      { id: 'ssrc', kind: 'source', label: 'sensor_stream', icon: 'radio-tower', x: 0, y: 0.5, status: 'online', meta: 'Stream · live' },
      { id: 'sfuse', kind: 'transform', label: 'fuse_positions', icon: 'workflow', x: 1, y: 0.5, status: 'healthy', meta: 'Low-code · 4 steps' },
      { id: 'sds', kind: 'dataset', label: 'sensor_feed_north', icon: 'table', x: 2, y: 0.5, status: 'healthy', meta: 'v12 · branch main' },
      { id: 'sproj', kind: 'projection', label: 'Sensor entities', icon: 'boxes', x: 3, y: 0.5, status: 'healthy', meta: '→ Midgard fabric' },
    ],
    edges: [
      { from: 'ssrc', to: 'sfuse' },
      { from: 'sfuse', to: 'sds' },
      { from: 'sds', to: 'sproj' },
    ],
    versions: [
      { version: 'v12', branch: 'main', ts: '13:59', status: 'OK' },
      { version: 'v11', branch: 'main', ts: '09:20', status: 'OK' },
    ],
    suggestion: 'Fusion lag stable at 0.4s over the last 4d. No action required.',
  },
  {
    id: 'maintenance_rollup',
    name: 'maintenance_rollup',
    context: 'MAINTENANCE_ROLLUP',
    branch: 'main',
    currentVersion: 'v8',
    expectations: '6/6',
    marking: 'internal',
    compartment: null,
    nodes: [
      { id: 'msrc', kind: 'source', label: 'maint_sftp', icon: 'database', x: 0, y: 0.5, status: 'warn', meta: 'SFTP · hourly' },
      { id: 'mroll', kind: 'transform', label: 'rollup_events', icon: 'workflow', x: 1, y: 0.5, status: 'healthy', meta: 'Low-code · 3 steps' },
      { id: 'mds', kind: 'dataset', label: 'maintenance_events', icon: 'table', x: 2, y: 0.5, status: 'warn', meta: 'v8 · dev/fuse-fix' },
    ],
    edges: [
      { from: 'msrc', to: 'mroll' },
      { from: 'mroll', to: 'mds' },
    ],
    versions: [
      { version: 'v8', branch: 'dev/fuse-fix', ts: '11:18', status: 'OK' },
      { version: 'v7', branch: 'main', ts: 'yesterday', status: 'OK' },
    ],
    suggestion: 'Integrity check on closed_by degrading 4d. Suggest a quarantine rule pending upstream fix.',
  },
];

const CONNECTORS: ForgeConnector[] = [
  { id: 'flights_jdbc', name: 'flights_jdbc', kind: 'JDBC', cadence: 'pull · 5m', datasetsFed: 4, volume: '2.1 GB/d', state: 'healthy', stateLabel: 'Healthy', transport: 'PQ-mTLS · X25519+ML-KEM-768', egress: 'none · air-gap compatible', schema: 'typed contract v3', action: null },
  { id: 'sensor_stream', name: 'sensor_stream', kind: 'Stream', cadence: 'pull · live', datasetsFed: 1, volume: '14 GB/d', state: 'online', stateLabel: 'Live', transport: 'PQ-mTLS · X25519+ML-KEM-768', egress: 'none · air-gap compatible', schema: 'typed contract v3', action: null },
  { id: 'maint_sftp', name: 'maint_sftp', kind: 'SFTP drop', cadence: 'pull · hourly', datasetsFed: 2, volume: '120 MB/d', state: 'healthy', stateLabel: 'Healthy', transport: 'PQ-mTLS · X25519+ML-KEM-768', egress: 'none · air-gap compatible', schema: 'typed contract v2', action: null },
  { id: 'legacy_soap', name: 'legacy_soap', kind: 'SOAP bridge', cadence: 'pull · daily', datasetsFed: 1, volume: '8 MB/d', state: 'warn', stateLabel: 'Cert expiring', transport: 'PQ-mTLS · X25519+ML-KEM-768', egress: 'none · air-gap compatible', schema: 'typed contract v1', action: 'Client certificate expires in 6 days. Rotation is zero-downtime.' },
  { id: 'weather_edge', name: 'weather_edge', kind: 'Edge cache', cadence: 'pull · 15m', datasetsFed: 1, volume: '340 MB/d', state: 'offline', stateLabel: 'Paused', transport: 'PQ-mTLS · X25519+ML-KEM-768', egress: 'none · air-gap compatible', schema: 'typed contract v2', action: null },
];

const DATASETS: ForgeDataset[] = [
  {
    id: 'flight_tracks_eu', name: 'flight_tracks_eu', version: 'v42', branch: 'main', rows: '4.2M', columns: 18,
    marking: 'confidential', state: 'secure', stateLabel: 'Sealed',
    diff: [
      { text: '+ 12,406 rows', tone: 'add' },
      { text: '− 88 rows (dedup)', tone: 'remove' },
      { text: '~ column alt_ft: bounds filter added', tone: 'change' },
    ],
    lineage: [
      { icon: 'database', label: 'flights_jdbc' },
      { icon: 'workflow', label: 'clean_flights' },
      { icon: 'table', label: 'flight_tracks_eu v42' },
      { icon: 'boxes', label: 'Aircraft objects' },
    ],
  },
  {
    id: 'sensor_feed_north', name: 'sensor_feed_north', version: 'v12', branch: 'main', rows: '96M', columns: 21,
    marking: 'restricted', state: 'healthy', stateLabel: 'Healthy',
    diff: [
      { text: '+ 4.1M rows', tone: 'add' },
      { text: '~ column freshness_s: SLO tightened', tone: 'change' },
    ],
    lineage: [
      { icon: 'radio-tower', label: 'sensor_stream' },
      { icon: 'workflow', label: 'fuse_positions' },
      { icon: 'table', label: 'sensor_feed_north v12' },
      { icon: 'boxes', label: 'Sensor entities' },
    ],
  },
  {
    id: 'maintenance_events', name: 'maintenance_events', version: 'v8', branch: 'dev/fuse-fix', rows: '48k', columns: 12,
    marking: 'internal', state: 'warn', stateLabel: 'Ahead of main',
    diff: [
      { text: '+ 312 rows', tone: 'add' },
      { text: '~ column closed_by: quarantine rule pending', tone: 'change' },
    ],
    lineage: [
      { icon: 'database', label: 'maint_sftp' },
      { icon: 'workflow', label: 'rollup_events' },
      { icon: 'table', label: 'maintenance_events v8' },
      { icon: 'boxes', label: 'Maintenance events' },
    ],
  },
  {
    id: 'crew_roster', name: 'crew_roster', version: 'v31', branch: 'main', rows: '3,051', columns: 17,
    marking: 'internal', state: 'secure', stateLabel: 'Sealed',
    diff: [
      { text: '+ 12 rows', tone: 'add' },
      { text: '~ column clearance: normalised', tone: 'change' },
    ],
    lineage: [
      { icon: 'database', label: 'hr_jdbc' },
      { icon: 'workflow', label: 'clean_roster' },
      { icon: 'table', label: 'crew_roster v31' },
      { icon: 'boxes', label: 'Crew objects' },
    ],
  },
];

const HEALTH_CHECKS: ForgeHealthCheck[] = [
  { id: 'hc-alt-bounds', dataset: 'flight_tracks_eu', check: 'alt_ft within [0, 60000]', kind: 'bounds', passRate: '99.6%', passValue: 99.6, trend: 'fixed in v42', state: 'secure', stateLabel: 'Passing', projectionGate: 'CLEAR', diagnosis: null },
  { id: 'hc-freshness', dataset: 'sensor_feed_north', check: 'freshness < 30s', kind: 'freshness', passRate: '100%', passValue: 100, trend: 'stable', state: 'healthy', stateLabel: 'Passing', projectionGate: 'CLEAR', diagnosis: null },
  { id: 'hc-closed-by', dataset: 'maintenance_events', check: 'closed_by not null when status=closed', kind: 'integrity', passRate: '97.1%', passValue: 97.1, trend: 'degrading 4d', state: 'warn', stateLabel: 'Drift', projectionGate: 'BLOCKS', diagnosis: 'Nulls correlate with imports from maint_sftp after 02:00. Likely upstream export truncation; suggest a quarantine rule pending source fix.' },
  { id: 'hc-crew-unique', dataset: 'crew_roster', check: 'unique(crew_id)', kind: 'uniqueness', passRate: '100%', passValue: 100, trend: 'stable', state: 'secure', stateLabel: 'Passing', projectionGate: 'CLEAR', diagnosis: null },
  { id: 'hc-volume', dataset: 'flight_tracks_eu', check: 'row volume ±15% of 7d mean', kind: 'volume', passRate: '91.2%', passValue: 91.2, trend: 'seasonal', state: 'warn', stateLabel: 'Watch', projectionGate: 'CLEAR', diagnosis: 'Volume dip tracks a scheduled maintenance window; within seasonal envelope. No action required.' },
];

const SCHEDULES: ForgeSchedule[] = [
  { id: 'sc-flight', pipeline: 'flight_tracks_eu', mode: 'batch', trigger: 'cron · every 5m', last: 'OK · 1m42s', next: 'in 3m', state: 'healthy', stateLabel: 'On schedule', backfill: 'auto on gap', concurrency: '1 · queue next', onFailure: 'retry 3× then alert', note: null },
  { id: 'sc-fusion', pipeline: 'sensor_fusion_live', mode: 'stream', trigger: 'continuous', last: 'running · 4d', next: '—', state: 'online', stateLabel: 'Streaming', backfill: 'checkpointed · resume', concurrency: '1 · queue next', onFailure: 'retry 3× then alert', note: null },
  { id: 'sc-maint', pipeline: 'maintenance_rollup', mode: 'batch', trigger: 'on upstream commit', last: 'OK · 22s', next: 'on v43', state: 'healthy', stateLabel: 'Event-driven', backfill: 'auto on gap', concurrency: '1 · queue next', onFailure: 'retry 3× then alert', note: null },
  { id: 'sc-crew', pipeline: 'crew_roster_sync', mode: 'batch', trigger: 'cron · daily 02:00', last: 'FAIL · retry 2/3', next: '02:00', state: 'warn', stateLabel: 'Retrying', backfill: 'auto on gap', concurrency: '1 · queue next', onFailure: 'retry 3× then alert', note: 'Source connection refused at 02:00. Next retry with backoff; alert fires if retry 3 fails. No partial version was committed.' },
];

const TRANSFORM: ForgeTransform = {
  id: 'fuse_positions',
  branch: 'dev/fuse-fix',
  nextVersion: 'v43',
  filesCount: 3,
  expectations: '12/12',
  files: ['fuse_positions.py', 'clean_flights.py', 'expectations.yml'],
  activeFile: 'fuse_positions.py',
  code: [
    'from forge import transform, Dataset, expect',
    '',
    '@transform(',
    '    inputs={"raw": Dataset("sensor_stream")},',
    '    output=Dataset("flight_tracks_eu"),',
    ')',
    'def fuse_positions(raw):',
    '    df = raw.dataframe()',
    '    df = df[df.alt_ft.between(0, 60000)]  # bounds [FR-F-6]',
    '    return df.dedupe(on="icao24", keep="latest")',
    '',
    'expect(fuse_positions).rows_within(0.15).of_7d_mean()',
  ],
  contractChecks: [
    ['Input schema', 'sensor_stream v12 · 21 cols ok'],
    ['Output schema', 'flight_tracks_eu · 18 cols ok'],
    ['Expectations', '2 attached · run on build'],
    ['Marking flow', 'CONFIDENTIAL propagates ok'],
  ],
};

const PROJECTIONS: ForgeProjection[] = [
  {
    id: 'aircraft', name: 'Aircraft projection', source: 'flight_tracks_eu · v42', target: 'Aircraft · object fabric',
    objects: '12,408', syncLag: '41s', columnsMapped: '18/18', linkTypes: 2, unmarkedRows: 0,
    marking: 'confidential', compartment: 'OPS-N',
    mappings: [
      { id: 'm-icao', column: 'icao24', to: 'Aircraft.tail_id', kind: 'key', note: 'primary match' },
      { id: 'm-callsign', column: 'callsign', to: 'Aircraft.callsign', kind: 'property', note: 'trim + uppercase' },
      { id: 'm-operator', column: 'operator_code', to: 'operates → Route', kind: 'link', note: 'via route registry' },
      { id: 'm-position', column: 'alt_ft, lat, lon', to: 'Aircraft.position', kind: 'struct', note: 'composite · 1Hz downsample' },
      { id: 'm-marking', column: 'src_classification', to: 'marking', kind: 'governance', note: 'row-level marking' },
    ],
  },
];

const AUDIT_SEED: AuditRecord[] = [
  { id: 'FRG-2042', actor: 'svc:forge/builder', action: 'Build · flight_tracks_eu', resource: 'dataset/flight_tracks_eu@v41', pqSignature: 'ML-DSA ✓', ts: '14:02:11' },
  { id: 'FRG-2041', actor: 'j.benjamin', action: 'Commit · fuse_positions', resource: 'transform/fuse_positions@v40', pqSignature: 'ML-DSA ✓', ts: '13:58:20' },
  { id: 'FRG-2040', actor: 'svc:forge/projector', action: 'Project · Aircraft objects', resource: 'projection/aircraft', pqSignature: 'ML-DSA ✓', ts: '13:57:02' },
  { id: 'FRG-2039', actor: 'svc:forge/connector', action: 'Pull · flights_jdbc', resource: 'connector/flights_jdbc', pqSignature: 'ML-DSA ✓', ts: '13:55:44' },
  { id: 'FRG-2038', actor: 'svc:forge/health', action: 'Gate · closed_by integrity', resource: 'check/hc-closed-by', pqSignature: 'ML-DSA ✓', ts: '13:54:19' },
];

const AUDIT_LIVE_POOL: Array<Omit<AuditRecord, 'id' | 'ts'>> = [
  { actor: 'svc:forge/connector', action: 'Pull · sensor_stream', resource: 'connector/sensor_stream', pqSignature: 'ML-DSA ✓' },
  { actor: 'svc:forge/projector', action: 'Project · sync', resource: 'projection/aircraft', pqSignature: 'ML-DSA ✓' },
  { actor: 'svc:forge/health', action: 'Gate · freshness SLO', resource: 'check/hc-freshness', pqSignature: 'ML-DSA ✓' },
  { actor: 'svc:forge/builder', action: 'Seal · content-hash', resource: 'dataset/sensor_feed_north@v12', pqSignature: 'ML-DSA ✓' },
];

const DEFAULT_POLICY: PolicyContext = {
  subject: 'j.benjamin',
  purpose: 'data-operations',
  clearance: 'confidential',
  roles: ['data-engineer', 'operator'],
};

export interface ForgeMockOptions {
  /** Simulated round-trip latency in ms (default 160). */
  latencyMs?: number;
  policy?: PolicyContext;
}

/**
 * In-memory implementation of `ForgeGateway`.
 *
 * Deterministic (no `Date.now()`/`Math.random()` at module load): the audit
 * clock advances from a fixed seed and ids increment from a fixed base, so the
 * console renders reproducibly. Models the enclosure closely enough to exercise
 * every Forge flow — staged builds, previewed-then-gated transform commits, and
 * a live audit stream.
 */
export class ForgeMock implements ForgeGateway {
  private readonly latency: number;
  private readonly ctx: PolicyContext;
  private audit: AuditRecord[];
  private seq = 0;
  private runSeq = 0;
  private poolIdx = 0;
  private clock = 14 * 3600 + 2 * 60 + 11; // seeds "14:02:11"
  private listeners = new Set<(r: AuditRecord) => void>();
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(opts: ForgeMockOptions = {}) {
    this.latency = opts.latencyMs ?? 160;
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
    return `FRG-${2043 + this.seq}`;
  }

  private emit(rec: Omit<AuditRecord, 'id' | 'ts'>): AuditRecord {
    const record: AuditRecord = { ...rec, id: this.nextId(), ts: this.stamp() };
    this.audit = [record, ...this.audit].slice(0, 200);
    this.listeners.forEach((fn) => fn(record));
    return record;
  }

  // ---- reads ---------------------------------------------------------------

  listPipelines(): Promise<ForgePipeline[]> {
    return this.wait(PIPELINES.map((p) => ({ ...p, nodes: p.nodes.map((n) => ({ ...n })), edges: p.edges.map((e) => ({ ...e })), versions: p.versions.map((v) => ({ ...v })) })));
  }
  listConnectors(): Promise<ForgeConnector[]> {
    return this.wait(CONNECTORS.map((c) => ({ ...c })));
  }
  listDatasets(): Promise<ForgeDataset[]> {
    return this.wait(DATASETS.map((d) => ({ ...d, diff: d.diff.map((x) => ({ ...x })), lineage: d.lineage.map((x) => ({ ...x })) })));
  }
  listHealthChecks(): Promise<ForgeHealthCheck[]> {
    return this.wait(HEALTH_CHECKS.map((h) => ({ ...h })));
  }
  listSchedules(): Promise<ForgeSchedule[]> {
    return this.wait(SCHEDULES.map((s) => ({ ...s })));
  }
  getTransform(): Promise<ForgeTransform> {
    return this.wait({ ...TRANSFORM, files: [...TRANSFORM.files], code: [...TRANSFORM.code], contractChecks: TRANSFORM.contractChecks.map((c) => [...c] as [string, string]) });
  }
  listProjections(): Promise<ForgeProjection[]> {
    return this.wait(PROJECTIONS.map((p) => ({ ...p, mappings: p.mappings.map((m) => ({ ...m })) })));
  }

  // ---- key flows -----------------------------------------------------------

  async runPipeline(id: string): Promise<ForgeBuildResult> {
    const pipeline = PIPELINES.find((p) => p.id === id) ?? PIPELINES[0];
    this.runSeq += 1;
    const nextVersion = `v${parseInt(pipeline.currentVersion.slice(1), 10) + 1}`;
    const audit = this.emit({
      actor: this.ctx.subject,
      action: `Build · ${pipeline.name}`,
      resource: `dataset/${pipeline.name}@${nextVersion}`,
      pqSignature: 'ML-DSA ✓',
    });
    const steps: ForgeBuildStep[] = [
      { stage: 'resolve', detail: 'sources 2/2 · PQ-mTLS OK', status: 'ok' },
      { stage: 'transform', detail: 'clean_flights · 6/6 steps', status: 'ok' },
      { stage: 'transform', detail: 'fuse_positions · expectations 11/12 watch', status: 'warn' },
      { stage: 'seal', detail: 'AES-256-GCM · ML-KEM-1024 wrap', status: 'ok' },
      { stage: 'project', detail: '12,408 objects → fabric', status: 'ok' },
      { stage: 'done', detail: `build ${nextVersion} OK · 1m38s · lineage signed (ML-DSA)`, status: 'ok' },
    ];
    return this.wait({ runId: `RUN-${1600 + this.runSeq}`, version: nextVersion, durationLabel: '1m38s', steps });
  }

  async previewTransform(sql: string): Promise<ForgePreviewResult> {
    this.emit({
      actor: this.ctx.subject,
      action: 'Preview · transform',
      resource: 'transform/fuse_positions · sandbox',
      pqSignature: 'ML-DSA ✓',
    });
    const trimmed = sql.trim();
    const diff: ForgeTransformDiff = {
      summary: trimmed ? `Preview of: ${trimmed.slice(0, 72)}${trimmed.length > 72 ? '…' : ''}` : 'Preview of the proposed bounds filter',
      rowsIn: 48112,
      rowsOut: 47924,
      expectations: '12/12',
      durationLabel: '4.1s',
      hunks: [
        { op: '~', text: 'fuse_positions: add df[df.alt_ft.between(0, 60000)] before join' },
        { op: '-', text: '188 out-of-bounds rows dropped (alt_ft ≥ 60000)' },
        { op: '+', text: "expectation rows_within(0.15).of_7d_mean() now 12/12" },
      ],
    };
    return this.wait({ diff });
  }

  async applyTransform(sql: string, humanGateToken?: string): Promise<ForgeApplyResult> {
    if (!humanGateToken) {
      throw new ForgeHumanGateRequiredError('fuse_positions');
    }
    void sql;
    const audit = this.emit({
      actor: this.ctx.subject,
      action: 'Commit · fuse_positions',
      resource: 'transform/fuse_positions@v43',
      pqSignature: 'ML-DSA ✓',
    });
    return this.wait({ auditId: audit.id, version: 'v43', applied: true });
  }

  // ---- governance ----------------------------------------------------------

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
