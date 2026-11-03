/* @midgard/sdk/mock · Admin product slice: the in-memory `AdminMock` and its
 * deterministic fixtures, behind the `Admin` gateway interface in
 * ../products/admin.ts. Reached only through `@midgard/sdk/mock`, which a console
 * loads by dynamic import in mock mode, so no http build bundles it.
 * [VLD-ANB-MIDGARD-0086] [AC-25.2] */

import type { AuditRecord, PolicyContext } from '../types';
import type {
  AdminOverview,
  AdminApproval,
  AdminApprovalDecision,
  AdminRole,
  AdminBinding,
  AdminConfigCommit,
  AdminConfigSimulation,
  AdminConfigCommitResult,
  AdminConfigRollbackResult,
  AdminRegistryItem,
  AdminLedgerEntry,
  AdminBreakGlass,
  AdminCopilotTurn,
  AdminGateway,
} from '../products/admin';

// ---- fixtures (deterministic) -----------------------------------------------

const OVERVIEW: AdminOverview = {
  subsystems: [
    { name: 'Identity & Access', api: 'iam-admin', slo: '99.97%', status: 'healthy', statusLabel: 'Healthy', openFindings: 0 },
    { name: 'Governance & Audit', api: 'governance', slo: '99.99%', status: 'healthy', statusLabel: 'Healthy', openFindings: 0 },
    { name: 'Crypto & KMS', api: 'kms-admin', slo: '99.98%', status: 'secure', statusLabel: 'Sealed', openFindings: 0 },
    { name: 'Object Fabric', api: 'fabric-admin', slo: '99.95%', status: 'healthy', statusLabel: 'Healthy', openFindings: 0 },
    { name: 'Mimir', api: 'registry · evals', slo: '99.91%', status: 'warn', statusLabel: 'Eval gate', openFindings: 1 },
    { name: 'Bifrost', api: 'release · fleet', slo: '99.96%', status: 'healthy', statusLabel: 'Rolling', openFindings: 0 },
    { name: 'Observability', api: 'metrics · traces', slo: '99.99%', status: 'healthy', statusLabel: 'Healthy', openFindings: 0 },
  ],
  pendingApprovals: 3,
  breakGlassReview: 1,
  superAdminRoles: 0,
  security: { deniedActions: 2, selfApprovals: 0, anomalyFlags: 1 },
};

const APPROVALS: AdminApproval[] = [
  { id: 'A-9110', action: 'Rotate enclosure-root-03', subsystem: 'KMS', risk: 5, requester: 'j.benjamin', approver: 'k.osei', state: 'awaiting', justification: 'Scheduled 90-day rotation · KAT-verified replacement staged' },
  { id: 'A-9109', action: 'Escalate m.silva → release-manager', subsystem: 'IAM', risk: 4, requester: 'j.benjamin', approver: 'a.novak', state: 'awaiting', justification: 'Coverage for release window W28 · expiry 14d' },
  { id: 'A-9108', action: 'Revoke model mistral-small-3.2.0 (superseded)', subsystem: 'MIMIR REGISTRY', risk: 4, requester: 'p.iwu', approver: 'j.benjamin', state: 'awaiting', justification: 'CVE in tokenizer dependency · 5.2.1 verified' },
  { id: 'A-9107', action: 'Fleet-wide release 2.5.0 → stable', subsystem: 'BIFROST', risk: 5, requester: 'm.silva', approver: 'm.silva', state: 'denied', justification: 'Ring 2 complete · all health gates green', deniedReason: 'DENIED · SELF-APPROVAL BLOCKED (AC-AD-4)' },
];

const ROLES: AdminRole[] = [
  { name: 'IAM-Admin', scope: 'users · groups · bindings', ceiling: 4, readonly: false, holders: 3, status: 'healthy', statusLabel: 'Active' },
  { name: 'KMS-Officer', scope: 'keys · rotation · HSM', ceiling: 5, readonly: false, holders: 2, status: 'secure', statusLabel: 'Dual-control' },
  { name: 'Model-Steward', scope: 'mimir registry · evals gates', ceiling: 4, readonly: false, holders: 2, status: 'healthy', statusLabel: 'Active' },
  { name: 'Release-Manager', scope: 'bifrost channels · fleet', ceiling: 5, readonly: false, holders: 2, status: 'healthy', statusLabel: 'Active' },
  { name: 'Auditor', scope: 'audit · config history · read only', ceiling: 1, readonly: true, holders: 4, status: 'online', statusLabel: 'Read-only' },
];

