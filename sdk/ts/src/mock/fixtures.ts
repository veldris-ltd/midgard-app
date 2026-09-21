/* Seed data for the MockGateway. Derived from the design-system prototype's
 * inline consts (docs/design/ui_kits/midgard-console/*), extended to cover the
 * full set of Midgard-core screens. Deterministic — no Date.now()/random at
 * module load — so the mock is reproducible. */

import type {
  ActionType,
  AuditRecord,
  ComputeJob,
  Identity,
  KeyRecord,
  ObjectType,
  PolicyRule,
  RolloutState,
  RpcSurface,
  SloMetric,
} from '../types';

const gate = (name: string, humanGated = false): ActionType => ({ name, humanGated });

export const OBJECT_TYPES: ObjectType[] = [
  {
    name: 'Aircraft', apiName: 'aircraft', propertyCount: 14, instanceCount: '12,408',
    marking: 'confidential', compartment: 'OPS-N',
    links: [{ label: 'operates → Route' }, { label: 'assigned → Crew' }],
    actions: [gate('Ground aircraft', true), gate('Schedule maintenance', true)],
    updated: '2m ago',
  },
  {
    name: 'Facility', apiName: 'facility', propertyCount: 9, instanceCount: '1,882',
    marking: 'restricted', compartment: null,
    links: [{ label: 'hosts → Asset' }],
    actions: [gate('Change status', true)],
    updated: '11m ago',
  },
  {
    name: 'Sensor Feed', apiName: 'sensor_feed', propertyCount: 21, instanceCount: '640',
    marking: 'confidential', compartment: 'SIG',
    links: [{ label: 'observes → Entity' }],
    actions: [gate('Recalibrate', false)],
    updated: '18s ago',
  },
  {
    name: 'Crew Member', apiName: 'crew_member', propertyCount: 17, instanceCount: '3,051',
    marking: 'internal', compartment: null,
    links: [{ label: 'assigned → Aircraft' }],
    actions: [gate('Update clearance', true)],
    updated: '1h ago',
  },
  {
    name: 'Maintenance Event', apiName: 'maintenance_event', propertyCount: 12, instanceCount: '48,112',
    marking: 'internal', compartment: null,
    links: [{ label: 'on → Aircraft' }],
    actions: [gate('Close event', false)],
    updated: '4m ago',
  },
];

export const AUDIT_SEED: AuditRecord[] = [
  { id: 'AUD-1042', actor: 'agent:mimir/dispatch-01', action: 'Action · Schedule maintenance', resource: 'aircraft/AC-2214', pqSignature: 'ML-DSA ✓', ts: '14:32:08' },
  { id: 'AUD-1041', actor: 'j.benjamin', action: 'Query · search-around', resource: 'aircraft/*', pqSignature: 'ML-DSA ✓', ts: '14:31:56' },
  { id: 'AUD-1040', actor: 'svc:forge/projector', action: 'Write · object sync', resource: 'sensor_feed/†640', pqSignature: 'ML-DSA ✓', ts: '14:31:41' },
  { id: 'AUD-1039', actor: 'j.benjamin', action: 'Query · object types', resource: 'fabric/registry', pqSignature: 'ML-DSA ✓', ts: '14:31:12' },
  { id: 'AUD-1038', actor: 'svc:heimdall/fusion', action: 'Write · entity projection', resource: 'entity/E-88213', pqSignature: 'ML-DSA ✓', ts: '14:30:47' },
];

/** Rotating pool used to synthesise "live" audit records on the stream. */
export const AUDIT_LIVE_POOL: Omit<AuditRecord, 'id' | 'ts'>[] = [
  { actor: 'agent:mimir/dispatch-01', action: 'Query · OAG grounding', resource: 'aircraft/AC-2261', pqSignature: 'ML-DSA ✓' },
  { actor: 'svc:forge/projector', action: 'Write · object sync', resource: 'facility/†1882', pqSignature: 'ML-DSA ✓' },
  { actor: 'k.rao', action: 'Query · search-around', resource: 'crew_member/*', pqSignature: 'ML-DSA ✓' },
  { actor: 'svc:kms/rotator', action: 'Action · key rotation', resource: 'kms/dek-ops-n', pqSignature: 'ML-DSA ✓' },
];

export const POLICIES: PolicyRule[] = [
  { id: 'POL-CONF-OPS', name: 'Confidential OPS-N read', effect: 'allow', marking: 'confidential', purpose: 'operations', roles: ['operator', 'analyst'] },
  { id: 'POL-SIG-READ', name: 'SIG compartment read', effect: 'allow', marking: 'confidential', purpose: 'analysis', roles: ['sigint-analyst'] },
  { id: 'POL-RESTRICT', name: 'Restricted default read', effect: 'allow', marking: 'restricted', purpose: 'operations', roles: ['operator'] },
  { id: 'POL-DENY-EXPORT', name: 'Deny egress of confidential', effect: 'deny', marking: 'confidential', purpose: 'export', roles: ['*'] },
];

