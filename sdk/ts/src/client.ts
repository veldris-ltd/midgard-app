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
  /** Apply a validated, function-backed action to the one object named in
   *  `params.target`. Gated actions require a `humanGateToken`: the id of an
   *  approval granted for this caller, action and object. `ObjectFabric.ApplyAction` */
  applyAction(req: ApplyActionRequest): Promise<ApplyActionResponse>;
  /** Approvals whose object the caller can read, newest first. `[NFR-AI-1]` */
  listApprovals(): Promise<Approval[]>;
  /** Ask a second person to approve a gated action on one object. */
  requestApproval(actionType: string, target: string): Promise<Approval>;
  /** Approve or reject someone else's pending approval. */
  decideApproval(id: string, approve: boolean): Promise<Approval>;
  /** Search-around: expand links from a seed object out to `hops`, returning at most
   *  `limit` nodes (the edge's default is 60); `truncated` says when it stopped short. */
  searchAround(seedId: string, hops: number, limit?: number): Promise<SearchGraph>;

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
