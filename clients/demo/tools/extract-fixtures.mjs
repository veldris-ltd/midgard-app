// Extract the demo fixtures from the release's console mocks. [AC-C-13] [FR-C-1]
//
// Run through extract-fixtures.sh, which loads Node's type stripping and the SDK's
// extensionless-import hook (sdk/ts/scripts/ts-resolve.mjs). Output is canonical JSON
// (sorted keys, no whitespace), one file per console, plus manifest.json with the
// git blob id of every input and the SHA-256 of every output.
import { readFileSync, writeFileSync, mkdirSync, rmSync, readdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { join, dirname, relative } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '../../..');
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : dflt; };
const version = readFileSync(join(ROOT, 'Cargo.toml'), 'utf8').match(/^\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)[1];
const release = arg('--release', version);
const out = arg('--out', join(ROOT, 'clients/demo/fixtures', release));
const die = (m) => { console.error(`extract-fixtures: ${m}`); process.exit(1); };

const mock = await import(pathToFileURL(join(ROOT, 'sdk/ts/src/mock/index.ts')).href);
const fresh = {
  midgard: () => new mock.MockGateway({ latencyMs: 0 }),
  admin: () => new mock.AdminMock({ latencyMs: 0 }),
  heimdall: () => new mock.HeimdallMock({ latencyMs: 0 }),
  forge: () => new mock.ForgeMock({ latencyMs: 0 }),
  mimir: () => new mock.MimirMock({ latencyMs: 0 }),
  bifrost: () => new mock.BifrostMock({ latencyMs: 0 }),
};
const SOURCE = {
  midgard: ['sdk/ts/src/mock/MockGateway.ts', 'sdk/ts/src/mock/fixtures.ts'],
  admin: ['sdk/ts/src/mock/admin.ts'], heimdall: ['sdk/ts/src/mock/heimdall.ts'],
  forge: ['sdk/ts/src/mock/forge.ts'], mimir: ['sdk/ts/src/mock/mimir.ts'],
  bifrost: ['sdk/ts/src/mock/bifrost.ts'],
};

