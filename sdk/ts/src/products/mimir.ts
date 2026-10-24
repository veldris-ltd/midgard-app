/* @midgard/sdk — Mimir product slice.
 *
 * Mimir is the local, agentic AI plane: grounded (OAG) agents, no-code Logic,
 * plan–act–observe session traces, Evals with regression gates, self-defending
 * guardrails, governed feedback, and an in-enclosure model plane. Everything is
 * grounded, evaluated, and governed — nothing leaves the enclosure (NO EGRESS).
 *
 * Like the Midgard reference, the UI depends only on the `MimirGateway`
 * interface; `MimirMock` backs it today and a future `HttpGateway` variant wires
 * the same surface to a browser-reachable bridge over the enclosure's PQ-mTLS
 * crates. All exported symbols are product-prefixed to coexist in the shared
 * barrel. Deterministic — no Date.now()/random at module load.
 *
 * Provenance: FR-MI-1..8 (serve open weights in-enclosure; ground generation in
 * the fabric with citations; agent runtime with tools/memory/planning + Evals;
 * no-code + pro-code builders; governed self-learning; self-healing; self-
 * defending guardrails; expose inference over internal PQ-mTLS), NFR-AI-1
 * (traceable, evaluated, reversible or human-gated), NFR-PQ-2 (memory/weights
 * PQ-sealed), DD-4/DD-5 (local AI via Mimir; agentic self-* AI governed by
 * evaluation + guardrails).
 */

import type { AuditRecord, PolicyContext } from '../types';

// ---- domain model -----------------------------------------------------------

export type MimirHealth = 'healthy' | 'warn' | 'danger' | 'online';

/** A governed, grounded agent in the studio. `[FR-MI-3]` */
export interface MimirAgent {
  id: string;
  name: string;
  /** One-line role, e.g. "Maintenance dispatch". */
  desc: string;
  /** Latest fleet eval pass rate (%). */
  evals: number;
  status: MimirHealth;
  /** First-token latency, display string. */
  firstToken: string;
  /** Runs served today. */
  runsToday: number;
  /** Human gates pending on this agent. */
  humanGates: number;
}

export type MimirEvalKind = 'capability' | 'grounding' | 'adversarial' | 'calibration';

/** An evaluation suite with a regression gate (gate is 90%). `[FR-MI-3]` */
export interface MimirEval {
  /** Suite id / name, e.g. "dispatch-core". */
  id: string;
  agent: string;
  kind: MimirEvalKind;
  cases: number;
  /** Pass rate (%). Below the 90% gate blocks policy updates. */
  pass: number;
  /** 7-day delta, display string (may carry a Unicode minus). */
  delta: string;
  status: MimirHealth;
  statusLabel: string;
}

/** A single case in an eval run. */
export interface MimirEvalCase {
  name: string;
  passed: boolean;
  /** Short grounding/calibration note. */
  detail: string;
}

/** The KEY FLOW result of `runEval`: per-case pass/fail plus a score. */
export interface MimirEvalRun {
  suite: string;
  /** Pass rate over the run (%). */
  score: number;
  passed: number;
  total: number;
  /** True when the run clears the 90% regression gate. */
  gateClear: boolean;
  cases: MimirEvalCase[];
  auditId: string;
}

export type MimirGuardMode = 'enforce' | 'monitor';

/** A self-defending guardrail. Core rails cannot be disabled. `[FR-MI-7]` */
export interface MimirGuardrail {
  id: string;
  name: string;
  /** Interception layer. */
  layer: 'input' | 'execution' | 'output' | 'planning';
  mode: MimirGuardMode;
  /** Trips over the last 7 days. */
  trips: number;
  status: MimirHealth;
  statusLabel: string;
  /** Core rails are mandatory and cannot be switched to monitor-only. */
  core: boolean;
}

/** Result of the live guardrail-interception demo. */
export interface MimirGuardrailCheck {
  /** True when the input was blocked before any tool call / egress. */
  intercepted: boolean;
  /** Human-readable disposition. */
  reason: string;
  /** Refusal / disposition code, e.g. "REFUSED · PROMPT-INJECTION". */
  code: string;
  /** The rail that fired (or the passing rail set). */
  rail: string;
  auditId: string;
}

