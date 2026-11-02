/* @midgard/sdk — Admin Control Panel gateway slice.
 *
 * Admin is the suite's unified, self-governing management vertical: overview,
 * dual-control approvals, role-separated IAM (no super-admin), GitOps-style
 * config, registry/suite governance, an append-only audit ledger, break-glass,
 * and an advisory-only copilot. It renders under the Midgard red scope but is a
 * cross-cutting console — hence its own product-prefixed types and gateway.
 *
 * Design invariants modelled here (FR-AD-* / AC-AD-* / DD-*):
 *   - Dual-control with separation-of-duties: `approve()` DENIES self-approval
 *     (requester == approver) and PQ-signs the audit for granted AND denied
 *     paths [AC-AD-4, AC-AD-6].
 *   - No super-admin role exists; capability is column-split [AC-AD-2].
 *   - Break-glass is a request → time-boxed active → expired lifecycle, alerted
 *     and forced into post-hoc review [FR-AD-8, AC-AD-8].
 *   - The copilot is advisory-only and can refuse with a named code [FR-AD-10].
 *
 * Deterministic: no Date.now()/random at module load or in fixtures, so the
 * mock is reproducible. Swap `AdminMock` for an HTTP-backed `AdminGateway`
 * once a browser-reachable admin-zone bridge exists — no screen changes.
 *
 * The mock and its fixtures live in ../mock/admin.ts and are exported from
 * `@midgard/sdk/mock` only, so a console's main bundle never holds them.
 * [VLD-ANB-MIDGARD-0086] [AC-25.2]
 */

import type { AuditRecord, PolicyContext } from '../types';

// ---- risk & shared enums ----------------------------------------------------

/** Admin action risk class 1–5 (KDD-2). Risk ≥4 implies dual-control. */
export type AdminRiskClass = 1 | 2 | 3 | 4 | 5;

/** StatusPill-compatible health states used across the admin surfaces. */
export type AdminStatus = 'secure' | 'online' | 'healthy' | 'warn' | 'danger';

// ---- overview ---------------------------------------------------------------

/** A governed subsystem the panel composes (FR-AD-1). */
export interface AdminSubsystem {
  name: string;
  /** Admin surface / API name — admin API only, never the data plane. */
  api: string;
  /** 30-day SLO. */
  slo: string;
  status: AdminStatus;
  statusLabel: string;
  openFindings: number;
}

/** Aggregated cross-subsystem oversight (FR-AD-9). */
export interface AdminOverview {
  subsystems: AdminSubsystem[];
  pendingApprovals: number;
  /** Break-glass sessions awaiting post-hoc review. */
  breakGlassReview: number;
  /** Invariant: always 0 — no all-powerful role exists (AC-AD-2). */
  superAdminRoles: number;
  security: { deniedActions: number; selfApprovals: number; anomalyFlags: number };
}

// ---- dual-control approvals -------------------------------------------------

export type AdminApprovalState = 'awaiting' | 'approved' | 'denied' | 'executed';

/** A guarded action in the dual-control queue (FR-AD-5). */
export interface AdminApproval {
  id: string;
  action: string;
  subsystem: string;
  risk: AdminRiskClass;
  requester: string;
  /** Second controller. Must differ from `requester` (separation of duties). */
  approver: string;
  state: AdminApprovalState;
  /** Mandatory written justification. */
  justification: string;
  /** Present when state='denied', e.g. the self-approval reason. */
  deniedReason?: string;
}

/** Outcome of `approve()`. Denials are audited too (AC-AD-6). */
export interface AdminApprovalDecision {
  decision: 'granted' | 'denied';
  reason: string;
  /** PQ-signed audit id (also emitted for denials). */
  auditId: string;
}

// ---- identity & roles (RBAC, no super-admin) --------------------------------

/** A role-separated admin role (FR-AD-2). Never grants all capabilities. */
export interface AdminRole {
  name: string;
  /** The single column this role administers. */
  scope: string;
  /** Highest risk this role can request. */
  ceiling: AdminRiskClass;
  readonly: boolean;
  holders: number;
  status: AdminStatus;
  statusLabel: string;
}

/** A live, expiring role binding (FR-AD-2 — all bindings expire). */
export interface AdminBinding {
  user: string;
  role: string;
  scope: string;
  /** Time-to-expiry label, e.g. "47d". */
  expires: string;
  mfa: string;
  status: AdminStatus;
}

// ---- config as code (GitOps) ------------------------------------------------

export type AdminConfigState = 'simulated' | 'committed' | 'in review' | 'rolled-back';

/** A versioned config change with a signed diff (FR-AD-6, FR-AD-7). */
export interface AdminConfigCommit {
  id: string;
  title: string;
  subsystem: string;
  risk: AdminRiskClass;
  state: AdminConfigState;
  author: string;
  /** Content hash of the change body — versioned like source. */
  contentHash: string;
  /** Unified-diff lines (rendered in a Terminal). */
  diff: string[];
}

