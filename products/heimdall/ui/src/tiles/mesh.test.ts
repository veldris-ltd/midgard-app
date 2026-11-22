/* The terrain patch for one tile: vertices on the globe, raised by the heights. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { EARTH_RADIUS_KM, fromXyz, toXyz } from '../globe/projection.ts';
import { buildPatch, skirtDepthKm } from './mesh.ts';
import { tileBounds } from './tileMath.ts';

const close = (a: number, b: number, eps = 1e-9) => assert.ok(Math.abs(a - b) <= eps, `${a} != ${b}`);
const T = { z: 6, x: 31, y: 19 };

function vertex(p: ReturnType<typeof buildPatch>, i: number) {
  return { x: p.center.x + p.positions[i * 3], y: p.center.y + p.positions[i * 3 + 1], z: p.center.z + p.positions[i * 3 + 2] };
}

test('a flat patch: (n+1)² grid vertices on the sphere, then the skirt, all relative to its centre', () => {
  const n = 4;
  const p = buildPatch(T, null, n, 1);
  const grid = (n + 1) ** 2;
  const skirt = 4 * n;
  assert.equal(p.positions.length, (grid + skirt) * 3);
  assert.equal(p.uv.length, (grid + skirt) * 2);
  assert.equal(p.geoUv.length, (grid + skirt) * 2);
  assert.equal(p.normals.length, p.positions.length);
  // Two triangles per grid cell, and two per skirt segment.
  assert.equal(p.indices.length, (n * n * 2 + skirt * 2) * 3);
  assert.ok(Math.max(...p.indices) < grid + skirt);
  // The north-west and south-east corners sit on the tile's corners at sea level.
  const b = tileBounds(T);
  const nw = fromXyz(vertex(p, 0));
  // Float32 offsets: millimetres at most.
  close(nw.lat, b.north, 1e-6); close(nw.lon, b.west, 1e-6); close(nw.altKm, 0, 1e-4);
  const se = fromXyz(vertex(p, grid - 1));
  close(se.lat, b.south, 1e-6); close(se.lon, b.east, 1e-6);
  // Positions are small offsets from the centre, which keeps float32 precise close in.
  const c = Math.hypot(p.center.x, p.center.y, p.center.z);
  close(c, 1, 1e-3);
});

test('texture coordinates: the tile\'s own run north to south, the world\'s by longitude and latitude', () => {
  const n = 2;
  const p = buildPatch(T, null, n, 1);
  assert.deepEqual([p.uv[0], p.uv[1]], [0, 0]);
  const last = (n + 1) ** 2 - 1;
  assert.deepEqual([p.uv[last * 2], p.uv[last * 2 + 1]], [1, 1]);
  const b = tileBounds(T);
  close(p.geoUv[0], (b.west + 180) / 360, 1e-6);
  close(p.geoUv[1], (b.north + 90) / 180, 1e-6);
});

test('heights raise the vertices, exaggerated; below sea level stays at the sea surface', () => {
  const n = 1;
  const heights = new Float32Array([1000, -200, 0, 500]);
  const p = buildPatch(T, heights, n, 4);
  const alt = (i: number) => fromXyz(vertex(p, i)).altKm;
  close(alt(0), 4, 1e-4);
  close(alt(1), 0, 1e-4);
  close(alt(2), 0, 1e-4);
  close(alt(3), 2, 1e-4);
  assert.equal(p.maxKm, 4);
});

test('the skirt hangs below its edge vertex by the skirt depth', () => {
  const n = 2;
  const p = buildPatch(T, null, n, 1);
  const grid = (n + 1) ** 2;
  const depth = skirtDepthKm(T);
  assert.ok(depth > 0);
  for (let s = grid; s < grid + 4 * n; s++) close(fromXyz(vertex(p, s)).altKm, -depth, 1e-4);
});

test('normals point straight out from the globe\'s centre', () => {
  const p = buildPatch(T, new Float32Array([800, 800, 800, 800]), 1, 4);
  const v = vertex(p, 0);
  const r = Math.hypot(v.x, v.y, v.z);
  close(p.normals[0], v.x / r, 1e-6);
  close(p.normals[1], v.y / r, 1e-6);
  close(p.normals[2], v.z / r, 1e-6);
  // And the raise is in kilometres over the Earth's radius.
  close(r, 1 + 3.2 / EARTH_RADIUS_KM, 1e-9);
  void toXyz;
});

test('the wrong number of heights is refused', () => {
  assert.throws(() => buildPatch(T, new Float32Array(3), 1, 1), /heights/);
});