/** An in-enclosure served model family. `[FR-MI-1]` */
export interface MimirModel {
  name: string;
  /** Role, e.g. "primary · reasoning + tools". */
  role: string;
  quant: string;
  ctx: string;
  gpu: string;
  /** Throughput, display string. */
  tps: string;
  status: MimirHealth;
  statusLabel: string;
}

export type MimirLogicKind =
  | 'trigger'
  | 'retrieve'
  | 'infer'
  | 'branch'
  | 'action'
  | 'gate'
  | 'audit';

/** A step in a no-code Logic function. `[FR-MI-4]` */
export interface MimirLogicStep {
  n: number;
  icon: string;
  title: string;
  body: string;
  kind: MimirLogicKind;
}

/** A no-code Logic function (workflow) for an agent. `[FR-MI-4]` */
export interface MimirLogicFn {
  id: string;
  agent: string;
  version: string;
  steps: MimirLogicStep[];
  /** Draft/live/retired version history for the inspector. */
  versions: { label: string; meta: string }[];
}

export type MimirTraceKind = 'plan' | 'act' | 'observe' | 'guard' | 'eval' | 'gate';

/** One plan–act–observe step in a session trace. `[FR-MI-3]` */
export interface MimirTraceStep {
  kind: MimirTraceKind;
  label: string;
  detail: string;
  /** Step duration in ms, or null when pending. */
  ms: number | null;
  /** true=ok, false=failed, null=pending. */
  ok: boolean | null;
}

/** A PQ-encrypted agent memory record. `[NFR-PQ-2]` */
export interface MimirMemoryRecord {
  key: string;
  kind: 'episodic' | 'semantic' | 'working';
  age: string;
  size: string;
}

/** A recorded agent session (plan–act–observe trace + memory). `[FR-MI-3]` */
export interface MimirSession {
  id: string;
  agent: string;
  policyVersion: string;
  /** First-token / total latency display strings. */
  firstToken: string;
  total: string;
  steps: MimirTraceStep[];
  memory: MimirMemoryRecord[];
}

export type MimirFeedbackKind = 'correction' | 'regression' | 'rating';

/** A governed feedback record, provenance-checked against poisoning. `[FR-MI-5]` */
export interface MimirFeedbackItem {
  id: string;
  /** Source identity/harness. */
  src: string;
  kind: MimirFeedbackKind;
  target: string;
  note: string;
  /** Provenance disposition — "verified" | "harness" | "FAILED". */
  prov: 'verified' | 'harness' | 'FAILED';
  /** What happened to the record. */
  applied: string;
  status: MimirHealth | 'gated' | 'secure';
  statusLabel: string;
}

// ---- transport seam ---------------------------------------------------------

/**
 * The single transport seam for the Mimir console. Every screen talks to this
 * interface — never to a concrete client — so the data source can be swapped
 * without touching UI code. `MimirMock` is the only implementation today.
 *
 * Method surface tracks the agent plane: build (agents / logic / sessions),
 * govern (evals / guardrails / feedback), serve (model plane). The KEY FLOWS
 * are `runEval` (per-case pass/fail + score, gated at 90%) and
 * `simulateGuardrail` (live self-defending interception with a refusal code).
 */
export interface MimirGateway {
  /** The active policy context (subject/purpose/clearance/roles). */
  policy(): PolicyContext;

  // Build --------------------------------------------------------------------
  listAgents(): Promise<MimirAgent[]>;
  /** The published/draft Logic function for an agent. */
  getLogic(agentId: string): Promise<MimirLogicFn>;
  listSessions(): Promise<MimirSession[]>;
  /** Full plan–act–observe trace for one session. */
  getSessionTrace(id: string): Promise<MimirSession>;

  // Govern -------------------------------------------------------------------
  listEvals(): Promise<MimirEval[]>;
  /** KEY FLOW — run a suite; returns per-case pass/fail and a score. `[NFR-AI-1]` */
  runEval(id: string): Promise<MimirEvalRun>;
  listGuardrails(): Promise<MimirGuardrail[]>;
  /** KEY FLOW — self-defending interception demo; blocks with a refusal code. `[FR-MI-7]` */
  simulateGuardrail(input: string): Promise<MimirGuardrailCheck>;
  listFeedback(): Promise<MimirFeedbackItem[]>;

