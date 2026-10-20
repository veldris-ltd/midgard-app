/* The Mimir console's live client: the model plane and session traces read from the
 * running `mimir-server` through the `http-gateway` edge (`GET /mimir/models`,
 * `GET /mimir/sessions`), which signs the user in, authorises and audits the read, and
 * reads Mimir's observer port over PQ-mTLS. Everything else (agents, Logic, evals,
 * guardrails, feedback, the audit stream) is still the in-enclosure simulation of
 * MimirMock until the agent runtime is a service. [FR-MI-8] [FR-MI-1] [FR-MI-3] */
import type { MimirGateway, MimirModel, MimirSession, MimirTraceStep } from './mimir';
import { MimirMock } from './mimir';

export interface MimirHttpOptions {
  /** Base URL of the HTTP/JSON edge, e.g. http://127.0.0.1:7710/api/v1 */
  baseUrl: string;
  /** Bearer of the PQ-signed token from `login`. */
  token: string;
  /** Called when the gateway answers 401 (the session expired). */
  onUnauthorized?: () => void;
}

interface WireSession {
  audit_seq: number; at: number; caller: string; purpose: string; workload: string; outcome: string;
  model: string | null; prompt_tokens: number; tokens: number; first_token_ms: number; total_ms: number;
  detail: string | null;
  ledger: { seq: number; actor: string; action: string; resource: string; ts: string; entry_hash: string; sig_alg: string } | null;
}

const secs = (ms: number) => (ms >= 10000 ? `${Math.round(ms / 1000)}s` : `${(ms / 1000).toFixed(2)}s`);

export class MimirHttp extends MimirMock implements MimirGateway {
  private readonly base: string;
  private readonly token: string;
  private readonly onUnauthorized?: () => void;

  constructor(opts: MimirHttpOptions) {
    super();
    this.base = opts.baseUrl.replace(/\/$/, '');
    this.token = opts.token;
    this.onUnauthorized = opts.onUnauthorized;
  }

  private async get(path: string): Promise<any> {
    const res = await fetch(`${this.base}${path}`, { headers: { authorization: `Bearer ${this.token}` } });
    if (res.status === 401) {
      this.onUnauthorized?.();
      throw new Error('session expired: sign in again');
    }
    const body = await res.json().catch(() => null);
    if (!res.ok) throw new Error(body?.error ?? `HTTP ${res.status}`);
    return body;
  }

  private async wireSessions(limit = 200): Promise<WireSession[]> {
    const b = await this.get(`/mimir/sessions?limit=${limit}`);
    return Array.isArray(b?.sessions) ? b.sessions : [];
  }

  async listModels(): Promise<MimirModel[]> {
    const [d, sessions] = await Promise.all([this.get('/mimir/models'), this.wireSessions(500).catch(() => [])]);
    const m = d?.model ?? {};
    const node = d?.node ?? {};
    const consumers = new Map<string, { caller: string; calls: number; lastAt: number; purposes: Set<string> }>();
    for (const s of sessions) {
      const c = consumers.get(s.caller) ?? { caller: s.caller, calls: 0, lastAt: 0, purposes: new Set<string>() };
      c.calls++;
      c.lastAt = Math.max(c.lastAt, s.at);
      c.purposes.add(s.purpose);
      consumers.set(s.caller, c);
    }
    const healthy = d?.sidecar_healthy !== false;
    const hw = node.gpus > 0 ? `${node.gpus}× GPU` : `CPU · ${node.cpu_cores ?? '?'} CORES`;
    return [{
      name: String(m.id ?? d?.engine ?? 'model'),
      role: `${m.family ?? ''} ${m.version ?? ''} · ${m.licence ?? 'licence unknown'} · served by ${m.engine ?? d?.engine ?? '?'}`.trim(),
      quant: String(m.quantisation ?? '—'),
      ctx: d?.ctx ? String(d.ctx) : '—',
      gpu: hw,
      tps: d?.tokens_per_s != null ? `${d.tokens_per_s} tok/s` : '— tok/s',
      status: healthy ? 'healthy' : 'danger',
      statusLabel: healthy ? 'Serving' : 'Sidecar down',
      live: {
        family: String(m.family ?? ''), version: String(m.version ?? ''), file: m.file ?? null, sizeBytes: m.size_bytes ?? null,
        sha256: String(m.sha256 ?? ''), licence: String(m.licence ?? ''), osiApproved: m.osi_approved === true,
        redistributable: m.redistributable === true, signature: String(m.signature ?? ''), engine: String(m.engine ?? d?.engine ?? ''),
        sidecarHealthy: typeof d?.sidecar_healthy === 'boolean' ? d.sidecar_healthy : null,
        node: `${hw}${node.in_container ? ' · in a container' : ''}`,
        firstTokenMs: d?.avg_first_token_ms ?? null, avgTotalMs: d?.avg_total_ms ?? null,
        calls: d?.calls ?? 0, served: d?.served ?? 0, failed: d?.failed ?? 0, denied: d?.denied ?? 0, ledgerEntries: d?.ledger_entries ?? 0,
        consumers: [...consumers.values()].sort((a, b) => b.calls - a.calls)
          .map((c) => ({ caller: c.caller, calls: c.calls, lastAt: c.lastAt, purposes: [...c.purposes] })),
      },
    }];
  }

