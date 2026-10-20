import type { GatewayClient } from '../client';
import type {
  ApplyActionRequest,
  ApplyActionResponse,
  AuditRecord,
  ComputeJob,
  Identity,
  KeyRecord,
  ObjectType,
  PolicyContext,
  PolicyDecision,
  PolicyRule,
  QueryRequest,
  QueryResponse,
  RolloutState,
  RpcSurface,
  SearchGraph,
  SloMetric,
} from '../types';
import {
  AUDIT_LIVE_POOL,
  AUDIT_SEED,
  COMPUTE_JOBS,
  CONTRACT_SURFACE,
  IDENTITIES,
  KEYS,
  OBJECT_TYPES,
  OBSERVABILITY,
  POLICIES,
  RESILIENCE,
} from './fixtures';

/** Error thrown when a gated action is applied without a human-gate token. */
export class HumanGateRequiredError extends Error {
  constructor(actionType: string) {
    super(`Human gate required for action "${actionType}"`);
    this.name = 'HumanGateRequiredError';
  }
}

export interface MockGatewayOptions {
  /** Simulated round-trip latency in ms (default 180). */
  latencyMs?: number;
  policy?: PolicyContext;
}

const DEFAULT_POLICY: PolicyContext = {
  subject: 'j.benjamin',
  purpose: 'operations',
  clearance: 'confidential',
  roles: ['operator', 'analyst'],
};

/**
 * In-memory implementation of the GatewayClient seam.
 *
 * Models the enclosure's behaviour closely enough to exercise every UI flow:
 * policy-scoped queries, human-gated actions that emit signed audit records,
 * and a live audit stream. No network, no persistence — swap for HttpGateway
 * when a browser-reachable bridge exists.
 */
export class MockGateway implements GatewayClient {
  private readonly latency: number;
  private readonly ctx: PolicyContext;
  private audit: AuditRecord[];
  private seq = AUDIT_SEED.length;
  private poolIdx = 0;
  private clock = 14 * 3600 + 32 * 60 + 8; // seconds since midnight, seeds "14:32:08"
  private listeners = new Set<(r: AuditRecord) => void>();
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(opts: MockGatewayOptions = {}) {
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
    return `AUD-${1042 + (this.seq - AUDIT_SEED.length)}`;
  }

  private emit(rec: Omit<AuditRecord, 'id' | 'ts'>): AuditRecord {
    const record: AuditRecord = { ...rec, id: this.nextId(), ts: this.stamp() };
    this.audit = [record, ...this.audit].slice(0, 200);
    this.listeners.forEach((fn) => fn(record));
    return record;
  }

  // ---- object fabric -------------------------------------------------------

  listObjectTypes(): Promise<ObjectType[]> {
    return this.wait(OBJECT_TYPES.map((t) => ({ ...t })));
  }

  async query(req: QueryRequest): Promise<QueryResponse> {
    const audit = this.emit({
      actor: this.ctx.subject,
      action: `Query · ${req.objectType}`,
      resource: `${req.objectType}/*`,
      pqSignature: 'ML-DSA ✓',
    });
    // The fabric returns instances marking-filtered by policy; the mock
    // synthesises a small set from the matching type.
    const type = OBJECT_TYPES.find((t) => t.apiName === req.objectType);
    const n = Math.min(req.maxResults ?? 5, 5);
    const results = Array.from({ length: type ? n : 0 }, (_, i) => ({
      id: `${req.objectType}/${1000 + i}`,
      typeId: req.objectType,
      propertyValues: { status: i % 2 ? 'active' : 'standby', updated: type?.updated ?? '' },
      markings: [
        {
          id: `mk-${type?.marking ?? 'internal'}`,
          name: (type?.marking ?? 'internal').toUpperCase(),
          level: type?.marking ?? 'internal',
          compartments: type?.compartment ? [type.compartment] : [],
        },
      ],
    }));
    const needle = req.search?.toLowerCase();
    const found = results.filter(
      (o) =>
        (!req.id || o.id === req.id) &&
        (!needle || o.id.toLowerCase().includes(needle) || Object.values(o.propertyValues).some((v) => v.toLowerCase().includes(needle))),
    );
    return this.wait({ results: found, matched: found.length, auditId: audit.id });
  }