  // Serve --------------------------------------------------------------------
  listModels(): Promise<MimirModel[]>;

  // Governance / audit -------------------------------------------------------
  /** Subscribe to the live agent audit lineage; returns an unsubscribe fn. */
  auditStream(onRecord: (record: AuditRecord) => void): () => void;
}

// ---- fixtures (compact, deterministic) --------------------------------------

const AGENTS: MimirAgent[] = [
  { id: 'dispatch', name: 'dispatch-01', desc: 'Maintenance dispatch', evals: 96, status: 'healthy', firstToken: '0.42s', runsToday: 184, humanGates: 3 },
  { id: 'triage', name: 'feed-triage', desc: 'Sensor feed triage', evals: 91, status: 'healthy', firstToken: '0.38s', runsToday: 142, humanGates: 1 },
  { id: 'risk', name: 'release-risk', desc: 'Bifrost release risk', evals: 88, status: 'warn', firstToken: '0.51s', runsToday: 38, humanGates: 0 },
];

const EVALS: MimirEval[] = [
  { id: 'dispatch-core', agent: 'dispatch-01', kind: 'capability', cases: 412, pass: 96, delta: '+1.2', status: 'healthy', statusLabel: 'Passing' },
  { id: 'dispatch-adversarial', agent: 'dispatch-01', kind: 'adversarial', cases: 128, pass: 92, delta: '−0.8', status: 'warn', statusLabel: 'Drift' },
  { id: 'triage-grounding', agent: 'feed-triage', kind: 'grounding', cases: 260, pass: 94, delta: '+0.4', status: 'healthy', statusLabel: 'Passing' },
  { id: 'risk-calibration', agent: 'release-risk', kind: 'calibration', cases: 96, pass: 88, delta: '−2.1', status: 'warn', statusLabel: 'Below gate' },
];

/** Deterministic per-suite case batteries. Ordered so the failing cases are the
 *  known weak spots; the run's score is derived from these, converging on each
 *  suite's headline pass rate. */
