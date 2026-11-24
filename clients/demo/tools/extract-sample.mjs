// The sample data: what the signed release's web consoles show in mock mode
// (`sdk/ts/src/mock/`), as one body the core compiles in (SampleSource). [AC-C-13]
// [FR-C-1] [FR-C-5]
//
// Run through extract-sample.sh, which first extracts the fixtures afresh from the
// mocks (extract-fixtures.sh --out <tmp>) and passes that directory here. The body
// keeps every collection the mocks serve, row for row, with the fixtures' markings
// (the row's own marking, or the committed overlay's), and the personas named in the
// mocks. It drops what the mocks do not serve: the synthetic Heimdall hour
// (`heimdall.motion`, the extractor's own addition for the map). It adds one thing
// the mocks do not hold but a web screen does: Mimir's Agent Studio exchange
// (`clients/demo/studio.json`, copied once from `AgentStudioScreen.tsx`), served as
// `mimir.studioExchange`; the copy names the screen's git blob, and extraction stops
// when the screen is no longer that blob. Output is canonical JSON (sorted keys, no
// whitespace), as the fixtures are.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '../../..');
const arg = (name) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : undefined; };
const die = (m) => { console.error(`extract-sample: ${m}`); process.exit(1); };
const fixtures = arg('--fixtures') ?? die('--fixtures <dir> is required');
const out = arg('--out') ?? die('--out <file> is required');

/** Collections the extractor adds that the mocks do not serve. */
const NOT_IN_MOCKS = new Set(['heimdall.motion']);

const canon = (v) => JSON.stringify(v, (_, x) =>
  x && typeof x === 'object' && !Array.isArray(x) ? Object.fromEntries(Object.keys(x).sort().map((k) => [k, x[k]])) : x);
const read = (f) => readFileSync(join(fixtures, f), 'utf8');

const manifest = JSON.parse(read('manifest.json'));
if (manifest.format !== 'midgard-demo-fixtures') die('not a fixture directory');
const files = Object.keys(manifest.files).sort();
for (const f of files) {
  const digest = createHash('sha256').update(read(f)).digest('hex');
  if (digest !== manifest.files[f]) die(`${f}: digest differs from the manifest`);
}

const collections = {};
for (const f of files.filter((f) => f !== 'personas.json')) {
  for (const [name, c] of Object.entries(JSON.parse(read(f)).collections)) {
    if (NOT_IN_MOCKS.has(name)) continue;
    if (name in collections) die(`${name} appears twice`);
    collections[name] = c;
  }
}
for (const name of NOT_IN_MOCKS) if (name in collections) die(`${name} is not mock data`);

const blob = (p) => execFileSync('git', ['hash-object', join(ROOT, p)], { encoding: 'utf8' }).trim();

// Mimir's Agent Studio exchange: written in the web screen, not the mock.
const STUDIO = 'clients/demo/studio.json';
const studio = JSON.parse(readFileSync(join(ROOT, STUDIO), 'utf8'));
if (studio.format !== 'midgard-studio-exchange' || studio.version !== 1) die(`${STUDIO}: not a studio exchange`);
if (blob(studio.source.path) !== studio.source.blob)
  die(`${studio.source.path} is no longer the blob ${STUDIO} was copied from: copy the exchange again and update its blob`);
if ('mimir.studioExchange' in collections) die('mimir.studioExchange appears twice');
collections['mimir.studioExchange'] = {
  type: 'MimirStudioExchange',
  rows: [{ id: studio.exchange.id, marking: studio.marking, item: studio.exchange }],
};

const inputs = {
  ...manifest.inputs,
  'clients/demo/tools/extract-sample.mjs': blob('clients/demo/tools/extract-sample.mjs'),
  [STUDIO]: blob(STUDIO),
  [studio.source.path]: studio.source.blob,
};
const body = {
  format: 'midgard-sample-data',
  version: 1,
  release: manifest.release,
  inputs,
  personas: JSON.parse(read('personas.json')),
  collections,
};
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, canon(body));
const rows = Object.values(collections).reduce((n, c) => n + c.rows.length, 0);
console.log(`extract-sample: ${Object.keys(collections).length} collections, ${rows} rows -> ${out}`);