  async applyAction(req: ApplyActionRequest): Promise<ApplyActionResponse> {
    const type = OBJECT_TYPES.find((t) => t.actions.some((a) => a.name === req.actionType));
    const action = type?.actions.find((a) => a.name === req.actionType);
    if (action?.humanGated && !req.humanGateToken) {
      throw new HumanGateRequiredError(req.actionType);
    }
    const audit = this.emit({
      actor: this.ctx.subject,
      action: `Action · ${req.actionType}`,
      resource: req.params?.target ?? `${type?.apiName ?? 'fabric'}/*`,
      pqSignature: 'ML-DSA ✓',
    });
    return this.wait({ auditId: audit.id, applied: true });
  }

  async searchAround(seedId: string, hops: number): Promise<SearchGraph> {
    const audit = this.emit({
      actor: this.ctx.subject,
      action: 'Query · search-around',
      resource: seedId,
      pqSignature: 'ML-DSA ✓',
    });
    const nodes: SearchGraph['nodes'] = [
      { id: seedId, label: seedId, typeName: 'Aircraft', marking: 'confidential', hop: 0 },
    ];
    const edges: SearchGraph['edges'] = [];
    const shape = [
      { label: 'operates → Route', typeName: 'Route', marking: 'internal' as const },
      { label: 'assigned → Crew', typeName: 'Crew Member', marking: 'internal' as const },
      { label: 'observes → Entity', typeName: 'Sensor Feed', marking: 'confidential' as const },
      { label: 'on → Event', typeName: 'Maintenance Event', marking: 'internal' as const },
    ];
    let idx = 0;
    for (let hop = 1; hop <= Math.max(1, hops); hop++) {
      const count = hop === 1 ? 3 : 2;
      for (let i = 0; i < count; i++) {
        const s = shape[idx % shape.length];
        idx += 1;
        const id = `n${hop}-${i}`;
        nodes.push({ id, label: `${s.typeName} ${hop}.${i + 1}`, typeName: s.typeName, marking: s.marking, hop });
        const parent = hop === 1 ? seedId : `n${hop - 1}-0`;
        edges.push({ from: parent, to: id, label: s.label });
      }
    }
    return this.wait({ seed: seedId, nodes, edges, auditId: audit.id });
  }

  // ---- governance / audit --------------------------------------------------

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

  // ---- policies ------------------------------------------------------------

  listPolicies(): Promise<PolicyRule[]> {
    return this.wait(POLICIES.map((p) => ({ ...p })));
  }

  async simulatePolicy(ctx: PolicyContext, marking: string): Promise<PolicyDecision> {
    // Deny-by-default: an explicit deny wins, otherwise an allow whose marking,
    // purpose and a role all match; else deny-by-default.
    const applicable = POLICIES.filter(
      (p) => p.marking === marking && (p.purpose === ctx.purpose || p.purpose === '*'),
    );
    const deny = applicable.find(
      (p) => p.effect === 'deny' && (p.roles.includes('*') || p.roles.some((r) => ctx.roles.includes(r))),
    );
    const allow = applicable.find(
      (p) => p.effect === 'allow' && p.roles.some((r) => ctx.roles.includes(r)),
    );
    const audit = this.emit({
      actor: ctx.subject,
      action: `Policy · dry-run (${marking}/${ctx.purpose})`,
      resource: 'governance/pdp',
      pqSignature: 'ML-DSA ✓',
    });
    if (deny) {
      return this.wait({ decision: 'deny', matchedRule: deny.id, reason: `Explicit deny by ${deny.name}.`, auditId: audit.id });
    }
    if (allow) {
      return this.wait({ decision: 'grant', matchedRule: allow.id, reason: `Granted by ${allow.name}.`, auditId: audit.id });
    }
    return this.wait({ decision: 'deny', matchedRule: null, reason: 'Deny by default — no matching allow rule.', auditId: audit.id });
  }

  // ---- platform read models ------------------------------------------------

  listIdentities(): Promise<Identity[]> {
    return this.wait(IDENTITIES.map((i) => ({ ...i })));
  }
  listKeys(): Promise<KeyRecord[]> {
    return this.wait(KEYS.map((k) => ({ ...k })));
  }
  listComputeJobs(): Promise<ComputeJob[]> {
    return this.wait(COMPUTE_JOBS.map((j) => ({ ...j })));
  }
  getObservability(): Promise<SloMetric[]> {
    return this.wait(OBSERVABILITY.map((m) => ({ ...m })));
  }
  getResilience(): Promise<RolloutState[]> {
    return this.wait(RESILIENCE.map((r) => ({ ...r })));
  }
  getContractSurface(): Promise<RpcSurface[]> {
    return this.wait(CONTRACT_SURFACE.map((s) => ({ ...s, rpcs: s.rpcs.map((r) => ({ ...r })) })));
  }
}