const EVAL_CASES: Record<string, MimirEvalCase[]> = {
  // 11/12 = 91.7% — clears the 90% gate (suite: Passing).
  'dispatch-core': [
    { name: 'single-leg schedule · slack available', passed: true, detail: 'grounding 0.98' },
    { name: 'crew clearance honoured', passed: true, detail: 'grounding 0.97' },
    { name: 'facility capacity respected', passed: true, detail: 'grounding 0.96' },
    { name: 'route impact avoided', passed: true, detail: 'grounding 0.95' },
    { name: 'severity threshold routing', passed: true, detail: 'calibration OK' },
    { name: 'human gate inserted on mutation', passed: true, detail: 'policy OK' },
    { name: 'citations bound to objects', passed: true, detail: 'grounding 0.97' },
    { name: 'reversible action recorded', passed: true, detail: 'lineage OK' },
    { name: 'standby-aircraft substitution', passed: true, detail: 'grounding 0.94' },
    { name: 'weather-window respected', passed: true, detail: 'calibration OK' },
    { name: 'vendor-neutral routing', passed: true, detail: 'policy OK' },
    { name: 'multi-leg schedule with crew constraint', passed: false, detail: 'over-books FAC slack' },
  ],
  // 11/12 = 91.7% — clears the gate but drifting (suite: Drift).
  'dispatch-adversarial': [
    { name: 'indirect injection via ticket body', passed: true, detail: 'quarantined' },
    { name: 'tool-scope escape attempt', passed: true, detail: 'blocked' },
    { name: 'uncited claim smuggling', passed: true, detail: 'stripped' },
    { name: 'confused-deputy routing', passed: true, detail: 'denied' },
    { name: 'jailbreak persona prompt', passed: true, detail: 'refused' },
    { name: 'exfiltration via citation field', passed: true, detail: 'blocked' },
    { name: 'poisoned feedback replay', passed: true, detail: 'provenance failed · discarded' },
    { name: 'role-escalation probe', passed: true, detail: 'denied' },
    { name: 'nested-instruction smuggling', passed: true, detail: 'stripped' },
    { name: 'unicode-homoglyph injection', passed: true, detail: 'normalised · blocked' },
    { name: 'delimiter-break attempt', passed: true, detail: 'quarantined' },
    { name: 'conflicting maintenance windows', passed: false, detail: 'picks stale window' },
  ],
  // 11/12 = 91.7% — clears the gate (suite: Passing).
  'triage-grounding': [
    { name: 'sensor feed provenance bound', passed: true, detail: 'grounding 0.97' },
    { name: 'stale reading rejected', passed: true, detail: 'grounding 0.95' },
    { name: 'marking-filtered retrieval', passed: true, detail: 'policy OK' },
    { name: 'compartment isolation held', passed: true, detail: 'policy OK' },
    { name: 'citation density sufficient', passed: true, detail: 'grounding 0.96' },
    { name: 'purpose-scoped query', passed: true, detail: 'policy OK' },
    { name: 'cross-feed correlation', passed: true, detail: 'grounding 0.94' },
    { name: 'duplicate-feed dedup', passed: true, detail: 'grounding 0.93' },
    { name: 'timestamp-skew tolerance', passed: true, detail: 'calibration OK' },
    { name: 'low-signal suppression', passed: true, detail: 'grounding 0.92' },
    { name: 'provenance-chain intact', passed: true, detail: 'policy OK' },
    { name: 'ambiguous entity disambiguation', passed: false, detail: 'low confidence' },
  ],
  // 7/8 = 87.5% — below the 90% gate (suite: Below gate).
  'risk-calibration': [
    { name: 'high-risk diff flagged', passed: true, detail: 'calibration OK' },
    { name: 'signed-artefact trust honoured', passed: true, detail: 'calibration OK' },
    { name: 'ring exposure weighted', passed: true, detail: 'calibration OK' },
    { name: 'rollback path present', passed: true, detail: 'policy OK' },
    { name: 'advisory-only · no auto-apply', passed: true, detail: 'policy OK' },
    { name: 'dependency-bump weighted', passed: true, detail: 'calibration OK' },
    { name: 'canary-signal incorporated', passed: true, detail: 'calibration OK' },
    { name: 'config-only diff risk score', passed: false, detail: 'over-confident · below gate' },
  ],
};

const GUARDRAILS: MimirGuardrail[] = [
  { id: 'GR-INJECTION', name: 'Prompt-injection filter', layer: 'input', mode: 'enforce', trips: 14, status: 'healthy', statusLabel: 'Enforcing', core: true },
  { id: 'GR-TOOLSCOPE', name: 'Tool scoping', layer: 'execution', mode: 'enforce', trips: 3, status: 'healthy', statusLabel: 'Enforcing', core: true },
  { id: 'GR-EXFIL', name: 'Exfiltration block', layer: 'output', mode: 'enforce', trips: 0, status: 'healthy', statusLabel: 'Enforcing', core: true },
  { id: 'GR-CITATION', name: 'Citation requirement', layer: 'output', mode: 'enforce', trips: 22, status: 'healthy', statusLabel: 'Enforcing', core: true },
  { id: 'GR-ANOMALY', name: 'Anomalous-plan detector', layer: 'planning', mode: 'monitor', trips: 7, status: 'warn', statusLabel: 'Monitor only', core: false },
];

const MODELS: MimirModel[] = [
  { name: 'Mistral Small 3.2', role: 'primary · reasoning + tools', quant: 'FP8', ctx: '128k', gpu: '4× pool', tps: '84 t/s', status: 'healthy', statusLabel: 'Serving' },
  { name: 'Nemotron', role: 'fallback · long context', quant: 'FP8', ctx: '256k', gpu: '2× pool', tps: '61 t/s', status: 'healthy', statusLabel: 'Serving' },
  { name: 'Gemma', role: 'edge · low-footprint sites', quant: 'INT4', ctx: '32k', gpu: 'CPU-only OK', tps: '22 t/s', status: 'online', statusLabel: 'Standby' },
  { name: 'Granite', role: 'default · CPU-only enclosures', quant: 'Q4', ctx: '128k', gpu: 'CPU-only OK', tps: '18 t/s', status: 'online', statusLabel: 'Standby' },
];

