/* The map tiles switch, on the built bundle: with VITE_HEIMDALL_TILES off, no tile
 * host, no tile path and no tile code is anywhere in dist/; with it on, they are (so
 * the check is not vacuous). Builds the console twice into temporary directories;
 * no network. Run with `pnpm test:bundle`. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ui = fileURLToPath(new URL('..', import.meta.url));
const vite = join(ui, 'node_modules', '.bin', 'vite');

/** Everything that would mark a tile request or the tile code in a bundle. */
const TILE_MARKS = [
  's3.amazonaws.com',
  'elevation-tiles-prod',
  'terrarium',
  'tiles.maps.eox.at',
  's2cloudless',
  'tiles.versatiles.org',
  'versatiles',
];

function build(switchValue) {
  const out = mkdtempSync(join(tmpdir(), 'heimdall-tiles-'));
  const env = { ...process.env };
  if (switchValue === undefined) delete env.VITE_HEIMDALL_TILES;
  else env.VITE_HEIMDALL_TILES = switchValue;
  const r = spawnSync(vite, ['build', '--outDir', out, '--emptyOutDir', '--logLevel', 'error'], { cwd: ui, env, encoding: 'utf8' });
  assert.equal(r.status, 0, `vite build failed:\n${r.stdout}\n${r.stderr}`);
  return out;
}

function files(dir) {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? files(p) : [p];
  });
}

function marksIn(dir) {
  const found = new Map();
  for (const f of files(dir)) {
    const text = readFileSync(f).toString('latin1');
    for (const m of TILE_MARKS) if (text.includes(m)) found.set(m, [...(found.get(m) ?? []), f.slice(dir.length + 1)]);
  }
  return found;
}

test('with the switch off, dist/ has no tile host, path or code', () => {
  for (const value of ['off', 'false', '0']) {
    const out = build(value);
    try {
      const found = marksIn(out);
      assert.deepEqual(Object.fromEntries(found), {}, `VITE_HEIMDALL_TILES=${value}`);
      const names = files(out).map((f) => f.slice(out.length + 1));
      assert.ok(!names.some((n) => /globeTiles|MapView|loader-/.test(n)), names.join(', '));
    } finally {
      rmSync(out, { recursive: true, force: true });
    }
  }
});

test('by default the switch is on, and the three tile hosts are in dist/', () => {
  const out = build(undefined);
  try {
    const found = marksIn(out);
    for (const host of ['s3.amazonaws.com', 'tiles.maps.eox.at', 'tiles.versatiles.org']) assert.ok(found.has(host), host);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});
