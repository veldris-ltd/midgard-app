/* Midgard domain model (TypeScript).
 *
 * These types mirror the real backend contract so the mock adapter is
 * swap-compatible with a future transport:
 *   - contracts/proto/midgard/v1/common.proto        -> Marking, PolicyContext, AuditRecord
 *   - contracts/proto/midgard/v1/object_fabric.proto -> ObjectInstance, Query*, ApplyAction*
 *   - services/fabric (Rust library types)           -> ObjectType, LinkType, ActionType, Value
 *
 * The remaining per-screen read types (identity, kms, policies, compute,
 * observability, resilience, sdk surface) are UI-facing view models; when the
 * corresponding services gain a proto + HTTP bridge these are the shapes the
 * HttpGateway will produce.
 */

// ---- common.proto -----------------------------------------------------------

export type MarkingLevel = 'confidential' | 'restricted' | 'internal' | 'open';

/** Classification marking (fixed colours in the UI, governance chrome). */
export interface Marking {
  id: string;
  name: string;
  level: MarkingLevel;
  /** Optional compartments, e.g. OPS-N, SIG. */
  compartments: string[];
}

/** Governance context carried on every fabric call. `[NFR-SEC-2]` */
export interface PolicyContext {
  subject: string;
  purpose: string;
  clearance: MarkingLevel;
  roles: string[];
}

/** Append-only, ML-DSA-signed lineage entry. */
export interface AuditRecord {
  id: string;
  actor: string;
  action: string;
  resource: string;
  /** Display timestamp (HH:MM:SS) as emitted by the enclosure clock. */
  ts: string;
  /** PQ signature marker, e.g. "ML-DSA ✓". */
  pqSignature: string;
}

// ---- object_fabric.proto ----------------------------------------------------

/** A single fabric object instance, marking-filtered by policy. */
export interface ObjectInstance {
  id: string;
  typeId: string;
  propertyValues: Record<string, string>;
  markings: Marking[];
}

export interface QueryRequest {
  policy: PolicyContext;
  objectType: string;
  filter?: Record<string, string>;
  linkTraversals?: string[];
  maxResults?: number;
  /** Only objects whose key or a text property contains this, ignoring case. */
  search?: string;
  /** One object by full id (`<type>/<key>`): at most one result. */
  id?: string;
}

export interface QueryResponse {
  results: ObjectInstance[];
  /** How many readable objects matched before `maxResults` cut them. */
  matched?: number;
  auditId: string;
}

export interface ApplyActionRequest {
  policy: PolicyContext;
  actionType: string;
  params?: Record<string, string>;
  /** Human-gate approval: the approving person's reason, recorded under their name
   *  before the action runs. Required for gated actions. `[NFR-AI-1]` */
  humanGateToken?: string;
}

export interface ApplyActionResponse {
  auditId: string;
  applied: boolean;
  /** The ledger entry recording who approved a gated action, and why. */
  approvalAuditId?: string;
}

// ---- fabric type registry (view models) ------------------------------------

export interface LinkType {
  /** Human label, e.g. "operates → Route". */
  label: string;
}

export interface ActionType {
  name: string;
  /** Consequential actions require a human gate before ApplyAction. */
  humanGated: boolean;
}

/** An object *type* in the fabric registry (the Object Explorer rows). */
export interface ObjectType {
  name: string;
  apiName: string;
  propertyCount: number;
  instanceCount: string;
  marking: MarkingLevel;
  compartment: string | null;
  links: LinkType[];
  actions: ActionType[];
  updated: string;
}

// ---- search-around ----------------------------------------------------------

export interface GraphNode {
  id: string;
  label: string;
  typeName: string;
  marking: MarkingLevel;
  hop: number;
}

export interface GraphEdge {
  from: string;
  to: string;
  label: string;
}

export interface SearchGraph {
  seed: string;
  nodes: GraphNode[];
  edges: GraphEdge[];
  auditId: string;
}

// ---- policies & markings ----------------------------------------------------

export interface PolicyRule {
  id: string;
  name: string;
  effect: 'allow' | 'deny';
  marking: MarkingLevel;
  purpose: string;
  roles: string[];
}

export interface PolicyDecision {
  decision: 'grant' | 'deny';
  /** The rule id that determined the outcome, or null for deny-by-default. */
  matchedRule: string | null;
  reason: string;
  auditId: string;
}

// ---- identity & access ------------------------------------------------------

export interface Identity {
  subject: string;
  kind: 'user' | 'service' | 'agent';
  clearance: MarkingLevel;
  roles: string[];
  groups: string[];
  status: 'active' | 'suspended';
  lastSeen: string;
}

// ---- keys & crypto ----------------------------------------------------------

export interface KeyRecord {
  id: string;
  purpose: string;
  alg: string;
  custody: 'enclosure' | 'hsm' | 'client';
  wrapped: boolean;
  rotated: string;
  status: 'active' | 'rotating' | 'retired';
}

// ---- compute ----------------------------------------------------------------

export interface ComputeJob {
  id: string;
  kind: string;
  backend: string;
  state: 'queued' | 'running' | 'done' | 'failed';
  progress: number;
  submitted: string;
}

// ---- observability ----------------------------------------------------------

export interface SloMetric {
  label: string;
  value: string;
  target: string;
  healthy: boolean;
}

// ---- resilience -------------------------------------------------------------

export interface RolloutState {
  id: string;
  ring: string;
  version: string;
  progress: number;
  state: 'pending' | 'in-progress' | 'healthy' | 'rolled-back';
  healthGate: 'pass' | 'watch' | 'fail';
}

// ---- SDK & APIs (contract surface) -----------------------------------------

export interface RpcSurface {
  service: string;
  rpcs: { name: string; request: string; response: string; note: string }[];
}
