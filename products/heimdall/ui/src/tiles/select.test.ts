/* Which globe tiles to draw and fetch: the area in view, within the budget. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { type SelectOptions, groundViewAngleDeg, selectGlobeTiles } from './select.ts';
import { type Tile, ancestorAt, tileAt, tileBounds, tileKey } from './tileMath.ts';

const OPTS: SelectOptions = { minZoom: 2, maxZoom: 14, budget: 160, viewportPx: 900, aspect: 1.6, fovDeg: 45 };
const all = () => true;
const none = () => false;

function disjoint(tiles: Tile[]): boolean {
  for (const a of tiles) {
    for (const b of tiles) {
      if (a === b) continue;
      if (b.z > a.z && tileKey(ancestorAt(b, a.z)) === tileKey(a)) return false;
      if (b.z === a.z && tileKey(a) === tileKey(b)) return false;
    }
  }
  return true;
}

test('the ground in view widens with altitude up to the horizon', () => {
  const low = groundViewAngleDeg(10, 45, 1.6);
  const mid = groundViewAngleDeg(1000, 45, 1.6);
  const high = groundViewAngleDeg(40000, 45, 1.6);
  assert.ok(low < 0.2, `${low}`);
  assert.ok(mid > low && high > mid);
  // From 40,000 km the screen's corners look past the Earth: the horizon, about 82.1°.
  const horizon = Math.acos(1 / (1 + 40000 / 6371.0088)) * (180 / Math.PI);
  assert.ok(Math.abs(high - horizon) < 1e-9, `${high}`);
});

test('from far out only the coarsest tiles on the near side are drawn', () => {
  const { draw, want } = selectGlobeTiles({ lat: 30, lon: -10, altKm: 20000 }, OPTS, all);
  assert.equal(want.length, 0);
  assert.ok(draw.length > 0 && draw.length <= 48);
  assert.ok(draw.every((t) => t.z <= 3));
  // The far side of the globe is not drawn: the Pacific at 0°, 170° E.
  const far = tileAt(0, 170, 2);
  assert.ok(!draw.some((t) => tileKey(ancestorAt(t, 2)) === tileKey(far)));
});

test('close in, detail gathers under the camera and stays inside the budget', () => {
  const cam = { lat: 56.8, lon: -5.0, altKm: 5 };
  const { draw, want } = selectGlobeTiles(cam, OPTS, all);
  assert.equal(want.length, 0);
  assert.ok(draw.length <= OPTS.budget);
  assert.ok(disjoint(draw));
  const under = draw.find((t) => {
    const b = tileBounds(t);
    return cam.lat <= b.north && cam.lat >= b.south && cam.lon >= b.west && cam.lon <= b.east;
  });
  assert.ok(under, 'a tile under the camera');
  assert.ok(under.z >= 12, `zoom ${under.z}`);
  // Nothing on the other side of the world is fetched for a close-up of Scotland.
  assert.ok(draw.every((t) => tileBounds(t).east > -60 && tileBounds(t).west < 50));
});

test('a small budget gives coarser tiles, never more of them', () => {
  const cam = { lat: 56.8, lon: -5.0, altKm: 5 };
  const { draw, want } = selectGlobeTiles(cam, { ...OPTS, budget: 24 }, all);
  assert.ok(draw.length + want.length <= 24, `${draw.length + want.length}`);
  assert.ok(disjoint(draw));
});

test('nothing loaded: the visible root tiles are wanted and nothing is drawn', () => {
  const { draw, want } = selectGlobeTiles({ lat: 54, lon: -3, altKm: 2000 }, OPTS, none);
  assert.equal(draw.length, 0);
  assert.ok(want.length > 0);
  assert.ok(want.every((t) => t.z === OPTS.minZoom));
});

test('a tile splits only when all its children are ready; until then it is drawn and they are wanted', () => {
  const cam = { lat: 54, lon: -3, altKm: 2000 };
  // Only zoom 2 is loaded.
  const { draw, want } = selectGlobeTiles(cam, OPTS, (t) => t.z === 2);
  assert.ok(draw.length > 0 && draw.every((t) => t.z === 2));
  assert.ok(want.length > 0 && want.every((t) => t.z === 3));
  assert.ok(disjoint([...draw, ...want].filter((t) => t.z === 2)));
  // Wanted tiles are ordered nearest the camera first.
  const ground = tileAt(cam.lat, cam.lon, 3);
  assert.equal(tileKey(want[0]), tileKey(ground));
});

test('the maximum zoom caps detail', () => {
  const { draw } = selectGlobeTiles({ lat: 56.8, lon: -5, altKm: 2 }, { ...OPTS, maxZoom: 9 }, all);
  assert.ok(draw.every((t) => t.z <= 9));
  assert.ok(draw.some((t) => t.z === 9));
});
