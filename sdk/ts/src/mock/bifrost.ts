/* @midgard/sdk/mock · Bifrost product slice: the in-memory `BifrostMock` and its
 * deterministic fixtures, behind the `Bifrost` gateway interface in
 * ../products/bifrost.ts. Reached only through `@midgard/sdk/mock`, which a console
 * loads by dynamic import in mock mode, so no http build bundles it.
 * [VLD-ANB-MIDGARD-0086] [AC-25.2] */

import type { AuditRecord, PolicyContext } from '../types';
import type {
  BifrostHealthGate,
  BifrostRingStage,
  BifrostRing,
  BifrostRelease,
  BifrostArtefact,
  BifrostChannel,
  BifrostBundle,
  BifrostFleetNode,
  BifrostRollback,
  BifrostPromotion,
  BifrostGateway,
} from '../products/bifrost';

// ---- fixtures ---------------------------------------------------------------

const RINGS: BifrostRing[] = [
  { id: 'ring-0', name: 'RING 0 · CANARY', stage: 'canary', total: 4, done: 4 },
  { id: 'ring-1', name: 'RING 1 · STAGING', stage: 'staging', total: 12, done: 12 },
  { id: 'ring-2', name: 'RING 2 · PROD-EU', stage: 'broad', total: 48, done: 21 },
  { id: 'ring-3', name: 'RING 3 · AIR-GAP SITES', stage: 'air-gap', total: 9, done: 0, offlineBundle: true },
];

const RELEASES: BifrostRelease[] = [
  { id: 'REL-2050', name: 'midgard-suite', version: '2.5.0', channel: 'stable', riskScore: 'LOW', installations: '37/73', failedRequests: 0, rollbackGate: 'AUTO', healthGate: 'watch' },
  { id: 'REL-2041', name: 'midgard-suite', version: '2.4.1', channel: 'stable', riskScore: 'LOW', installations: '73/73', failedRequests: 0, rollbackGate: 'AUTO', healthGate: 'pass' },
  { id: 'REL-2051RC', name: 'midgard-suite', version: '2.5.1-rc', channel: 'canary', riskScore: 'MEDIUM', installations: '3/3', failedRequests: 0, rollbackGate: 'AUTO', healthGate: 'pass' },
];

const ARTEFACTS: BifrostArtefact[] = [
  { id: 'ART-1', name: 'midgard-suite', version: '2.5.0', kind: 'platform bundle', size: '4.1 GB', reproducible: '2/2', criticalCves: 0, signature: 'ML-DSA ✓', sbom: 'CycloneDX', state: 'secure', stateLabel: 'Verified' },
  { id: 'ART-2', name: 'mimir-weights-mistral', version: '3.2.1', kind: 'model weights', size: '38 GB', reproducible: 'n/a · digest', criticalCves: 0, signature: 'ML-DSA ✓', sbom: 'CycloneDX', state: 'secure', stateLabel: 'Verified' },
  { id: 'ART-3', name: 'forge-connectors', version: '1.9.2', kind: 'plugin set', size: '212 MB', reproducible: '2/2', criticalCves: 1, signature: 'ML-DSA ✓', sbom: 'CycloneDX', state: 'warn', stateLabel: 'CVE review' },
  { id: 'ART-4', name: 'heimdall-fusion', version: '3.0.0-rc2', kind: 'service image', size: '890 MB', reproducible: '1/2 ⚠', criticalCves: 0, signature: 'ML-DSA ✓', sbom: 'CycloneDX', state: 'danger', stateLabel: 'Not reproducible' },
];

const CHANNELS: BifrostChannel[] = [
  { name: 'stable', cadence: 'monthly', subscribers: 61, current: '2.4.1', next: '2.5.0 staged', policy: 'all gates + 7d soak', state: 'secure', stateLabel: 'Healthy' },
  { name: 'beta', cadence: 'weekly', subscribers: 9, current: '2.5.0', next: '2.5.1 building', policy: 'all gates', state: 'healthy', stateLabel: 'Healthy' },
  { name: 'canary', cadence: 'on merge', subscribers: 3, current: '2.5.1-rc', next: 'continuous', policy: 'health gates only', state: 'online', stateLabel: 'Rolling' },
];

const BUNDLES: BifrostBundle[] = [
  { id: 'B-2050-07A', release: '2.5.0', sites: 'site-north-ag +3', size: '42 GB', cut: 'today 09:12', status: 'awaiting courier', state: 'gated', stateLabel: 'Ready' },
  { id: 'B-2050-06B', release: '2.4.1', sites: 'site-east-ag', size: '41 GB', cut: '12d ago', status: 'applied · receipt returned', state: 'secure', stateLabel: 'Applied' },
  { id: 'B-2050-06A', release: '2.4.1', sites: 'site-north-ag +3', size: '41 GB', cut: '31d ago', status: 'applied · receipt returned', state: 'secure', stateLabel: 'Applied' },
];