/** Dry-run result of a config simulation before commit (FR-AD-6). */
export interface AdminConfigSimulation {
  grants: string[];
  revokes: string[];
  /** Plain-language blast-radius summary (advisory, copilot-explained). */
  summary: string;
}

/** Result of committing a config change. */
export interface AdminConfigCommitResult {
  version: string;
  committed: boolean;
  auditId: string;
}

/** Result of rolling a committed change back. */
export interface AdminConfigRollbackResult {
  version: string;
  rolledBack: boolean;
  auditId: string;
}

// ---- registry & suite governance --------------------------------------------

export type AdminRegistryState = 'approved' | 'revoked' | 'pending';

/** A governed registry item (model / weights) — verified, fails closed (FR-AD-4). */
export interface AdminRegistryItem {
  name: string;
  family: string;
  state: AdminRegistryState;
  /** Weights signature verified (ML-DSA). */
  signature: boolean;
  uses: string;
  risk: AdminRiskClass;
}

// ---- audit ledger -----------------------------------------------------------

/** One append-only, ML-DSA-signed ledger entry (NFR-PQ-3). Denials + sims too. */
export interface AdminLedgerEntry {
  id: string;
  actor: string;
  action: string;
  subsystem: string;
  /** Justification / reason column. */
  justification: string;
  approver: string;
  ts: string;
}

// ---- break-glass ------------------------------------------------------------

export type AdminBreakGlassState = 'idle' | 'active' | 'review';

/** Emergency-elevation state (FR-AD-8): time-boxed, justified, alerted, reviewed. */
export interface AdminBreakGlass {
  state: AdminBreakGlassState;
  holder: string;
  /** Never wider than the emergency requires. */
  scope: string;
  /** Countdown label while active. */
  remaining: string;
  justification: string;
}

// ---- advisory copilot -------------------------------------------------------

export type AdminCopilotVariant = 'message' | 'proposal' | 'refusal';
export type AdminRefusalCode = 'CONTROL_BYPASS_REFUSED' | 'OUT_OF_SCOPE';

/** One turn in the advisory copilot thread (FR-AD-10). Never executes. */
export interface AdminCopilotTurn {
  role: 'admin' | 'copilot';
  variant: AdminCopilotVariant;
  text: string;
  /** Present on refusal turns — the visible, named refusal code. */
  refusalCode?: AdminRefusalCode;
  /** Present on proposal turns — the drafted-but-not-executed change. */
  proposalRisk?: AdminRiskClass;
  proposalNote?: string;
}

// ---- gateway ----------------------------------------------------------------

/**
 * The single transport seam for the Admin console. Every screen talks to this
 * interface — never a concrete client — so `AdminMock` can be swapped for an
 * `HttpGateway` variant without touching UI code.
 */
export interface AdminGateway {
  /** Active policy context (subject/purpose/clearance/roles) for this session. */
  policy(): PolicyContext;

  // Overview -----------------------------------------------------------------
  getOverview(): Promise<AdminOverview>;

  // Dual-control approvals ---------------------------------------------------
  listApprovals(): Promise<AdminApproval[]>;
  /** Second-controller decision. ENFORCES separation of duties: if `approver`
   *  is the same subject as the request's requester the decision is 'denied'
   *  with reason 'separation-of-duties: requester cannot approve own request';
   *  otherwise 'granted' with a PQ-signed audit id. Both paths are ledgered. */
  approve(id: string, approver: string): Promise<AdminApprovalDecision>;

  // Identity & roles ---------------------------------------------------------
  listRoles(): Promise<AdminRole[]>;
  listBindings(): Promise<AdminBinding[]>;

  // Config as code -----------------------------------------------------------
  listConfigCommits(): Promise<AdminConfigCommit[]>;
  simulateConfig(commitId: string): Promise<AdminConfigSimulation>;
  commitConfig(id: string): Promise<AdminConfigCommitResult>;
  rollbackConfig(id: string): Promise<AdminConfigRollbackResult>;

  // Registry & governance ----------------------------------------------------
  listRegistry(): Promise<AdminRegistryItem[]>;

  // Audit ledger -------------------------------------------------------------
  /** Cross-subsystem ledger search (filter tokens: actor:/subsystem:/since:). */
  searchLedger(query: string): Promise<AdminLedgerEntry[]>;
  /** Subscribe to the live admin ledger; returns an unsubscribe function. */
  auditStream(onRecord: (record: AuditRecord) => void): () => void;

  // Break-glass --------------------------------------------------------------
  getBreakGlass(): Promise<AdminBreakGlass>;
  /** Open a time-boxed, dual-controlled emergency elevation (idle → active). */
  requestBreakGlass(reason: string): Promise<AdminBreakGlass>;

  // Advisory copilot ---------------------------------------------------------
  getCopilotThread(): Promise<AdminCopilotTurn[]>;
}
