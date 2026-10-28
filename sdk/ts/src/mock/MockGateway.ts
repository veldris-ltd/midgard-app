import type { GatewayClient } from '../client';
import type {
  ApplyActionRequest,
  ApplyActionResponse,
  Approval,
  ApprovalStatus,
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

/** Error thrown when a gated action is applied without a current human-gate
 *  approval. `message` is the gateway's own wording, where it gave one. */
export class HumanGateRequiredError extends Error {
  constructor(actionType: string, message?: string) {
    super(message ?? `Human gate required for action "${actionType}"`);
    this.name = 'HumanGateRequiredError';
  }
}

/** A request the gateway refused, with its HTTP status and the gateway's message. */
export class GatewayError extends Error {
  readonly status: number;
  constructor(status: number, message: string) {
    super(message);
    this.name = 'GatewayError';
    this.status = status;
  }
}

export interface MockGatewayOptions {
  /** Simulated round-trip latency in ms (default 180). */
  latencyMs?: number;
  policy?: PolicyContext;
}

/** How long an approved human gate can be redeemed (seconds), as at the edge. */
const APPROVAL_TTL = 900;
/** How long the mock's simulated second person takes to approve a request (ms). */
const MOCK_APPROVER_DELAY_MS = 3000;

const nowSecs = () => Math.floor(Date.now() / 1000);

/** An approval's status now: an approved gate past its expiry has expired. */
function status(a: Approval): ApprovalStatus {
  return a.status === 'approved' && (a.expiresAt ?? 0) <= nowSecs() ? 'expired' : a.status;
}

/** The one object an action names, as the edge requires: a wildcard or a missing
 *  target is refused. */
function oneTarget(objectType: string, target: string | undefined): string {
  if (!target || target.endsWith('/*')) {
    throw new GatewayError(400, `an action applies to one object: name it as "target":"${objectType}/<id>"`);
  }
  return target;
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
  private approvals: Approval[] = [];
  private approvalSeq = 0;
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
    // The mock's synthesised objects carry no searchable properties, so conditions
    // and ordering are accepted and ignored; paging still applies.
    const page = results.slice(req.offset ?? 0);
    return this.wait({ results: page, total: results.length, offset: req.offset ?? 0, limit: n, auditId: audit.id });
  }

  async applyAction(req: ApplyActionRequest): Promise<ApplyActionResponse> {
    const type = OBJECT_TYPES.find((t) => t.actions.some((a) => a.name === req.actionType));
    const action = type?.actions.find((a) => a.name === req.actionType);
    const target = oneTarget(type?.apiName ?? 'fabric', req.params?.target);
    if (action?.humanGated) {
      if (!req.humanGateToken) throw new HumanGateRequiredError(req.actionType);
      const approval = this.approvals.find(
        (a) =>
          a.id === req.humanGateToken &&
          a.actionType === req.actionType &&
          a.target === target &&
          a.requester === this.ctx.subject &&
          status(a) === 'approved',
      );
      if (!approval) {
        throw new HumanGateRequiredError(
          req.actionType,
          `Human gate token is not a current approval of "${req.actionType}" on ${target} for this user`,
        );
      }
      approval.status = 'used';
    }
    const audit = this.emit({
      actor: this.ctx.subject,
      action: `Action · ${req.actionType}`,
      resource: target,
      pqSignature: 'ML-DSA ✓',
    });
    return this.wait({ auditId: audit.id, applied: true });
  }

  listApprovals(): Promise<Approval[]> {
    return this.wait(this.approvals.map((a) => ({ ...a, status: status(a), mine: a.requester === this.ctx.subject })).reverse());
  }

  async requestApproval(actionType: string, target: string): Promise<Approval> {
    const type = OBJECT_TYPES.find((t) => t.actions.some((a) => a.name === actionType));
    const action = type?.actions.find((a) => a.name === actionType);
    if (!type || !action) throw new GatewayError(404, `unknown action type: ${actionType}`);
    if (!action.humanGated) throw new GatewayError(400, `action "${actionType}" needs no approval`);
    const approval: Approval = {
      id: `APR-${++this.approvalSeq}`,
      actionType,
      target: oneTarget(type.apiName, target),
      requester: this.ctx.subject,
      requestedAt: nowSecs(),
      status: 'pending',
      mine: true,
    };
    this.approvals.push(approval);
    // No second person signs in to the mock, so a simulated one approves shortly.
    setTimeout(() => {
      if (approval.status === 'pending') this.decideApproval(approval.id, true).catch(() => {});
    }, MOCK_APPROVER_DELAY_MS);
    return this.wait({ ...approval });
  }

  /** The mock has one session, so it plays the second person: a decision here is
   *  made by "approver", never by the requester. */
  async decideApproval(id: string, approve: boolean): Promise<Approval> {
    const approval = this.approvals.find((a) => a.id === id);
    if (!approval) throw new GatewayError(404, `unknown approval: ${id}`);
    if (approval.status !== 'pending') {
      throw new GatewayError(409, `approval ${id} is already ${status(approval)}`);
    }
    approval.approver = 'approver';
    if (approve) {
      approval.status = 'approved';
      approval.expiresAt = nowSecs() + APPROVAL_TTL;
    } else {
      approval.status = 'rejected';
    }
    this.emit({
      actor: 'approver',
      action: `Approve · ${approval.actionType}`,
      resource: approval.target,
      pqSignature: 'ML-DSA ✓',
    });
    return this.wait({ ...approval });
  }

  async searchAround(seedId: string, hops: number): Promise<SearchGraph> {
    const audit = this.emit({
      actor: this.ctx.subject,
      action: 'Query · search-around',
      resource: seedId,
      pqSignature: 'ML-DSA ✓',
    });
    const nodes: SearchGraph['nodes'] = [
      { id: seedId, label: seedId, typeName: 'Aircraft', marking: 'confidential', hop: 0, propertyValues: { status: 'active' } },
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
        nodes.push({ id, label: `${s.typeName} ${hop}.${i + 1}`, typeName: s.typeName, marking: s.marking, hop, propertyValues: { status: 'active' } });
        const parent = hop === 1 ? seedId : `n${hop - 1}-0`;
        edges.push({ from: parent, to: id, label: s.label });
      }
    }
    return this.wait({ seed: seedId, nodes, edges, truncated: false, nodeLimit: 60, seedObjects: 1, auditId: audit.id });
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