const FLEET: BifrostFleetNode[] = [
  { id: 'enclave-dub-01', ring: 'PROD-EU', desired: '2.5.0', actual: '2.5.0', drift: 'none', agent: 'ML-DSA ✓', state: 'secure', stateLabel: 'Converged' },
  { id: 'enclave-fra-02', ring: 'PROD-EU', desired: '2.5.0', actual: '2.5.0', drift: 'none', agent: 'ML-DSA ✓', state: 'secure', stateLabel: 'Converged' },
  { id: 'enclave-ams-03', ring: 'PROD-EU', desired: '2.5.0', actual: '2.4.1', drift: 'version · held', agent: 'ML-DSA ✓', state: 'warn', stateLabel: 'Health gate' },
  { id: 'site-north-ag', ring: 'AIR-GAP', desired: '2.5.0', actual: '2.4.0', drift: 'bundle pending', agent: 'offline', state: 'offline', stateLabel: 'Awaiting courier' },
  { id: 'enclave-lon-01', ring: 'PROD-EU', desired: '2.5.0', actual: '2.5.0+cfg', drift: 'config · 1 key', agent: 'ML-DSA ✓', state: 'warn', stateLabel: 'Config drift' },
];

const ROLLBACKS: BifrostRollback[] = [
  { id: 'RB-REL-2050', scope: 'midgard-suite 2.5.0 → 2.4.1', kind: 'release', trigger: 'health gate failure', readiness: 'staged on 73/73', lastDrill: '2d ago', state: 'secure', stateLabel: 'Armed' },
  { id: 'RB-MIMIR-P7', scope: 'mimir policy v7 → v6', kind: 'agent policy', trigger: 'eval regression', readiness: 'instant · versioned', lastDrill: '6h ago', state: 'secure', stateLabel: 'Armed' },
  { id: 'RB-FORGE-42', scope: 'forge flight_tracks v42 → v41', kind: 'dataset', trigger: 'manual only', readiness: 'time travel', lastDrill: 'n/a', state: 'healthy', stateLabel: 'Available' },
];

const AUDIT_SEED: AuditRecord[] = [
  { id: 'AUD-B-1042', actor: 'j.benjamin', action: 'Promote · REL-2050 → ring broad', resource: 'ring/prod-eu', pqSignature: 'ML-DSA ✓', ts: '14:32:08' },
  { id: 'AUD-B-1041', actor: 'svc:bifrost/reconciler', action: 'Reconcile · fleet drift', resource: 'enclave-ams-03', pqSignature: 'ML-DSA ✓', ts: '14:31:56' },
  { id: 'AUD-B-1040', actor: 'svc:bifrost/signer', action: 'Sign · artefact', resource: 'midgard-suite:2.5.0', pqSignature: 'ML-DSA ✓', ts: '14:31:41' },
  { id: 'AUD-B-1039', actor: 'j.benjamin', action: 'Cut · offline bundle', resource: 'B-2050-07A', pqSignature: 'ML-DSA ✓', ts: '14:31:12' },
  { id: 'AUD-B-1038', actor: 'agent:mimir/release-risk', action: 'Score · release risk (LOW)', resource: 'REL-2050', pqSignature: 'ML-DSA ✓', ts: '14:30:47' },
];

const AUDIT_LIVE_POOL: Omit<AuditRecord, 'id' | 'ts'>[] = [
  { actor: 'svc:bifrost/reconciler', action: 'Reconcile · desired vs actual', resource: 'enclave-lon-01', pqSignature: 'ML-DSA ✓' },
  { actor: 'svc:bifrost/verifier', action: 'Verify · reproducible build', resource: 'forge-connectors:1.9.2', pqSignature: 'ML-DSA ✓' },
  { actor: 'agent:mimir/release-risk', action: 'Score · diff surface 84% config', resource: 'REL-2050', pqSignature: 'ML-DSA ✓' },
  { actor: 'svc:bifrost/courier', action: 'Receipt · bundle applied', resource: 'site-east-ag', pqSignature: 'ML-DSA ✓' },
];

const DEFAULT_POLICY: PolicyContext = {
  subject: 'j.benjamin',
  purpose: 'release-management',
  clearance: 'internal',
  roles: ['release-manager', 'operator'],
};

export interface BifrostMockOptions {
  /** Simulated round-trip latency in ms (default 180). */
  latencyMs?: number;
  policy?: PolicyContext;
}

// ---- mock -------------------------------------------------------------------