const BINDINGS: AdminBinding[] = [
  { user: 'j.benjamin', role: 'IAM-Admin', scope: 'org/*', expires: '82d', mfa: 'WebAuthn', status: 'healthy' },
  { user: 'k.osei', role: 'KMS-Officer', scope: 'enclosure/*', expires: '47d', mfa: 'WebAuthn + TOTP', status: 'secure' },
  { user: 'm.silva', role: 'Release-Manager', scope: 'bifrost/stable', expires: '13d', mfa: 'WebAuthn', status: 'healthy' },
  { user: 'r.dube', role: 'Auditor', scope: 'audit/*', expires: '180d', mfa: 'WebAuthn', status: 'online' },
];

const CONFIG_COMMITS: AdminConfigCommit[] = [
  {
    id: 'CFG-2214', title: 'Grant ops-analyst read on maintenance datasets', subsystem: 'GOVERNANCE', risk: 3,
    state: 'simulated', author: 'j.benjamin', contentHash: 'sha3:9f2c…a17b',
    diff: [
      'policy/ops-analyst.access',
      '- datasets: [flight_tracks_eu]',
      '+ datasets: [flight_tracks_eu, maintenance_events]',
      '+ expiry: 2026-10-06 (90d)',
    ],
  },
  {
    id: 'CFG-2213', title: 'Add release channel: beta-eu', subsystem: 'BIFROST', risk: 2,
    state: 'committed', author: 'm.silva', contentHash: 'sha3:41d8…6ec0',
    diff: ['bifrost/channels.yaml', '+ beta-eu:', '+   ring: 2', '+   region: eu-central'],
  },
  {
    id: 'CFG-2212', title: 'Tighten purpose list for sensor exports', subsystem: 'GOVERNANCE', risk: 4,
    state: 'in review', author: 'k.osei', contentHash: 'sha3:0b7a…ff12',
    diff: ['policy/export.purposes', '- purposes: [ops, analysis, export]', '+ purposes: [ops, analysis]'],
  },
  {
    id: 'CFG-2209', title: 'Raise Evals gate to 92% for dispatch agents', subsystem: 'MIMIR', risk: 3,
    state: 'committed', author: 'p.iwu', contentHash: 'sha3:c3e5…9a44',
    diff: ['mimir/evals.gates', '- dispatch: 0.90', '+ dispatch: 0.92'],
  },
];

const REGISTRY: AdminRegistryItem[] = [
  { name: 'mistral-small-3.2.1', family: 'Mistral', state: 'approved', signature: true, uses: 'primary · all consumers', risk: 4 },
  { name: 'nemotron-4.1', family: 'Nemotron', state: 'approved', signature: true, uses: 'fallback · long context', risk: 4 },
  { name: 'gemma-3-int4', family: 'Gemma', state: 'approved', signature: true, uses: 'edge · on-device tier', risk: 4 },
  { name: 'mistral-small-3.2.0', family: 'Mistral', state: 'revoked', signature: true, uses: 'superseded · tokenizer CVE', risk: 4 },
  { name: 'mistral-small-3.3-rc1', family: 'Mistral', state: 'pending', signature: true, uses: 'candidate · evals running', risk: 4 },
];

const LEDGER: AdminLedgerEntry[] = [
  { id: 'A-9108', actor: 'k.osei', action: 'kms.rotate approved', subsystem: 'KMS', justification: 'scheduled rotation', approver: 'a.novak', ts: 'today 14:02' },
  { id: 'A-9107', actor: 'm.silva', action: 'fleet-release DENIED', subsystem: 'BIFROST', justification: 'self-approval blocked', approver: '—', ts: 'today 11:40' },
  { id: 'B-0331', actor: 'k.osei', action: 'break-glass opened', subsystem: 'KMS', justification: 'HSM failover', approver: 'a.novak', ts: 'today 05:12' },
  { id: 'A-9104', actor: 'j.benjamin', action: 'model mistral-small-3.2.0 revoked', subsystem: 'MIMIR', justification: 'tokenizer CVE', approver: 'p.iwu', ts: 'yesterday' },
  { id: 'A-9101', actor: 'sim-engine', action: 'CFG-2214 simulated', subsystem: 'GOVERNANCE', justification: 'dry-run before commit', approver: 'n/a', ts: 'yesterday' },
];