export const IDENTITIES: Identity[] = [
  { subject: 'j.benjamin', kind: 'user', clearance: 'confidential', roles: ['operator', 'analyst'], groups: ['ops-north'], status: 'active', lastSeen: '14:31' },
  { subject: 'k.rao', kind: 'user', clearance: 'restricted', roles: ['analyst'], groups: ['fusion'], status: 'active', lastSeen: '13:58' },
  { subject: 'agent:mimir/dispatch-01', kind: 'agent', clearance: 'confidential', roles: ['agent-runtime'], groups: ['mimir'], status: 'active', lastSeen: 'live' },
  { subject: 'svc:forge/projector', kind: 'service', clearance: 'internal', roles: ['projector'], groups: ['forge'], status: 'active', lastSeen: 'live' },
  { subject: 'a.stone', kind: 'user', clearance: 'internal', roles: ['auditor'], groups: ['governance'], status: 'suspended', lastSeen: '2d ago' },
];

export const KEYS: KeyRecord[] = [
  { id: 'kek-root', purpose: 'Root KEK', alg: 'ML-KEM-1024', custody: 'enclosure', wrapped: false, rotated: '2026-06-01', status: 'active' },
  { id: 'dek-ops-n', purpose: 'OPS-N data key', alg: 'AES-256-GCM', custody: 'enclosure', wrapped: true, rotated: '2026-07-08', status: 'rotating' },
  { id: 'dek-sig', purpose: 'SIG data key', alg: 'AES-256-GCM', custody: 'hsm', wrapped: true, rotated: '2026-07-02', status: 'active' },
  { id: 'sig-svc-mesh', purpose: 'Service mesh auth', alg: 'ML-DSA-65', custody: 'enclosure', wrapped: false, rotated: '2026-07-09', status: 'active' },
];

export const COMPUTE_JOBS: ComputeJob[] = [
  { id: 'JOB-3391', kind: 'Typed query', backend: 'in-process', state: 'running', progress: 62, submitted: '14:31' },
  { id: 'JOB-3390', kind: 'Function apply', backend: 'in-process', state: 'done', progress: 100, submitted: '14:29' },
  { id: 'JOB-3389', kind: 'Projection rebuild', backend: 'forge-engine', state: 'queued', progress: 0, submitted: '14:33' },
  { id: 'JOB-3388', kind: 'Marking reindex', backend: 'in-process', state: 'failed', progress: 41, submitted: '14:22' },
];

export const OBSERVABILITY: SloMetric[] = [
  { label: 'CONTROL-PLANE SLO', value: '99.97%', target: '99.9%', healthy: true },
  { label: 'QUERY P95', value: '38ms', target: '<100ms', healthy: true },
  { label: 'LINEAGE SIGNED', value: '100%', target: '100%', healthy: true },
  { label: 'PQ HANDSHAKE FAIL', value: '0', target: '0', healthy: true },
  { label: 'DENY-BY-DEFAULT HITS', value: '1,204', target: '—', healthy: true },
];

export const RESILIENCE: RolloutState[] = [
  { id: 'RLT-fabric', ring: 'canary', version: 'v1.4.2', progress: 100, state: 'healthy', healthGate: 'pass' },
  { id: 'RLT-gateway', ring: 'broad', version: 'v1.4.2', progress: 74, state: 'in-progress', healthGate: 'watch' },
  { id: 'RLT-governance', ring: 'broad', version: 'v1.4.1', progress: 100, state: 'healthy', healthGate: 'pass' },
  { id: 'RLT-kms', ring: 'canary', version: 'v1.4.3-rc1', progress: 20, state: 'pending', healthGate: 'pass' },
];

export const CONTRACT_SURFACE: RpcSurface[] = [
  {
    service: 'midgard.v1.ObjectFabric',
    rpcs: [
      { name: 'Query', request: 'QueryRequest', response: 'QueryResponse', note: 'Typed query under policy; marking-filtered + audited.' },
      { name: 'ApplyAction', request: 'ApplyActionRequest', response: 'ApplyActionResponse', note: 'Function-backed action; human-gated; returns audit id.' },
    ],
  },
  {
    service: 'midgard.v1.common',
    rpcs: [
      { name: 'Marking', request: '—', response: '—', note: 'Classification tag (level + compartments).' },
      { name: 'PolicyContext', request: '—', response: '—', note: 'Subject / purpose / clearance / roles on every call.' },
      { name: 'AuditRecord', request: '—', response: '—', note: 'ML-DSA-signed append-only lineage entry.' },
    ],
  },
];