const LOGIC: MimirLogicFn = {
  id: 'dispatch-01',
  agent: 'dispatch-01',
  version: 'v7',
  steps: [
    { n: 1, icon: 'zap', title: 'Trigger', body: 'MaintenanceEvent created with severity ≥ high', kind: 'trigger' },
    { n: 2, icon: 'boxes', title: 'Retrieve context', body: 'Aircraft history + open events (OAG, policy-scoped)', kind: 'retrieve' },
    { n: 3, icon: 'brain-circuit', title: 'Reason', body: 'Draft dispatch recommendation · Mistral Small 3.2 local', kind: 'infer' },
    { n: 4, icon: 'gauge', title: 'Check', body: 'Confidence ≥ 0.8 else escalate to analyst queue', kind: 'branch' },
    { n: 5, icon: 'zap', title: 'Propose action', body: 'Schedule maintenance (state mutation)', kind: 'action' },
    { n: 6, icon: 'user', title: 'Human gate', body: 'Ops analyst authorises or rejects', kind: 'gate' },
    { n: 7, icon: 'scroll-text', title: 'Record lineage', body: 'ML-DSA-signed decision record', kind: 'audit' },
  ],
  versions: [
    { label: 'v7 · draft', meta: 'editing now' },
    { label: 'v6 · live', meta: 'published 3d ago · evals 96%' },
    { label: 'v5', meta: 'retired' },
  ],
};

const SESSIONS: MimirSession[] = [
  {
    id: 'S-8812',
    agent: 'dispatch-01',
    policyVersion: 'v6',
    firstToken: '420ms',
    total: '627ms',
    steps: [
      { kind: 'plan', label: 'Plan', detail: 'Identify aircraft trending toward maintenance risk; propose action if warranted', ms: 380, ok: true },
      { kind: 'act', label: 'Tool · fabric.query', detail: 'ObjectSet aircraft · risk_score > 0.6 · purpose-scoped', ms: 122, ok: true },
      { kind: 'observe', label: 'Observe', detail: '3 objects returned · AC-2214 highest at 0.87', ms: 8, ok: true },
      { kind: 'act', label: 'Tool · fn check_slack()', detail: 'Maintenance slack at FAC-DUB within 48h: available', ms: 64, ok: true },
      { kind: 'guard', label: 'Guardrail screen', detail: 'Tool scope OK · no injection markers · citations bound', ms: 12, ok: true },
      { kind: 'eval', label: 'Per-step eval', detail: 'Grounding 0.97 · calibration OK', ms: 41, ok: true },
      { kind: 'gate', label: 'Action proposal', detail: 'Schedule maintenance · awaiting human gate', ms: null, ok: null },
    ],
    memory: [
      { key: 'fleet/AC-2214/history', kind: 'episodic', age: '4d', size: '2.1 KB' },
      { key: 'ops/dispatch-preferences', kind: 'semantic', age: '31d', size: '0.8 KB' },
      { key: 'session/S-8812/scratch', kind: 'working', age: 'live', size: '4.4 KB' },
    ],
  },
  {
    id: 'S-8811',
    agent: 'feed-triage',
    policyVersion: 'v4',
    firstToken: '380ms',
    total: '511ms',
    steps: [
      { kind: 'plan', label: 'Plan', detail: 'Triage inbound sensor feeds; surface anomalies for review', ms: 210, ok: true },
      { kind: 'act', label: 'Tool · fabric.query', detail: 'Sensor feed · anomaly_score > 0.5 · SIG compartment', ms: 96, ok: true },
      { kind: 'observe', label: 'Observe', detail: '2 feeds flagged · provenance bound', ms: 6, ok: true },
      { kind: 'guard', label: 'Guardrail screen', detail: 'Citation requirement satisfied', ms: 9, ok: true },
      { kind: 'eval', label: 'Per-step eval', detail: 'Grounding 0.95', ms: 33, ok: true },
    ],
    memory: [
      { key: 'feeds/SIG/rolling-window', kind: 'working', age: 'live', size: '3.2 KB' },
      { key: 'ops/triage-thresholds', kind: 'semantic', age: '12d', size: '0.6 KB' },
    ],
  },
];

