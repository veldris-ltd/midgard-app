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
 *
 * The mock and its fixtures live in ../mock/mimir.ts and are exported from
 * `@midgard/sdk/mock` only, so a console's main bundle never holds them.
 * [VLD-ANB-MIDGARD-0086] [AC-25.2]
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