// name, the Rust read model, the read, and (optionally) how to id a row.
const one = (id) => async (p) => [{ ...(await p), __id: id }];
const C = [
  ['midgard.objectTypes', 'MidgardObjectType', (g) => g.listObjectTypes(), (x) => x.apiName],
  ['midgard.objects', 'MidgardObjectInstance', async (g) => {
    const types = await g.listObjectTypes(); const rows = [];
    for (const t of types) rows.push(...(await fresh.midgard().query({ policy: g.policy(), objectType: t.apiName, maxResults: 5 })).results);
    return rows; }],
  ['midgard.searchAround', 'MidgardSearchGraph', async (g) => {
    const types = await g.listObjectTypes(); const rows = [];
    for (const t of types) rows.push(await fresh.midgard().searchAround(`${t.apiName}/1000`, 2));
    return rows; }, (x) => x.seed],
  ['midgard.audit', 'AuditRecord', (g) => g.getAudit(50)],
  ['midgard.policies', 'MidgardPolicyRule', (g) => g.listPolicies()],
  ['midgard.identities', 'MidgardIdentity', (g) => g.listIdentities(), (x) => x.subject],
  ['midgard.keys', 'MidgardKeyRecord', (g) => g.listKeys()],
  ['midgard.computeJobs', 'MidgardComputeJob', (g) => g.listComputeJobs()],
  ['midgard.observability', 'MidgardSloMetric', (g) => g.getObservability(), (x) => x.label],
  ['midgard.resilience', 'MidgardRolloutState', (g) => g.getResilience()],
  ['midgard.contractSurface', 'MidgardRpcSurface', (g) => g.getContractSurface(), (x) => x.service],
  ['admin.overview', 'AdminOverview', (g) => one('overview')(g.getOverview())],
  ['admin.approvals', 'AdminApproval', (g) => g.listApprovals()],
  ['admin.roles', 'AdminRole', (g) => g.listRoles(), (x) => x.name],
  ['admin.bindings', 'AdminBinding', (g) => g.listBindings(), (x) => `${x.user}:${x.role}`],
  ['admin.configCommits', 'AdminConfigCommit', (g) => g.listConfigCommits()],
  ['admin.configSimulations', 'AdminConfigSimulation', async (g) => {
    const rows = [];
    for (const c of await g.listConfigCommits()) rows.push({ ...(await fresh.admin().simulateConfig(c.id)), __id: c.id });
    return rows; }],
  ['admin.registry', 'AdminRegistryItem', (g) => g.listRegistry(), (x) => x.name],
  ['admin.ledger', 'AdminLedgerEntry', (g) => g.searchLedger('')],
  ['admin.breakGlass', 'AdminBreakGlass', (g) => one('breakglass')(g.getBreakGlass())],
  ['admin.copilot', 'AdminCopilotTurn', (g) => g.getCopilotThread()],
  ['heimdall.picture', 'HeimdallPicture', async (g) => [{ ...(await g.getPicture()), tracks: [], entities: [], __id: 'picture' }]],
  ['heimdall.tracks', 'HeimdallTrack', async (g) => (await g.getPicture()).tracks],
  ['heimdall.feeds', 'HeimdallFeed', (g) => g.listFeeds(), (x) => x.name],
  ['heimdall.entities', 'HeimdallEntity', (g) => g.listEntities()],
  ['heimdall.syncSites', 'HeimdallSyncSite', (g) => g.listSyncSites(), (x) => x.name],
  ['heimdall.workflows', 'HeimdallWorkflow', (g) => g.listWorkflows()],
  ['heimdall.decisions', 'HeimdallDecision', (g) => g.listDecisions()],
  ['heimdall.audit', 'AuditRecord', (g) => g.getAudit(50)],
  ['forge.pipelines', 'ForgePipeline', (g) => g.listPipelines()],
  ['forge.connectors', 'ForgeConnector', (g) => g.listConnectors()],
  ['forge.datasets', 'ForgeDataset', (g) => g.listDatasets()],
  ['forge.healthChecks', 'ForgeHealthCheck', (g) => g.listHealthChecks()],
  ['forge.schedules', 'ForgeSchedule', (g) => g.listSchedules()],
  ['forge.transform', 'ForgeTransform', (g) => one('transform')(g.getTransform())],
  ['forge.projections', 'ForgeProjection', (g) => g.listProjections()],
  ['forge.audit', 'AuditRecord', (g) => g.getAudit(50)],
  ['mimir.agents', 'MimirAgent', (g) => g.listAgents()],
  ['mimir.logic', 'MimirLogicFn', async (g) => {
    const rows = [];
    for (const a of await g.listAgents()) rows.push({ ...(await fresh.mimir().getLogic(a.id)), __id: a.id });
    return rows; }],
  ['mimir.sessions', 'MimirSession', (g) => g.listSessions()],
  ['mimir.evals', 'MimirEval', (g) => g.listEvals()],
  ['mimir.guardrails', 'MimirGuardrail', (g) => g.listGuardrails()],
  ['mimir.feedback', 'MimirFeedbackItem', (g) => g.listFeedback()],
  ['mimir.models', 'MimirModel', (g) => g.listModels()],
  ['bifrost.releases', 'BifrostRelease', (g) => g.listReleases()],
  ['bifrost.rings', 'BifrostRing', (g) => g.listRings()],
  ['bifrost.artefacts', 'BifrostArtefact', (g) => g.listArtefacts()],
  ['bifrost.channels', 'BifrostChannel', (g) => g.listChannels()],
  ['bifrost.bundles', 'BifrostBundle', (g) => g.listBundles()],
  ['bifrost.fleet', 'BifrostFleetNode', (g) => g.listFleet()],
  ['bifrost.rollbacks', 'BifrostRollback', (g) => g.listRollbacks()],
  ['bifrost.audit', 'AuditRecord', (g) => g.getAudit(50)],
];

const personas = JSON.parse(readFileSync(join(ROOT, 'clients/demo/personas.json'), 'utf8'));
const overlay = JSON.parse(readFileSync(join(ROOT, 'clients/demo/overlay.json'), 'utf8'));
const LEVELS = ['open', 'internal', 'restricted', 'confidential'];
for (const p of personas) if (!LEVELS.includes(p.clearance)) die(`persona ${p.id}: unknown clearance ${p.clearance}`);

function markingOf(name, id, x, entities) {
  const lvl = (v) => (LEVELS.includes(v) ? v : null);
  const fromItem = lvl(x.marking) ?? lvl(x.markings?.[0]?.level);
  if (fromItem) {
    const comps = x.compartments ?? x.markings?.[0]?.compartments ?? (x.compartment ? [x.compartment] : []);
    return { level: fromItem, compartments: [...new Set(comps)].sort() };
  }
  if (name === 'heimdall.tracks' && entities.has(id)) return entities.get(id);
  const c = overlay.collections[name] ?? {};
  return c.ids?.[id] ?? c.default ?? overlay.default;
}