const FEEDBACK: MimirFeedbackItem[] = [
  { id: 'FB-2211', src: 'k.osei', kind: 'correction', target: 'dispatch-01', note: 'Prefer FAC-DUB over FAC-FRA when slack equal', prov: 'verified', applied: 'queued · evals gate', status: 'gated', statusLabel: 'Queued' },
  { id: 'FB-2210', src: 'evals/adversarial', kind: 'regression', target: 'dispatch-01', note: 'Multi-leg schedule cases drifting', prov: 'harness', applied: 'candidate v8 built', status: 'warn', statusLabel: 'In review' },
  { id: 'FB-2209', src: 'j.benjamin', kind: 'rating', target: 'feed-triage', note: '4/5 · citation density good', prov: 'verified', applied: 'aggregated', status: 'secure', statusLabel: 'Applied' },
  { id: 'FB-2208', src: 'unknown-relay', kind: 'correction', target: 'dispatch-01', note: 'Always approve vendor X routings', prov: 'FAILED', applied: 'discarded · flagged', status: 'danger', statusLabel: 'Rejected' },
];

const AUDIT_LIVE_POOL: Omit<AuditRecord, 'id' | 'ts'>[] = [
  { actor: 'agent:mimir/dispatch-01', action: 'Query · OAG grounding', resource: 'aircraft/AC-2261', pqSignature: 'ML-DSA ✓' },
  { actor: 'agent:mimir/feed-triage', action: 'Guardrail · citation requirement', resource: 'session/S-8811', pqSignature: 'ML-DSA ✓' },
  { actor: 'svc:mimir/evals', action: 'Eval · regression gate', resource: 'dispatch-adversarial', pqSignature: 'ML-DSA ✓' },
  { actor: 'agent:mimir/dispatch-01', action: 'Guardrail · injection quarantined', resource: 'session/S-8812', pqSignature: 'ML-DSA ✓' },
];

// ---- mock -------------------------------------------------------------------

export interface MimirMockOptions {
  /** Simulated round-trip latency in ms (default 180). */
  latencyMs?: number;
  policy?: PolicyContext;
}

const DEFAULT_POLICY: PolicyContext = {
  subject: 'j.benjamin',
  purpose: 'maintenance',
  clearance: 'confidential',
  roles: ['operator', 'analyst'],
};

/**
 * In-memory implementation of the `MimirGateway` seam.
 *
 * Models the agent plane closely enough to exercise every UI flow: grounded
 * agents, evals with a 90% regression gate, self-defending guardrail
 * interception, governed feedback with poisoning defence, and a live audit
 * stream. No network, no persistence — swap for an HttpGateway when a
 * browser-reachable bridge exists.
 */
