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
} from './types';

/**
 * The single transport seam for the Midgard UI.
 *
 * Every screen talks to this interface — never to a concrete client — so the
 * data source can be swapped without touching UI code. Today the only
 * implementation is `MockGateway`. When a browser-reachable bridge exists (a
 * runnable Rust service exposing HTTP/JSON or gRPC-web over the enclosure's
 * PQ-mTLS crates), `HttpGateway` implements this same interface and the app is
 * repointed by changing one provider value.
 *
 * Method names and payloads intentionally track the proto/service surface:
 *   query / applyAction        -> ObjectFabric.Query / ApplyAction
 *   auditStream / getAudit     -> governance audit ledger (AuditRecord)
 *   the remaining reads        -> per-subsystem view models (see types.ts)
 */
export interface GatewayClient {
  /** The active policy context (subject/purpose/clearance/roles) for this session. */
  policy(): PolicyContext;

  // Object fabric ------------------------------------------------------------
  /** List object *types* in the fabric registry (Object Explorer). */
  listObjectTypes(): Promise<ObjectType[]>;
  /** Typed object/link query under policy. `ObjectFabric.Query` */
  query(req: QueryRequest): Promise<QueryResponse>;
  /** Apply a validated, function-backed action. Gated actions require a
   *  `humanGateToken`; the mock rejects gated calls without one. `ObjectFabric.ApplyAction` */
  applyAction(req: ApplyActionRequest): Promise<ApplyActionResponse>;
  /** Search-around: expand links from a seed object out to `hops`. */
  searchAround(seedId: string, hops: number): Promise<SearchGraph>;

  // Governance / audit -------------------------------------------------------
  /** Snapshot of the most recent audit records (newest first). */
  getAudit(limit?: number): Promise<AuditRecord[]>;
  /** Subscribe to the live audit lineage; returns an unsubscribe function. */
  auditStream(onRecord: (record: AuditRecord) => void): () => void;

  // Policies & markings ------------------------------------------------------
  listPolicies(): Promise<PolicyRule[]>;
  /** Deny-by-default policy dry-run against the given context. */
  simulatePolicy(ctx: PolicyContext, marking: string): Promise<PolicyDecision>;

  // Platform read models -----------------------------------------------------
  listIdentities(): Promise<Identity[]>;
  listKeys(): Promise<KeyRecord[]>;
  listComputeJobs(): Promise<ComputeJob[]>;
  getObservability(): Promise<SloMetric[]>;
  getResilience(): Promise<RolloutState[]>;
  getContractSurface(): Promise<RpcSurface[]>;
}
