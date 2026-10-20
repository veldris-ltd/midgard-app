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
import { HumanGateRequiredError } from '../mock/MockGateway';

/**
 * The real transport: talks HTTP/JSON to the `http-gateway` Rust service
 * (services/http-gateway), which serves the `GatewayClient` contract from the
 * enclosure. This is the swap-in for `MockGateway` — select it with
 * `VITE_GATEWAY=http` (see each product app's src/main.tsx). Start the edge with:
 *   cargo run -p http-gateway            # listens on 127.0.0.1:7710
 *
 * In production this Rust edge terminates the browser session and forwards to
 * the enclosure over PQ-mTLS. Today it serves object types, query, actions,
 * search-around, audit and policy dry-runs from the real `fabric` + `governance`
 * crates (see enclosure.rs) — seeded in development mode, or durable when served
 * from a state directory created by `mg-admin`.
 */
export interface HttpGatewayOptions {
  /** Base URL of the HTTP/JSON edge, e.g. http://127.0.0.1:7710/api/v1 */
  baseUrl: string;
  /** Bearer of a PQ-signed IdP token, when the edge terminates PQ-mTLS for the browser. */
  token?: string;
  /** Session policy context sent on fabric calls (defaults to the demo operator). */
  policy?: PolicyContext;
}

const DEFAULT_POLICY: PolicyContext = {
  subject: 'j.benjamin',
  purpose: 'operations',
  clearance: 'confidential',
  roles: ['operator', 'analyst'],
};

class HttpGateway implements GatewayClient {
  private readonly base: string;
  private readonly token?: string;
  private readonly ctx: PolicyContext;

  constructor(opts: HttpGatewayOptions) {
    this.base = opts.baseUrl.replace(/\/$/, '');
    this.token = opts.token;
    this.ctx = opts.policy ?? DEFAULT_POLICY;
  }

  policy(): PolicyContext {
    return this.ctx;
  }

  private headers(): HeadersInit {
    const h: Record<string, string> = { 'Content-Type': 'application/json' };
    if (this.token) h.Authorization = `Bearer ${this.token}`;
    return h;
  }

  private async get<T>(path: string): Promise<T> {
    const res = await fetch(`${this.base}${path}`, { headers: this.headers() });
    if (!res.ok) throw new Error(`http-gateway ${path} → ${res.status}`);
    return (await res.json()) as T;
  }

  private async post<T>(path: string, body: unknown): Promise<T> {
    const res = await fetch(`${this.base}${path}`, {
      method: 'POST',
      headers: this.headers(),
      body: JSON.stringify(body),
    });
    if (res.status === 403) {
      const j = (await res.json().catch(() => ({}))) as { error?: string };
      throw new HumanGateRequiredError(j.error ?? path);
    }
    if (!res.ok) throw new Error(`http-gateway ${path} → ${res.status}`);
    return (await res.json()) as T;
  }

  // Object fabric ------------------------------------------------------------
  listObjectTypes(): Promise<ObjectType[]> {
    return this.get<ObjectType[]>('/object-types');
  }
  query(req: QueryRequest): Promise<QueryResponse> {
    return this.post<QueryResponse>('/query', {
      objectType: req.objectType,
      maxResults: req.maxResults ?? 5,
      ...(req.filter ? { filter: req.filter } : {}),
      ...(req.search ? { search: req.search } : {}),
      ...(req.id ? { id: req.id } : {}),
    });
  }
  applyAction(req: ApplyActionRequest): Promise<ApplyActionResponse> {
    return this.post<ApplyActionResponse>('/actions', {
      actionType: req.actionType,
      params: req.params ?? {},
      humanGateToken: req.humanGateToken,
    });
  }
  searchAround(seedId: string, hops: number): Promise<SearchGraph> {
    return this.post<SearchGraph>('/search-around', { seedId, hops });
  }

  // Governance / audit -------------------------------------------------------
  getAudit(limit = 50): Promise<AuditRecord[]> {
    return this.get<AuditRecord[]>(`/audit?limit=${limit}`);
  }
  /** The edge is request/response; the live stream is a poll of GET /audit,
   *  emitting records not seen before (newest last, chronological). */
  auditStream(onRecord: (record: AuditRecord) => void): () => void {
    const seen = new Set<string>();
    let stopped = false;
    const tick = async () => {
      try {
        const recent = await this.getAudit(40);
        // recent is newest-first; emit unseen in chronological order.
        for (const rec of [...recent].reverse()) {
          if (!seen.has(rec.id)) {
            seen.add(rec.id);
            if (!stopped) onRecord(rec);
          }
        }
      } catch {
        /* transient; retry on next tick */
      }
    };
    // Prime the seen-set silently, then poll for new records.
    this.getAudit(40)
      .then((r) => r.forEach((x) => seen.add(x.id)))
      .catch(() => {});
    const timer = setInterval(tick, 4200);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }

  // Policies & markings ------------------------------------------------------
  listPolicies(): Promise<PolicyRule[]> {
    return this.get<PolicyRule[]>('/policies');
  }
  simulatePolicy(ctx: PolicyContext, marking: string): Promise<PolicyDecision> {
    return this.post<PolicyDecision>('/policy/simulate', {
      subject: ctx.subject,
      purpose: ctx.purpose,
      clearance: ctx.clearance,
      roles: ctx.roles,
      marking,
    });
  }

  // Platform read models -----------------------------------------------------
  listIdentities(): Promise<Identity[]> {
    return this.get<Identity[]>('/identities');
  }
  listKeys(): Promise<KeyRecord[]> {
    return this.get<KeyRecord[]>('/keys');
  }
  listComputeJobs(): Promise<ComputeJob[]> {
    return this.get<ComputeJob[]>('/compute');
  }
  getObservability(): Promise<SloMetric[]> {
    return this.get<SloMetric[]>('/observability');
  }
  getResilience(): Promise<RolloutState[]> {
    return this.get<RolloutState[]>('/resilience');
  }
  getContractSurface(): Promise<RpcSurface[]> {
    return this.get<RpcSurface[]>('/sdk-surface');
  }
}

/** Construct a real HTTP/JSON gateway client. */
export function createHttpGateway(opts: HttpGatewayOptions): GatewayClient {
  return new HttpGateway(opts);
}