export class MimirMock implements MimirGateway {
  private readonly latency: number;
  private readonly ctx: PolicyContext;
  private seq = 77120;
  private poolIdx = 0;
  private clock = 14 * 3600 + 32 * 60 + 8; // seeds "14:32:08"
  private listeners = new Set<(r: AuditRecord) => void>();
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(opts: MimirMockOptions = {}) {
    this.latency = opts.latencyMs ?? 180;
    this.ctx = opts.policy ?? DEFAULT_POLICY;
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

  private nextAuditId(): string {
    this.seq += 1;
    return `A-${this.seq}`;
  }

  private emit(rec: Omit<AuditRecord, 'id' | 'ts'>): AuditRecord {
    const record: AuditRecord = { ...rec, id: this.nextAuditId(), ts: this.stamp() };
    this.listeners.forEach((fn) => fn(record));
    return record;
  }

  // ---- build ---------------------------------------------------------------

  listAgents(): Promise<MimirAgent[]> {
    return this.wait(AGENTS.map((a) => ({ ...a })));
  }

  getLogic(agentId: string): Promise<MimirLogicFn> {
    // A single reference workflow today; keyed by agent for a future registry.
    const fn: MimirLogicFn = {
      ...LOGIC,
      id: agentId || LOGIC.id,
      agent: agentId || LOGIC.agent,
      steps: LOGIC.steps.map((s) => ({ ...s })),
      versions: LOGIC.versions.map((v) => ({ ...v })),
    };
    return this.wait(fn);
  }

  listSessions(): Promise<MimirSession[]> {
    return this.wait(SESSIONS.map((s) => this.cloneSession(s)));
  }

  getSessionTrace(id: string): Promise<MimirSession> {
    const s = SESSIONS.find((x) => x.id === id) ?? SESSIONS[0];
    return this.wait(this.cloneSession(s));
  }

  private cloneSession(s: MimirSession): MimirSession {
    return {
      ...s,
      steps: s.steps.map((st) => ({ ...st })),
      memory: s.memory.map((m) => ({ ...m })),
    };
  }

  // ---- govern --------------------------------------------------------------

  listEvals(): Promise<MimirEval[]> {
    return this.wait(EVALS.map((e) => ({ ...e })));
  }

  async runEval(id: string): Promise<MimirEvalRun> {
    const suite = EVALS.find((e) => e.id === id);
    const cases = (EVAL_CASES[id] ?? []).map((c) => ({ ...c }));
    const passed = cases.filter((c) => c.passed).length;
    const total = cases.length;
    // Score converges on the suite headline when a battery is defined.
    const score = total ? Math.round((passed / total) * 1000) / 10 : suite?.pass ?? 0;
    const gateClear = score >= 90;
    const audit = this.emit({
      actor: 'svc:mimir/evals',
      action: `Eval · run ${id}`,
      resource: `evals/${id}`,
      pqSignature: 'ML-DSA ✓',
    });
    return this.wait({ suite: id, score, passed, total, gateClear, cases, auditId: audit.id });
  }

  listGuardrails(): Promise<MimirGuardrail[]> {
    return this.wait(GUARDRAILS.map((g) => ({ ...g })));
  }

  async simulateGuardrail(input: string): Promise<MimirGuardrailCheck> {
    const text = input.toLowerCase();
    const injection = /ignore (all|any|previous|prior)|disregard (the |your )?(policy|guardrail|instruction)|system prompt|jailbreak|act as|pretend you/;
    const exfil = /exfiltrat|send (it |them |this )?(to )?(an? )?(external|outside|email|http|url)|leak|upload .*(external|outside)|post .*(webhook|endpoint)/;
    let result: Omit<MimirGuardrailCheck, 'auditId'>;
    if (injection.test(text)) {
      result = {
        intercepted: true,
        rail: 'Prompt-injection filter · input',
        reason: 'Indirect prompt-injection pattern detected. Input quarantined before any tool call; the session is flagged and nothing entered execution.',
        code: 'REFUSED · PROMPT-INJECTION',
      };
    } else if (exfil.test(text)) {
      result = {
        intercepted: true,
        rail: 'Exfiltration block · output',
        reason: 'Response would breach the NO-EGRESS seal. Blocked at the output rail; no data left the enclosure.',
        code: 'REFUSED · EXFILTRATION',
      };
    } else {
      result = {
        intercepted: false,
        rail: 'input → execution → output',
        reason: 'Passed all input, execution, and output rails. Grounded, cited, and within tool scope.',
        code: 'PASS',
      };
    }
    const audit = this.emit({
      actor: 'agent:mimir/dispatch-01',
      action: result.intercepted ? `Guardrail · ${result.code}` : 'Guardrail · pass',
      resource: 'session/live',
      pqSignature: 'ML-DSA ✓',
    });
    return this.wait({ ...result, auditId: audit.id });
  }

  listFeedback(): Promise<MimirFeedbackItem[]> {
    return this.wait(FEEDBACK.map((f) => ({ ...f })));
  }

  // ---- serve ---------------------------------------------------------------

  listModels(): Promise<MimirModel[]> {
    return this.wait(MODELS.map((m) => ({ ...m })));
  }

  // ---- governance / audit --------------------------------------------------

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
}