// Canonical JSON: sorted keys, no whitespace.
const canon = (v) => JSON.stringify(v, (_, x) =>
  x && typeof x === 'object' && !Array.isArray(x) ? Object.fromEntries(Object.keys(x).sort().map((k) => [k, x[k]])) : x);

const byConsole = new Map(Object.keys(fresh).map((c) => [c, {}]));
const entityMarkings = new Map();
for (const [name, type, read, idOf] of C) {
  const console_ = name.split('.')[0];
  const raw = await read(fresh[console_]());
  const seen = new Set();
  const rows = raw.map((x, i) => {
    const { __id, ...item } = x;
    const id = String(__id ?? idOf?.(item) ?? item.id ?? item.name ?? i);
    if (seen.has(id)) die(`${name}: duplicate row id ${id}; give the collection an id function`);
    seen.add(id);
    return { id, marking: markingOf(name, id, item, entityMarkings), item };
  });
  if (name === 'heimdall.entities') for (const r of rows) entityMarkings.set(r.id, r.marking);
  for (const id of Object.keys(overlay.collections[name]?.ids ?? {})) if (!seen.has(id)) die(`overlay names ${name}/${id}, which does not exist`);
  byConsole.get(console_)[name] = { type, rows };
}
// heimdall.tracks is read before heimdall.entities in C; re-mark tracks now that entity markings are known.
for (const r of byConsole.get('heimdall')['heimdall.tracks'].rows) if (entityMarkings.has(r.id)) r.marking = entityMarkings.get(r.id);

// The synthetic hour: a 60-minute closed loop per track, every 10 s.
const box = { latN: 56.0, latS: 51.0, lonW: -10.0, lonE: -2.0 };
const round = (n) => Math.round(n * 1e5) / 1e5;
const motion = byConsole.get('heimdall')['heimdall.tracks'].rows.map((r) => {
  const t = r.item;
  const lat0 = box.latN - (t.y / 100) * (box.latN - box.latS);
  const lon0 = box.lonW + (t.x / 100) * (box.lonE - box.lonW);
  const radiusNm = Math.max(t.spd, 1) / (2 * Math.PI);           // one clockwise loop per hour
  const h = (t.hdg * Math.PI) / 180;
  const k = 60 * Math.cos((lat0 * Math.PI) / 180);                // nm per degree of longitude
  // Centre to the right of the heading; start on the loop at the picture position.
  const cLat = lat0 - (radiusNm * Math.sin(h)) / 60;
  const cLon = lon0 + (radiusNm * Math.cos(h)) / k;
  const a0 = Math.atan2(-Math.cos(h), Math.sin(h));               // bearing from centre to start
  const samples = [];
  for (let s = 0; s < 360; s++) {
    const a = a0 + (2 * Math.PI * s) / 360;
    samples.push({ t: s * 10, lat: round(cLat + (radiusNm * Math.cos(a)) / 60), lon: round(cLon + (radiusNm * Math.sin(a)) / k) });
  }
  return { id: r.id, marking: r.marking, item: { id: r.id, provenance: 'synthetic: HeimdallMock track, closed loop', samples } };
});
byConsole.get('heimdall')['heimdall.motion'] = { type: 'HeimdallMotion', rows: motion };

const blob = (p) => execFileSync('git', ['hash-object', join(ROOT, p)], { encoding: 'utf8' }).trim();
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const files = {};
const write = (f, v) => { const s = canon(v); writeFileSync(join(out, f), s); files[f] = createHash('sha256').update(s).digest('hex'); };
write('personas.json', personas);
for (const [c, collections] of byConsole) {
  write(`${c}.json`, { console: c, sources: Object.fromEntries(SOURCE[c].map((p) => [p, blob(p)])), collections });
}
const inputs = Object.fromEntries([...Object.values(SOURCE).flat(), 'clients/demo/personas.json',
  'clients/demo/overlay.json', 'clients/demo/tools/extract-fixtures.mjs'].map((p) => [p, blob(p)]));
writeFileSync(join(out, 'manifest.json'), canon({ format: 'midgard-demo-fixtures', version: 1, release, files, inputs }));
console.log(`extract-fixtures: ${C.length} collections + motion -> ${relative(ROOT, out) || out}`);
