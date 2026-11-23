/* The map tile source, on the built bundle, for each of the three modes:
 * - none (unset, or VITE_HEIMDALL_TILE_SOURCE=none): no tile host, tile path, tile
 *   code or decode worker anywhere in dist/;
 * - mirror: the mirror's origin only, and none of the public hosts;
 * - public: the three public hosts, and no mirror.
 * Any other value fails the build. Each mode builds the same bytes twice. Builds into
 * temporary directories; no network. Run with `pnpm test:bundle`. [FR-H-1] [NFR-PRV-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ui = fileURLToPath(new URL('..', import.meta.url));
const vite = join(ui, 'node_modules', '.bin', 'vite');

const MIRROR = 'https://tiles.mirror-under-test.internal/heimdall';

/** What marks the public tile hosts in a bundle. */
const PUBLIC_MARKS = ['s3.amazonaws.com', 'elevation-tiles-prod', 'tiles.maps.eox.at', 's2cloudless_3857', 'wmts/1.0.0', 'tiles.versatiles.org'];
/** What marks any tile code at all (the paths, the mirror, the notice). */
const TILE_MARKS = [...PUBLIC_MARKS, 'terrarium', 's2cloudless', 'versatiles', 'mirror-under-test', 'MAP TILES PAUSED', 'MAP TILES NEED'];

const BASE_ENV = (() => {
  const env = { ...process.env };
  for (const k of ['VITE_HEIMDALL_TILE_SOURCE', 'VITE_HEIMDALL_TILE_MIRROR', 'VITE_HEIMDALL_TILES']) delete env[k];
  return env;
})();

function run(vars) {
  const out = mkdtempSync(join(tmpdir(), 'heimdall-tiles-'));
  const r = spawnSync(vite, ['build', '--outDir', out, '--emptyOutDir', '--logLevel', 'error'], { cwd: ui, env: { ...BASE_ENV, ...vars }, encoding: 'utf8' });
  return { out, r };
}

function build(vars) {
  const { out, r } = run(vars);
  assert.equal(r.status, 0, `vite build failed for ${JSON.stringify(vars)}:\n${r.stdout}\n${r.stderr}`);
  return out;
}

function files(dir) {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? files(p) : [p];
  });
}

function marksIn(dir, marks) {
  const found = new Map();
  for (const f of files(dir)) {
    const text = readFileSync(f).toString('latin1');
    for (const m of marks) if (text.includes(m)) found.set(m, [...(found.get(m) ?? []), f.slice(dir.length + 1)]);
  }
  return Object.fromEntries(found);
}

const names = (dir) => files(dir).map((f) => f.slice(dir.length + 1)).sort();

/** Every file's SHA-256, by name: two builds of one mode must match. */
function digest(dir) {
  return Object.fromEntries(names(dir).map((n) => [n, createHash('sha256').update(readFileSync(join(dir, n))).digest('hex')]));
}

function withBuilds(vars, check) {
  const a = build(vars);
  const b = build(vars);
  try {
    check(a);
    assert.deepEqual(digest(b), digest(a), `two builds of ${JSON.stringify(vars)} differ`);
  } finally {
    rmSync(a, { recursive: true, force: true });
    rmSync(b, { recursive: true, force: true });
  }
}

test('none, by default and when set: no tile host, path, code or worker in dist/', () => {
  for (const vars of [{}, { VITE_HEIMDALL_TILE_SOURCE: 'none' }]) {
    withBuilds(vars, (out) => {
      assert.deepEqual(marksIn(out, TILE_MARKS), {}, JSON.stringify(vars));
      const n = names(out);
      assert.ok(!n.some((x) => /globeTiles|MapView|loader-|decodeWorker|active-/.test(x)), n.join(', '));
      // And it is still the console, with its globe: not an empty or broken build.
      assert.ok(n.includes('index.html'), n.join(', '));
      assert.ok(n.some((x) => /^assets\/GlobeView-.*\.js$/.test(x)), n.join(', '));
    });
  }
});

test('mirror: the mirror origin only; no public host; decoding in its own worker file', () => {
  withBuilds({ VITE_HEIMDALL_TILE_SOURCE: 'mirror', VITE_HEIMDALL_TILE_MIRROR: MIRROR }, (out) => {
    assert.deepEqual(marksIn(out, PUBLIC_MARKS), {});
    const found = marksIn(out, [MIRROR, '/terrarium/', '/s2cloudless-2016/', '/osm/']);
    assert.ok(found[MIRROR], 'the mirror is in the bundle');
    const n = names(out);
    assert.ok(n.some((x) => /^assets\/decodeWorker-.*\.js$/.test(x)), n.join(', '));
    // A same-origin module worker from its own file, never a blob: URL, so a CSP holds.
    assert.ok(marksIn(out, ['{type:"module",name:"heimdall-tile-decode"}'])['{type:"module",name:"heimdall-tile-decode"}']);
    assert.deepEqual(marksIn(out, ['createObjectURL']), {});
  });
});

test('public: the three public hosts, and no mirror', () => {
  withBuilds({ VITE_HEIMDALL_TILE_SOURCE: 'public' }, (out) => {
    const found = marksIn(out, [...PUBLIC_MARKS, 'mirror-under-test']);
    for (const host of ['s3.amazonaws.com', 'tiles.maps.eox.at', 'tiles.versatiles.org']) assert.ok(found[host], host);
    assert.equal(found['mirror-under-test'], undefined);
  });
});

test('any other setting fails the build with a clear error', () => {
  const bad = [
    [{ VITE_HEIMDALL_TILE_SOURCE: '' }, /none, mirror or public/],
    [{ VITE_HEIMDALL_TILE_SOURCE: 'publik' }, /none, mirror or public/],
    [{ VITE_HEIMDALL_TILE_SOURCE: 'Public' }, /none, mirror or public/],
    [{ VITE_HEIMDALL_TILE_SOURCE: 'mirror' }, /VITE_HEIMDALL_TILE_MIRROR is required/],
    [{ VITE_HEIMDALL_TILE_SOURCE: 'mirror', VITE_HEIMDALL_TILE_MIRROR: 'http://tiles.internal' }, /https/],
    [{ VITE_HEIMDALL_TILE_MIRROR: MIRROR }, /not mirror/],
    [{ VITE_HEIMDALL_TILES: 'off' }, /retired/],
    [{ VITE_HEIMDALL_TILE_SOURCE: 'mirror', VITE_HEIMDALL_TILE_MIRROR: 'https://tiles.versatiles.org' }, /public tile host/],
    [{ VITE_HEIMDALL_TILE_SOURCE: 'mirror', VITE_HEIMDALL_TILE_MIRROR: 'https://bucket.s3.amazonaws.com/t' }, /public tile host/],
  ];
  for (const [vars, why] of bad) {
    const { out, r } = run(vars);
    try {
      assert.notEqual(r.status, 0, `built with ${JSON.stringify(vars)}`);
      assert.match(`${r.stdout}\n${r.stderr}`, why, JSON.stringify(vars));
    } finally {
      rmSync(out, { recursive: true, force: true });
    }
  }
});

test('a mirror on a public IP address builds, with a warning', () => {
  const { out, r } = run({ VITE_HEIMDALL_TILE_SOURCE: 'mirror', VITE_HEIMDALL_TILE_MIRROR: 'https://203.0.113.7/m' });
  try {
    assert.equal(r.status, 0, `${r.stdout}\n${r.stderr}`);
    assert.match(`${r.stdout}\n${r.stderr}`, /warning: .*public IP address/);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});
