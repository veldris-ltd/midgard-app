/* Bifrost gateway slice — continuous delivery (PQ-signed artefacts, rings,
 * fleet, offline bundles, rollback).
 *
 * One product-prefixed domain model + a single transport seam
 * (`BifrostGateway`) + an in-memory `BifrostMock`. Mirrors the Midgard
 * `GatewayClient`/`MockGateway` pattern: the UI depends only on the interface,
 * so the mock is swap-compatible with a future `HttpGateway` over the enclosure
 * PQ-mTLS bridge. Deterministic — no Date.now()/random at module load.
 *
 * Governance chrome tracked by the console screens:
 *   FR-B-2  zero-downtime rolling upgrades, health-gated automatic rollback
 *   FR-B-3  declarative release channels + fleet/installation management
 *   FR-B-4  reproducible builds + SBOM + PQ signature, verified before promotion
 *   FR-B-5  signed offline bundles for air-gapped sites
 *   FR-B-6  AI release intelligence (risk scoring / anomaly detection via Mimir)
 *   STD:NIST FIPS 204 (ML-DSA) / FIPS 205 (SLH-DSA) artefact signing
 *
 * The mock and its fixtures live in ../mock/bifrost.ts and are exported from
 * `@midgard/sdk/mock` only, so a console's main bundle never holds them.
 * [VLD-ANB-MIDGARD-0086] [AC-25.2]
 */

import type { AuditRecord, PolicyContext } from '../types';

// ---- domain model -----------------------------------------------------------

/** Health gate verdict on a rollout/ring — advancement only while it holds. */
export type BifrostHealthGate = 'pass' | 'watch' | 'fail';

/** Progressive deployment ring: canary before broad, air-gap by courier. */
export type BifrostRingStage = 'canary' | 'staging' | 'broad' | 'air-gap';

/** State tone shared with the DS StatusPill (`secure`/`warn`/`danger`/…). */
export type BifrostState =
  | 'secure'
  | 'healthy'
  | 'online'
  | 'warn'
  | 'danger'
  | 'offline'
  | 'gated';

/** A deployment ring with rolling progress (`done`/`total` installations). */
export interface BifrostRing {
  id: string;
  /** Display name, e.g. "RING 2 · PROD-EU". */
  name: string;
  stage: BifrostRingStage;
  total: number;
  done: number;
  /** Air-gap rings ship as a signed offline bundle rather than online rollout. */
  offlineBundle?: boolean;
}

/** A release under rollout across the channels/rings. */
export interface BifrostRelease {
  id: string;
  /** Artefact/suite name, e.g. "midgard-suite". */
  name: string;
  version: string;
  channel: string;
  /** Mimir advisory risk score (advisory-only; rule gates always apply). */
  riskScore: 'LOW' | 'MEDIUM' | 'HIGH';
  /** Installations progressed, e.g. "37/73". */
  installations: string;
  failedRequests: number;
  /** Rollback gate posture, e.g. "AUTO". */
  rollbackGate: string;
  healthGate: BifrostHealthGate;
}

/** A registered artefact: reproducible build + CycloneDX SBOM + ML-DSA seal. */
export interface BifrostArtefact {
  id: string;
  name: string;
  version: string;
  kind: string;
  size: string;
  /** Reproducibility attestation, e.g. "2/2", "1/2 ⚠", "n/a · digest". */
  reproducible: string;
  criticalCves: number;
  /** PQ signature marker, e.g. "ML-DSA ✓". */
  signature: string;
  /** SBOM marker, e.g. "CycloneDX". */
  sbom: string;
  state: BifrostState;
  stateLabel: string;
}

/** A release channel: cadence, subscribers, promotion policy. */
export interface BifrostChannel {
  name: string;
  cadence: string;
  subscribers: number;
  current: string;
  next: string;
  policy: string;
  state: BifrostState;
  stateLabel: string;
}

/** A signed offline bundle cut for air-gapped sites. */
export interface BifrostBundle {
  id: string;
  release: string;
  sites: string;
  size: string;
  cut: string;
  status: string;
  state: BifrostState;
  stateLabel: string;
}

/** A fleet installation with desired-vs-actual drift. */
export interface BifrostFleetNode {
  id: string;
  ring: string;
  desired: string;
  actual: string;
  /** Drift summary, e.g. "none", "version · held", "config · 1 key". */
  drift: string;
  /** On-host agent identity/attestation, e.g. "ML-DSA ✓" or "offline". */
  agent: string;
  state: BifrostState;
  stateLabel: string;
}

/** A standing rollback plan — rehearsed, not hoped for. */
export interface BifrostRollback {
  id: string;
  scope: string;
  kind: string;
  trigger: string;
  readiness: string;
  lastDrill: string;
  state: BifrostState;
  stateLabel: string;
}

/** Result of advancing a ring one promotion step. `[FR-B-2]` */
export interface BifrostPromotion {
  ring: BifrostRingStage;
  /** 0–100 rolled-out percentage of the ring after this step. */
  progress: number;
  healthGate: BifrostHealthGate;
  auditId: string;
}

// ---- transport seam ---------------------------------------------------------

/**
 * The single transport seam for the Bifrost console.
 *
 * Every screen talks to this interface — never a concrete client — so the data
 * source swaps without touching UI code. `BifrostMock` backs it today; a
 * browser-reachable `HttpGateway` implements the same shape later.
 *
 * KEY FLOW: `promoteRelease` (canary → broad, staged progress under a health
 * gate) and `rollback` (health-gated reversal to last-good). Artefacts carry
 * ML-DSA signatures + CycloneDX SBOM markers; verification is fail-closed.
 */
export interface BifrostGateway {
  /** The active policy context (subject/purpose/clearance/roles). */
  policy(): PolicyContext;

  // Delivery -----------------------------------------------------------------
  listReleases(): Promise<BifrostRelease[]>;
  listRings(): Promise<BifrostRing[]>;
  /** Advance `ring` one promotion step for `releaseId`; returns staged progress
   *  + health-gate verdict + a signed audit id. Fail-closed on a failing gate. */
  promoteRelease(releaseId: string, ring: BifrostRingStage): Promise<BifrostPromotion>;
  listArtefacts(): Promise<BifrostArtefact[]>;
  listChannels(): Promise<BifrostChannel[]>;
  listBundles(): Promise<BifrostBundle[]>;

  // Fleet --------------------------------------------------------------------
  listFleet(): Promise<BifrostFleetNode[]>;
  listRollbacks(): Promise<BifrostRollback[]>;
  /** Execute a standing rollback plan; returns the signed audit id. `[FR-B-2]` */
  rollback(id: string): Promise<{ auditId: string }>;

  // Governance / audit -------------------------------------------------------
  getAudit(limit?: number): Promise<AuditRecord[]>;
  auditStream(onRecord: (record: AuditRecord) => void): () => void;
}
