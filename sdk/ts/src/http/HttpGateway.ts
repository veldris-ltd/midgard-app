import type { GatewayClient } from '../client';
import type {
  ApplyActionRequest,
  ApplyActionResponse,
  Approval,
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
import { GatewayError, HumanGateRequiredError } from '../mock/MockGateway';

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
  /** The signed-in session's identity, as `whoami` reports it; `policy()` returns it.
   *  Without one the context is empty: the edge authorises from the token either way,
   *  but screens that show who is acting have no one to show. */
  policy?: PolicyContext;
  /** Called when the edge answers 401: the token has expired or no longer verifies
   *  (a gateway restart re-keys its IdP), so the session is over. */
  onUnauthorized?: () => void;
}

const NO_SESSION: PolicyContext = {
  subject: '',
  purpose: 'operations',
  clearance: 'open',
  roles: [],
};

class HttpGateway implements GatewayClient {
  private readonly base: string;
  private readonly token?: string;
  private readonly ctx: PolicyContext;
  private readonly onUnauthorized?: () => void;

  constructor(opts: HttpGatewayOptions) {
    this.base = opts.baseUrl.replace(/\/$/, '');
    this.token = opts.token;
    this.ctx = opts.policy ?? NO_SESSION;
    this.onUnauthorized = opts.onUnauthorized;
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
    if (!res.ok) return this.fail(res, path);
    return (await res.json()) as T;
  }

  private async post<T>(path: string, body: unknown): Promise<T> {
    const res = await fetch(`${this.base}${path}`, {
      method: 'POST',
      headers: this.headers(),
      body: JSON.stringify(body),
    });
    if (!res.ok) return this.fail(res, path);
    return (await res.json()) as T;
  }

  /** Throw the edge's own error message: a human gate the request lacks, or any
   *  other refusal with its status. */
  private async fail(res: Response, path: string): Promise<never> {
    const j = (await res.json().catch(() => ({}))) as { error?: string };
    const message = j.error ?? `http-gateway ${path} → ${res.status}`;
    if (res.status === 401) this.onUnauthorized?.();
    if (res.status === 403 && message.startsWith('Human gate')) {
      throw new HumanGateRequiredError(path, message);
    }
    throw new GatewayError(res.status, message);
  }

  // Object fabric ------------------------------------------------------------
  listObjectTypes(): Promise<ObjectType[]> {
    return this.get<ObjectType[]>('/object-types');
  }
  query(req: QueryRequest): Promise<QueryResponse> {
    return this.post<QueryResponse>('/query', {
      objectType: req.objectType,
      maxResults: req.maxResults ?? 5,
      filter: req.filter,
    });
  }
  applyAction(req: ApplyActionRequest): Promise<ApplyActionResponse> {
    return this.post<ApplyActionResponse>('/actions', {
      actionType: req.actionType,
      params: req.params ?? {},
      humanGateToken: req.humanGateToken,
    });
  }
  listApprovals(): Promise<Approval[]> {
    return this.get<Approval[]>('/approvals');
  }
  requestApproval(actionType: string, target: string): Promise<Approval> {
    return this.post<Approval>('/approvals', { actionType, target });
  }
  decideApproval(id: string, approve: boolean): Promise<Approval> {
    const verb = approve ? 'approve' : 'reject';
    return this.post<Approval>(`/approvals/${encodeURIComponent(id)}/${verb}`, {});
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