/**
 * In-memory implementation of the BifrostGateway seam.
 *
 * Holds live ring state so repeated `promoteRelease` calls advance a ring
 * canary → broad under a health gate, each step emitting a signed audit record.
 * No network, no persistence — swap for HttpGateway when a bridge exists.
 */
export class BifrostMock implements BifrostGateway {
  private readonly latency: number;
  private readonly ctx: PolicyContext;
  private rings: BifrostRing[];
  private audit: AuditRecord[];
  private seq = 0;
  private clock = 14 * 3600 + 32 * 60 + 8; // seeds "14:32:08"
  private listeners = new Set<(r: AuditRecord) => void>();
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(opts: BifrostMockOptions = {}) {
    this.latency = opts.latencyMs ?? 180;
    this.ctx = opts.policy ?? DEFAULT_POLICY;
    this.rings = RINGS.map((r) => ({ ...r }));
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
    return `AUD-B-${1043 + this.seq}`;
  }

  private emit(rec: Omit<AuditRecord, 'id' | 'ts'>): AuditRecord {
    const record: AuditRecord = { ...rec, id: this.nextId(), ts: this.stamp() };
    this.audit = [record, ...this.audit].slice(0, 200);
    this.listeners.forEach((fn) => fn(record));
    return record;
  }

  // ---- delivery ------------------------------------------------------------

  listReleases(): Promise<BifrostRelease[]> {
    return this.wait(RELEASES.map((r) => ({ ...r })));
  }

  listRings(): Promise<BifrostRing[]> {
    return this.wait(this.rings.map((r) => ({ ...r })));
  }

  async promoteRelease(releaseId: string, ring: BifrostRingStage): Promise<BifrostPromotion> {
    const target = this.rings.find((r) => r.stage === ring);
    // Air-gap rings advance by signed courier bundle, not online promotion —
    // fail-closed rather than pretend an online rollout occurred. `[FR-B-5]`
    if (target?.offlineBundle) {
      const blocked = this.emit({
        actor: this.ctx.subject,
        action: `Promote · ${releaseId} → ring ${ring} (bundle required)`,
        resource: target.id,
        pqSignature: 'ML-DSA ✓',
      });
      return this.wait({ ring, progress: Math.round(((target.done) / target.total) * 100), healthGate: 'fail', auditId: blocked.id });
    }
    if (target && target.done < target.total) {
      const step = Math.max(1, Math.ceil(target.total * 0.15));
      target.done = Math.min(target.total, target.done + step);
    }
    const progress = target ? Math.round((target.done / target.total) * 100) : 0;
    // While rolling the gate is WATCHing; a fully rolled ring clears to PASS.
    const healthGate: BifrostHealthGate = progress >= 100 ? 'pass' : 'watch';
    const audit = this.emit({
      actor: this.ctx.subject,
      action: `Promote · ${releaseId} → ring ${ring} (${progress}%)`,
      resource: target?.id ?? `ring/${ring}`,
      pqSignature: 'ML-DSA ✓',
    });
    return this.wait({ ring, progress, healthGate, auditId: audit.id });
  }

  listArtefacts(): Promise<BifrostArtefact[]> {
    return this.wait(ARTEFACTS.map((a) => ({ ...a })));
  }

  listChannels(): Promise<BifrostChannel[]> {
    return this.wait(CHANNELS.map((c) => ({ ...c })));
  }

  listBundles(): Promise<BifrostBundle[]> {
    return this.wait(BUNDLES.map((b) => ({ ...b })));
  }

  // ---- fleet ---------------------------------------------------------------

  listFleet(): Promise<BifrostFleetNode[]> {
    return this.wait(FLEET.map((f) => ({ ...f })));
  }

  listRollbacks(): Promise<BifrostRollback[]> {
    return this.wait(ROLLBACKS.map((r) => ({ ...r })));
  }

  async rollback(id: string): Promise<{ auditId: string }> {
    const plan = ROLLBACKS.find((r) => r.id === id);
    const audit = this.emit({
      actor: this.ctx.subject,
      action: `Rollback · ${plan?.scope ?? id}`,
      resource: id,
      pqSignature: 'ML-DSA ✓',
    });
    return this.wait({ auditId: audit.id });
  }

  // ---- governance / audit --------------------------------------------------

  getAudit(limit = 50): Promise<AuditRecord[]> {
    return this.wait(this.audit.slice(0, limit).map((r) => ({ ...r })));
  }

  auditStream(onRecord: (record: AuditRecord) => void): () => void {
    this.listeners.add(onRecord);
    if (!this.timer) {
      let poolIdx = 0;
      this.timer = setInterval(() => {
        const next = AUDIT_LIVE_POOL[poolIdx % AUDIT_LIVE_POOL.length];
        poolIdx += 1;
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