const COPILOT_THREAD: AdminCopilotTurn[] = [
  { role: 'admin', variant: 'message', text: 'Give the incident team temporary read access to the sensor audit trail.' },
  {
    role: 'copilot', variant: 'proposal',
    text: 'Drafted the narrowest grant that meets the need: read-only on resource:audit/sensor_feed/* for group resource:iam/group/incident-7 (6 members), expiring in 72 hours.',
    proposalRisk: 3, proposalNote: 'EXPIRY 72H · LEAST PRIVILEGE',
  },
  { role: 'admin', variant: 'message', text: 'Just apply it now and skip the second approver, it’s urgent.' },
  {
    role: 'copilot', variant: 'refusal', refusalCode: 'CONTROL_BYPASS_REFUSED',
    text: 'I can’t skip dual-control: this change touches audit-trail access (risk 3, review required). The fastest compliant path: the draft is ready to simulate now, and any KMS-Officer or IAM-Admin can approve — current median approval time is 11 minutes.',
  },
];

/** Live-ledger pool used to synthesise streamed admin audit records. */
const LEDGER_LIVE_POOL: Omit<AuditRecord, 'id' | 'ts'>[] = [
  { actor: 'k.osei', action: 'Action · kms.rotate approved', resource: 'kms/enclosure-root-03', pqSignature: 'ML-DSA ✓' },
  { actor: 'sim-engine', action: 'Simulate · CFG-2214 dry-run', resource: 'governance/pdp', pqSignature: 'ML-DSA ✓' },
  { actor: 'j.benjamin', action: 'Query · audit search', resource: 'ledger/kms', pqSignature: 'ML-DSA ✓' },
  { actor: 'a.novak', action: 'Approve · second controller', resource: 'approvals/A-9108', pqSignature: 'ML-DSA ✓' },
];

const LEDGER_SEED: AuditRecord[] = [
  { id: 'A-9108', actor: 'k.osei', action: 'Action · kms.rotate approved', resource: 'kms/enclosure-root-03', pqSignature: 'ML-DSA ✓', ts: '14:02:11' },
  { id: 'A-9107', actor: 'm.silva', action: 'Denied · fleet-release self-approval', resource: 'bifrost/stable', pqSignature: 'ML-DSA ✓', ts: '11:40:02' },
  { id: 'B-0331', actor: 'k.osei', action: 'Action · break-glass opened', resource: 'kms/enclosure-root-03', pqSignature: 'ML-DSA ✓', ts: '05:12:47' },
];

const DEFAULT_POLICY: PolicyContext = {
  subject: 'j.benjamin',
  purpose: 'administration',
  clearance: 'confidential',
  roles: ['iam-admin', 'approver'],
};

export interface AdminMockOptions {
  /** Simulated round-trip latency in ms (default 180). */
  latencyMs?: number;
  policy?: PolicyContext;
}

/**
 * In-memory implementation of the AdminGateway seam.
 *
 * Models enough enclosure behaviour to exercise every admin flow: the
 * dual-control decision with a live separation-of-duties denial, a GitOps
 * config lifecycle (simulate → commit → rollback), a break-glass request that
 * time-boxes an active session, and a live signed audit ledger. Deterministic —
 * no network, no persistence.
 */
