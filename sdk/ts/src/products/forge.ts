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
 * Shared governance primitives are imported from `../types`.
 *
 * The mock and its fixtures live in ../mock/forge.ts and are exported from
 * `@midgard/sdk/mock` only, so a console's main bundle never holds them.
 * [VLD-ANB-MIDGARD-0086] [AC-25.2]
 */

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
