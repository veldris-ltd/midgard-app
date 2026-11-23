/* The map's on-screen byte cap: tiles drawn, stand-ins and requests, nearest first.
 * [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { type Tile, tileKey } from '../tiles/tileMath.ts';
import { ON_SCREEN_BYTES, planView } from './onScreen.ts';

const MB = 1024 * 1024;
const row = (z: number, y: number, n: number): Tile[] => Array.from({ length: n }, (_, x) => ({ z, x, y }));
const O = { maxRequested: 96, maxBytes: 10 * MB, ancestorLevels: 5 };

test('the cap is 128 MB', () => {
  assert.equal(ON_SCREEN_BYTES, 128 * MB);
});

test('within the cap: every loaded tile is drawn from itself, the rest asked for', () => {
  const tiles = row(8, 10, 6);
  const loaded = new Map(tiles.slice(0, 3).map((t) => [tileKey(t), { bytes: 1 * MB }]));
  const p = planView(tiles, (t) => loaded.get(tileKey(t)), tileKey, O);
  assert.deepEqual(p.draw.map((d) => d?.up ?? null), [0, 0, 0, null, null, null]);
  assert.deepEqual(p.request.map(tileKey), tiles.slice(3).map(tileKey));
  assert.equal(p.bytes, 3 * MB);
  assert.equal(p.capped, false);
});

test('past the cap: no more tiles are asked for, and a loaded parent stands in', () => {
  // Eight children of one parent, nearest first; each child 3 MB, the parent 2 MB.
  const parentTile = { z: 7, x: 5, y: 5 };
  const tiles: Tile[] = [];
  for (let y = 10; y <= 11; y++) for (let x = 10; x <= 11; x++) tiles.push({ z: 8, x, y });
  tiles.push(...row(8, 12, 4).map((t) => ({ ...t, x: t.x + 10 })));
  const data = new Map<string, { bytes: number }>();
  for (const t of tiles.slice(0, 4)) data.set(tileKey(t), { bytes: 3 * MB });
  data.set(tileKey(parentTile), { bytes: 2 * MB });
  const p = planView(tiles, (t) => data.get(tileKey(t)), tileKey, O);
  // 3 + 3 + 3 = 9 MB; the fourth child (12 MB) would pass 10 MB, so its parent (11 MB)
  // does not fit either: nothing there. The cap stops every later request.
  assert.deepEqual(p.draw.slice(0, 4).map((d) => d && `${d.up}`), ['0', '0', '0', null]);
  assert.equal(p.bytes, 9 * MB);
  assert.equal(p.capped, true);
  assert.deepEqual(p.request, []);
  // With room for the parent but not the child, the parent stands in.
  const q = planView(tiles, (t) => data.get(tileKey(t)), tileKey, { ...O, maxBytes: 11 * MB });
  assert.equal(q.draw[3]?.up, 1);
  assert.deepEqual(q.draw[3]?.from, parentTile);
  assert.equal(q.bytes, 11 * MB);
  assert.deepEqual(q.request, []);
});

test('a stand-in shared by many tiles is counted once', () => {
  const tiles: Tile[] = [];
  for (let y = 10; y <= 11; y++) for (let x = 10; x <= 11; x++) tiles.push({ z: 8, x, y });
  const p = planView(tiles, (t) => (t.z === 7 ? { bytes: 4 * MB } : undefined), tileKey, O);
  assert.ok(p.draw.every((d) => d?.up === 1));
  assert.equal(p.bytes, 4 * MB);
  assert.equal(p.request.length, 4);
});

test('a huge screen of loaded tiles never draws more than the cap', () => {
  const tiles = Array.from({ length: 40 }, (_, i) => row(12, 100 + i, 40)).flat();
  const p = planView(tiles, () => ({ bytes: 1 * MB }), tileKey, { ...O, maxBytes: ON_SCREEN_BYTES });
  assert.ok(p.bytes <= ON_SCREEN_BYTES);
  assert.ok(p.capped);
  // The parents are not loaded here, so past the cap nothing more is drawn or asked for.
  assert.equal(p.draw.filter(Boolean).length, 128);
  assert.equal(p.request.length, 0);
});

test('a tile the source does not have stops the search there, as before', () => {
  const t = { z: 8, x: 1, y: 1 };
  const p = planView([t], (x) => (x.z === 8 ? null : { bytes: 1 }), tileKey, O);
  assert.deepEqual(p.draw, [null]);
  assert.deepEqual(p.used, [t]);
  assert.deepEqual(p.request, []);
});

test('at most maxRequested tiles are asked for', () => {
  const p = planView(row(10, 3, 200), () => undefined, tileKey, O);
  assert.equal(p.request.length, 96);
});