  async listSessions(): Promise<MimirSession[]> {
    return (await this.wireSessions()).map(toSession);
  }

  async getSessionTrace(id: string): Promise<MimirSession> {
    const all = await this.wireSessions(500);
    const hit = all.find((s) => `S-${s.audit_seq}` === id) ?? all[0];
    if (!hit) throw new Error('no sessions yet: nothing has called Mimir since it started');
    return toSession(hit);
  }
}

function toSession(s: WireSession): MimirSession {
  const gen = Math.max(0, s.total_ms - s.first_token_ms);
  const steps: MimirTraceStep[] = [
    { kind: 'guard', label: `Channel · ${s.caller}`, detail: `PQ-mTLS peer ${s.caller}, purpose "${s.purpose}", workload ${s.workload}.`, ms: null, ok: true },
    { kind: 'eval', label: s.ledger ? `${s.ledger.action} · ledger #${s.ledger.seq}` : `Policy decision · ledger #${s.audit_seq}`,
      detail: s.ledger
        ? `One ${s.ledger.sig_alg} signed ledger entry on ${s.ledger.resource}: hash ${s.ledger.entry_hash.slice(0, 16)}…`
        : 'The signed entry is older than the newest the plane reports.',
      ms: null, ok: s.outcome !== 'denied' },
  ];
  if (s.outcome !== 'denied') {
    steps.push(
      { kind: 'act', label: `Prefill · ${s.prompt_tokens} prompt tokens`, detail: `${s.model ?? 'model'} read the prompt; first token after ${s.first_token_ms} ms.`, ms: s.first_token_ms, ok: s.outcome === 'served' },
      { kind: 'observe', label: s.outcome === 'served' ? `Generated ${s.tokens} tokens` : 'Generation failed',
        detail: s.outcome === 'served'
          ? `${gen > 0 ? (s.tokens * 1000 / gen).toFixed(1) : '—'} tokens/s; ${secs(s.total_ms)} in all.`
          : (s.detail ?? 'the engine failed'),
        ms: gen, ok: s.outcome === 'served' },
    );
  }
  return {
    id: `S-${s.audit_seq}`,
    agent: s.caller,
    policyVersion: s.ledger ? `ledger #${s.ledger.seq}` : `ledger #${s.audit_seq}`,
    firstToken: `${s.first_token_ms} ms`,
    total: secs(s.total_ms),
    steps,
    memory: [],
    live: {
      caller: s.caller, purpose: s.purpose, at: s.at, outcome: s.outcome as 'served' | 'denied' | 'failed', model: s.model,
      promptTokens: s.prompt_tokens, tokens: s.tokens,
      ledger: s.ledger ? { seq: s.ledger.seq, action: s.ledger.action, resource: s.ledger.resource, entryHash: s.ledger.entry_hash, sigAlg: s.ledger.sig_alg } : null,
    },
  };
}
