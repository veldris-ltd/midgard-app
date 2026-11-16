// Extract the demo fixtures from the release's console mocks. [AC-C-13] [FR-C-1]
//
// Run through extract-fixtures.sh, which loads Node's type stripping and the SDK's
// extensionless-import hook (sdk/ts/scripts/ts-resolve.mjs). Output is canonical JSON
// (sorted keys, no whitespace), one file per console, plus manifest.json with the
// git blob id of every input and the SHA-256 of every output.
import { readFileSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { join, dirname, relative, resolve } from 'node:path';
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
const overlayPath = resolve(arg('--overlay', join(ROOT, 'clients/demo/overlay.json')));
const overlay = JSON.parse(readFileSync(overlayPath, 'utf8'));
const LEVELS = ['open', 'internal', 'restricted', 'confidential'];
const rank = (level) => LEVELS.indexOf(level);
const covers = (a, b) => rank(a.level) >= rank(b.level) && b.compartments.every((k) => a.compartments.includes(k));
const show = (m) => (m.compartments.length ? `${m.level} [${m.compartments.join(', ')}]` : m.level);
for (const p of personas) if (!LEVELS.includes(p.clearance)) die(`persona ${p.id}: unknown clearance ${p.clearance}`);

// The overlay is checked whole before anything is marked with it: every marking a
// known level with a list of compartment names, every collection one that is extracted.
function checkMarking(where, m) {
  if (!m || typeof m !== 'object' || Array.isArray(m)) die(`overlay ${where}: not a marking object`);
  if (!LEVELS.includes(m.level)) die(`overlay ${where}: unknown level ${m.level}`);
  if (!Array.isArray(m.compartments) || !m.compartments.every((k) => typeof k === 'string' && k !== ''))
    die(`overlay ${where}: compartments must be a list of strings`);
  const extra = Object.keys(m).filter((k) => k !== 'level' && k !== 'compartments');
  if (extra.length) die(`overlay ${where}: unknown key ${extra[0]}`);
  return { level: m.level, compartments: [...new Set(m.compartments)].sort() };
}
const NAMES = new Set(C.map(([name]) => name));
const ov = { default: checkMarking('default', overlay.default), collections: {} };
for (const [name, c] of Object.entries(overlay.collections ?? {})) {
  if (!NAMES.has(name)) die(`overlay names collection ${name}, which is not extracted`);
  ov.collections[name] = {
    default: c.default === undefined ? undefined : checkMarking(`${name} default`, c.default),
    ids: Object.fromEntries(Object.entries(c.ids ?? {}).map(([id, m]) => [id, checkMarking(`${name}/${id}`, m)])),
  };
}

// The marking a row carries itself: null when it has none, { unknown } when it has one
// this extractor does not recognise. A `markings` list yields its highest level and the
// union of its compartments.
function itemMarking(x) {
  const comps = (v) => {
    if (v === undefined || v === null) return []; // the contract's `compartment: string | null`
    if (typeof v === 'string' && v !== '') return [v];
    if (Array.isArray(v) && v.every((k) => typeof k === 'string' && k !== '')) return v;
    return null;
  };
  if (x.marking !== undefined) {
    if (!LEVELS.includes(x.marking)) return { unknown: x.marking };
    const cs = [...(comps(x.compartment) ?? [null]), ...(comps(x.compartments) ?? [null])];
    if (cs.includes(null)) return { unknown: { compartment: x.compartment, compartments: x.compartments } };
    return { level: x.marking, compartments: [...new Set(cs)].sort() };
  }
  if (x.markings !== undefined) {
    const ms = x.markings;
    if (!Array.isArray(ms) || ms.length === 0) return { unknown: ms };
    let level = null; const cs = new Set();
    for (const m of ms) {
      if (!m || !LEVELS.includes(m.level)) return { unknown: ms };
      const c = comps(m.compartments);
      if (c === null) return { unknown: ms };
      c.forEach((k) => cs.add(k));
      if (level === null || rank(m.level) > rank(level)) level = m.level;
    }
    return { level, compartments: [...cs].sort() };
  }
  return null;
}

// The highest level named anywhere inside a row (`marking`/`level` values only).
function innerFloor(v) {
  let top = null;
  const walk = (x) => {
    if (Array.isArray(x)) { x.forEach(walk); return; }
    if (!x || typeof x !== 'object') return;
    for (const [k, y] of Object.entries(x)) {
      if ((k === 'marking' || k === 'level') && LEVELS.includes(y) && (top === null || rank(y) > rank(top))) top = y;
      walk(y);
    }
  };
  walk(v);
  return top;
}

function markingOf(name, id, x, entities) {
  const own = itemMarking(x);
  const c = ov.collections[name] ?? { ids: {} };
  const named = c.ids[id];
  let m;
  if (own?.unknown !== undefined) {
    if (!named) die(`${name}/${id}: unrecognised marking ${JSON.stringify(own.unknown)}; name the row in overlay.json`);
    m = named;
  } else if (named) {
    if (own && !covers(named, own)) die(`${name}/${id}: overlay marks it ${show(named)}, below its own marking ${show(own)}`);
    m = named;
  } else if (own) {
    m = own;
  } else if (name === 'heimdall.tracks') {
    if (!entities.has(id)) die(`${name}/${id}: no heimdall.entities row with this id to take a marking from`);
    m = entities.get(id);
  } else {
    m = c.default ?? ov.default;
  }
  const floor = innerFloor(x);
  if (floor && rank(m.level) < rank(floor)) die(`${name}/${id}: marked ${m.level}, below ${floor} inside the row`);
  return m;
}

// Canonical JSON: sorted keys, no whitespace.
const canon = (v) => JSON.stringify(v, (_, x) =>
  x && typeof x === 'object' && !Array.isArray(x) ? Object.fromEntries(Object.keys(x).sort().map((k) => [k, x[k]])) : x);

// Read every collection first, then mark: heimdall.tracks take the marking of the
// heimdall.entities row with the same id, so entities are marked before tracks.
const byConsole = new Map(Object.keys(fresh).map((c) => [c, {}]));
const read_ = [];
for (const [name, type, read, idOf] of C) {
  const console_ = name.split('.')[0];
  const raw = await read(fresh[console_]());
  const seen = new Set();
  const rows = raw.map((x, i) => {
    const { __id, ...item } = x;
    const id = String(__id ?? idOf?.(item) ?? item.id ?? item.name ?? i);
    if (seen.has(id)) die(`${name}: duplicate row id ${id}; give the collection an id function`);
    seen.add(id);
    return { id, item };
  });
  for (const id of Object.keys(ov.collections[name]?.ids ?? {})) if (!seen.has(id)) die(`overlay names ${name}/${id}, which does not exist`);
  read_.push({ name, console_, type, rows });
}
const entityMarkings = new Map();
const order = (r) => (r.name === 'heimdall.entities' ? 0 : 1);
for (const col of [...read_].sort((a, b) => order(a) - order(b))) {
  for (const r of col.rows) r.marking = markingOf(col.name, r.id, r.item, entityMarkings);
  if (col.name === 'heimdall.entities') for (const r of col.rows) entityMarkings.set(r.id, r.marking);
}
for (const { name, console_, type, rows } of read_) {
  byConsole.get(console_)[name] = { type, rows: rows.map(({ id, marking, item }) => ({ id, marking, item })) };
}

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
  relative(ROOT, overlayPath), 'clients/demo/tools/extract-fixtures.mjs'].map((p) => [p, blob(p)]));
writeFileSync(join(out, 'manifest.json'), canon({ format: 'midgard-demo-fixtures', version: 1, release, files, inputs }));
console.log(`extract-fixtures: ${C.length} collections + motion -> ${relative(ROOT, out) || out}`);
