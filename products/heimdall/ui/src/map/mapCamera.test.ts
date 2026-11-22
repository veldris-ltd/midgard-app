/* The 2D geographic map's camera: Web Mercator, pan, zoom at a point, the tiles in
 * view. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  MAP_MAX_ZOOM,
  MAP_MIN_ZOOM,
  clampMapCamera,
  panMap,
  project,
  unproject,
  nearestCopyX,
  visibleTiles,
  zoomForAltitude,
  zoomMapAt,
} from './mapCamera.ts';

const close = (a: number, b: number, eps = 1e-6) => assert.ok(Math.abs(a - b) <= eps, `${a} != ${b}`);
const W = 1000;
const H = 600;
const cam = { lat: 56.5, lon: -3.5, zoom: 8 };

test('the camera\'s centre projects to the middle of the viewport, and back', () => {
  const p = project(cam.lat, cam.lon, cam, W, H);
  close(p.x, W / 2);
  close(p.y, H / 2);
  const q = unproject(123, 456, cam, W, H);
  const r = project(q.lat, q.lon, cam, W, H);
  close(r.x, 123);
  close(r.y, 456);
});

test('east is right and north is up; a zoom step doubles distances', () => {
  const e = project(cam.lat, cam.lon + 1, cam, W, H);
  const n = project(cam.lat + 1, cam.lon, cam, W, H);
  assert.ok(e.x > W / 2 && Math.abs(e.y - H / 2) < 1e-6);
  assert.ok(n.y < H / 2);
  const e2 = project(cam.lat, cam.lon + 1, { ...cam, zoom: 9 }, W, H);
  close(e2.x - W / 2, 2 * (e.x - W / 2));
});

test('dragging moves the ground with the pointer', () => {
  const moved = panMap(cam, 100, 50, W, H);
  // The point that was under (400, 250) is now under (500, 300).
  const before = unproject(400, 250, cam, W, H);
  const after = project(before.lat, before.lon, moved, W, H);
  close(after.x, 500);
  close(after.y, 300);
});

test('zooming at a point keeps that point still', () => {
  const at = unproject(800, 100, cam, W, H);
  const z = zoomMapAt(cam, 10.5, 800, 100, W, H);
  close(z.zoom, 10.5);
  const p = project(at.lat, at.lon, z, W, H);
  close(p.x, 800);
  close(p.y, 100);
});

test('the camera keeps to the zoom range and the Mercator world', () => {
  assert.equal(clampMapCamera({ lat: 0, lon: 0, zoom: 1 }).zoom, MAP_MIN_ZOOM);
  assert.equal(clampMapCamera({ lat: 0, lon: 0, zoom: 30 }).zoom, MAP_MAX_ZOOM);
  assert.ok(clampMapCamera({ lat: 89, lon: 0, zoom: 5 }).lat < 85.06);
  close(clampMapCamera({ lat: 0, lon: 190, zoom: 5 }).lon, -170);
});

test('the tiles in view cover the viewport, at the zoom below the camera\'s, capped at the source\'s', () => {
  const v = visibleTiles({ ...cam, zoom: 8.5 }, W, H, 14);
  assert.equal(v.z, 8);
  close(v.size, 256 * Math.SQRT2);
  const left = Math.min(...v.tiles.map((t) => t.left));
  const top = Math.min(...v.tiles.map((t) => t.top));
  const right = Math.max(...v.tiles.map((t) => t.left + v.size));
  const bottom = Math.max(...v.tiles.map((t) => t.top + v.size));
  assert.ok(left <= 0 && top <= 0 && right >= W && bottom >= H);
  assert.ok(v.tiles.length <= Math.ceil(W / v.size + 1) * Math.ceil(H / v.size + 1));
  // Nearest the centre first.
  const c = v.tiles[0];
  assert.ok(c.left <= W / 2 && c.left + v.size >= W / 2 && c.top <= H / 2 && c.top + v.size >= H / 2);
  // Past the source's deepest zoom, its tiles are drawn larger.
  const deep = visibleTiles({ ...cam, zoom: 15.5 }, W, H, 14);
  assert.equal(deep.z, 14);
  close(deep.size, 256 * 2 ** 1.5);
});

test('a globe camera altitude becomes a map zoom showing the same span', () => {
  const near = zoomForAltitude(50, 56, 800);
  const far = zoomForAltitude(2000, 56, 800);
  assert.ok(near > far);
  close(near - zoomForAltitude(100, 56, 800), 1, 1e-9);
});

test('across the antimeridian the tiles wrap round the world', () => {
  const c = { lat: -17, lon: 179.5, zoom: 5 };
  const v = visibleTiles(c, W, H, 14);
  const xs = new Set(v.tiles.map((t) => t.tile.x));
  assert.ok(xs.has(31) && xs.has(0), [...xs].join(','));
  assert.ok(v.tiles.every((t) => t.tile.x >= 0 && t.tile.x < 32));
  // Tile 0 is drawn east of tile 31, not a world away.
  const t31 = v.tiles.find((t) => t.tile.x === 31 && t.tile.y === v.tiles[0].tile.y)!;
  const t0 = v.tiles.find((t) => t.tile.x === 0 && t.tile.y === v.tiles[0].tile.y)!;
  close(t0.left - t31.left, v.size);
});

test('a position is drawn at the copy of the world nearest the viewport', () => {
  const c = { lat: -17, lon: 179.5, zoom: 5 };
  // Fiji's Lau Group, at 178.5° W: just east of the antimeridian, so just right of centre.
  const p = project(-17, -178.5, c, W, H);
  const x = nearestCopyX(p.x, W, c.zoom);
  assert.ok(x > W / 2 && x < W / 2 + 200, `${x}`);
  close(nearestCopyX(W / 2, W, c.zoom), W / 2);
});
