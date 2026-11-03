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
 * network, no persistence — swap for a real bridge behind this same interface.
 *
 * The mock and its fixtures live in ../mock/heimdall.ts and are exported from
 * `@midgard/sdk/mock` only, so a console's main bundle never holds them.
 * [VLD-ANB-MIDGARD-0086] [AC-25.2]
 */

import type { AuditRecord, MarkingLevel, PolicyContext } from '../types';

// ---- shared enums -----------------------------------------------------------

/** Live-state key shared by feeds, sites and workflows (maps 1:1 to StatusPill). */
export type HeimdallState = 'online' | 'secure' | 'healthy' | 'offline' | 'warn' | 'danger' | 'gated';

/** Track quality for a fused entity. */
export type HeimdallTrackState = 'stable' | 'anomalous';

/** Post-quantum source-signature scheme a feed authenticates each frame with. */
export type HeimdallSignatureScheme = 'ML-DSA' | 'SLH-DSA';

/** Picture layers the operator can toggle. */
export type HeimdallLayerId = 'air' | 'maritime' | 'ground' | 'feeds';

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