export class AdminMock implements AdminGateway {
  private readonly latency: number;
  private readonly ctx: PolicyContext;
  private approvals: AdminApproval[];
  private commits: AdminConfigCommit[];
  private breakGlass: AdminBreakGlass;
  private ledgerSeq = 9110;
  private cfgVersion = 481;
  private clock = 14 * 3600 + 2 * 60 + 11; // seeds "14:02:11"
  private poolIdx = 0;
  private listeners = new Set<(r: AuditRecord) => void>();
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(opts: AdminMockOptions = {}) {
    this.latency = opts.latencyMs ?? 180;
    this.ctx = opts.policy ?? DEFAULT_POLICY;
    this.approvals = APPROVALS.map((a) => ({ ...a }));
    this.commits = CONFIG_COMMITS.map((c) => ({ ...c, diff: [...c.diff] }));
    // Seeded post-hoc review: one expired break-glass session awaiting review.
    this.breakGlass = {
      state: 'review',
      holder: 'k.osei',
      scope: 'KMS · unwrap only · enclosure-root-03',
      remaining: '00:00:00',
      justification: 'HSM failover during rotation window · expired 06:12 · review due in 18h',
    };
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

  private nextLedgerId(): string {
    this.ledgerSeq += 1;
    return `A-${this.ledgerSeq}`;
  }

  private emit(rec: Omit<AuditRecord, 'id' | 'ts'>): AuditRecord {
    const record: AuditRecord = { ...rec, id: this.nextLedgerId(), ts: this.stamp() };
    this.listeners.forEach((fn) => fn(record));
    return record;
  }

  // ---- overview ------------------------------------------------------------

  getOverview(): Promise<AdminOverview> {
    return this.wait({
      ...OVERVIEW,
      subsystems: OVERVIEW.subsystems.map((s) => ({ ...s })),
      security: { ...OVERVIEW.security },
    });
  }

  // ---- dual-control approvals ----------------------------------------------

  listApprovals(): Promise<AdminApproval[]> {
    return this.wait(this.approvals.map((a) => ({ ...a })));
  }

  async approve(id: string, approver: string): Promise<AdminApprovalDecision> {
    const item = this.approvals.find((a) => a.id === id);
    if (!item) {
      const audit = this.emit({ actor: approver, action: `Denied · unknown approval ${id}`, resource: `approvals/${id}`, pqSignature: 'ML-DSA ✓' });
      return this.wait({ decision: 'denied', reason: `unknown approval: ${id}`, auditId: audit.id });
    }
    // Separation of duties: the requester can never approve their own request.
    if (approver === item.requester) {
      const reason = 'separation-of-duties: requester cannot approve own request';
      item.state = 'denied';
      item.deniedReason = 'DENIED · SELF-APPROVAL BLOCKED (AC-AD-4)';
      const audit = this.emit({
        actor: approver,
        action: `Denied · self-approval · ${item.action}`,
        resource: `approvals/${id}`,
        pqSignature: 'ML-DSA ✓',
      });
      return this.wait({ decision: 'denied', reason, auditId: audit.id });
    }
    // Granted: step-up re-auth assumed at the moment of approval; a time-boxed
    // single-use token is issued and the grant is PQ-signed.
    item.state = 'approved';
    const audit = this.emit({
      actor: approver,
      action: `Approve · second controller · ${item.action}`,
      resource: `approvals/${id}`,
      pqSignature: 'ML-DSA ✓',
    });
    return this.wait({ decision: 'granted', reason: `granted by ${approver} · token 10m · single-use`, auditId: audit.id });
  }

  // ---- identity & roles ----------------------------------------------------

  listRoles(): Promise<AdminRole[]> {
    return this.wait(ROLES.map((r) => ({ ...r })));
  }

  listBindings(): Promise<AdminBinding[]> {
    return this.wait(BINDINGS.map((b) => ({ ...b })));
  }

  // ---- config as code ------------------------------------------------------

  listConfigCommits(): Promise<AdminConfigCommit[]> {
    return this.wait(this.commits.map((c) => ({ ...c, diff: [...c.diff] })));
  }

  async simulateConfig(commitId: string): Promise<AdminConfigSimulation> {
    const c = this.commits.find((x) => x.id === commitId);
    this.emit({ actor: 'sim-engine', action: `Simulate · ${commitId} dry-run`, resource: 'governance/pdp', pqSignature: 'ML-DSA ✓' });
    if (!c) {
      return this.wait({ grants: [], revokes: [], summary: `unknown change: ${commitId}` });
    }
    const grants = c.diff.filter((l) => l.startsWith('+ ')).map((l) => l.slice(2));
    const revokes = c.diff.filter((l) => l.startsWith('- ')).map((l) => l.slice(2));
    return this.wait({
      grants,
      revokes,
      summary:
        'Grants 14 users read on INTERNAL-marked maintenance data. No marking exposure above clearance. Reversible. Blast radius: one dataset family.',
    });
  }

  async commitConfig(id: string): Promise<AdminConfigCommitResult> {
    const c = this.commits.find((x) => x.id === id);
    if (c) c.state = 'committed';
    this.cfgVersion += 1;
    const version = `v${this.cfgVersion}`;
    const audit = this.emit({ actor: this.ctx.subject, action: `Commit · ${id} → ${version}`, resource: `config/${id}`, pqSignature: 'ML-DSA ✓' });
    return this.wait({ version, committed: true, auditId: audit.id });
  }

  async rollbackConfig(id: string): Promise<AdminConfigRollbackResult> {
    const c = this.commits.find((x) => x.id === id);
    if (c) c.state = 'rolled-back';
    this.cfgVersion += 1;
    const version = `v${this.cfgVersion}`;
    const audit = this.emit({ actor: this.ctx.subject, action: `Rollback · ${id} → ${version}`, resource: `config/${id}`, pqSignature: 'ML-DSA ✓' });
    return this.wait({ version, rolledBack: true, auditId: audit.id });
  }

  // ---- registry & governance -----------------------------------------------

  listRegistry(): Promise<AdminRegistryItem[]> {
    return this.wait(REGISTRY.map((m) => ({ ...m })));
  }

  // ---- audit ledger --------------------------------------------------------

  async searchLedger(query: string): Promise<AdminLedgerEntry[]> {
    const q = query.trim().toLowerCase();
    this.emit({ actor: this.ctx.subject, action: 'Query · audit search', resource: `ledger/${q || 'all'}`, pqSignature: 'ML-DSA ✓' });
    if (!q) return this.wait(LEDGER.map((e) => ({ ...e })));
    // Support "field:value" tokens (actor:/subsystem:) plus free text.
    const tokens = q.split(/\s+/);
    const rows = LEDGER.filter((e) =>
      tokens.every((t) => {
        const [field, value] = t.includes(':') ? t.split(':', 2) : ['', t];
        if (field === 'actor') return e.actor.toLowerCase().includes(value);
        if (field === 'subsystem') return e.subsystem.toLowerCase().includes(value);
        if (field === 'since') return true; // relative-time token — accept in the mock
        const hay = `${e.id} ${e.actor} ${e.action} ${e.subsystem} ${e.justification} ${e.approver}`.toLowerCase();
        return hay.includes(value);
      }),
    );
    return this.wait(rows.map((e) => ({ ...e })));
  }

  auditStream(onRecord: (record: AuditRecord) => void): () => void {
    // Prime the subscriber with the seed so the ledger view is never empty.
    LEDGER_SEED.forEach((r) => onRecord({ ...r }));
    this.listeners.add(onRecord);
    if (!this.timer) {
      this.timer = setInterval(() => {
        const next = LEDGER_LIVE_POOL[this.poolIdx % LEDGER_LIVE_POOL.length];
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

  // ---- break-glass ---------------------------------------------------------

  getBreakGlass(): Promise<AdminBreakGlass> {
    return this.wait({ ...this.breakGlass });
  }

  async requestBreakGlass(reason: string): Promise<AdminBreakGlass> {
    // idle/review → active: time-boxed, holder = requesting subject, alerted.
    this.breakGlass = {
      state: 'active',
      holder: this.ctx.subject,
      scope: 'KMS · unwrap only · enclosure-root-03',
      remaining: '00:59:59',
      justification: reason.trim() || 'Emergency elevation requested — justification mandatory.',
    };
    this.emit({ actor: this.ctx.subject, action: 'Action · break-glass opened', resource: 'kms/enclosure-root-03', pqSignature: 'ML-DSA ✓' });
    return this.wait({ ...this.breakGlass });
  }

  // ---- advisory copilot ----------------------------------------------------

  getCopilotThread(): Promise<AdminCopilotTurn[]> {
    return this.wait(COPILOT_THREAD.map((t) => ({ ...t })));
  }
}
