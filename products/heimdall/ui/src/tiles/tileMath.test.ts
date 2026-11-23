/* Web Mercator tile maths, shared by the globe's tiles and the 2D map. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  MAX_MERCATOR_LAT,
  ancestorAt,
  children,
  latToTileY,
  lonToTileX,
  metresPerPixel,
  parent,
  subRect,
  tileAt,
  tileBounds,
  tileKey,
  tileXToLon,
  tileYToLat,
  validTile,
  zoomForSpan,
} from './tileMath.ts';

const close = (a: number, b: number, eps = 1e-9) => assert.ok(Math.abs(a - b) <= eps, `${a} != ${b}`);

test('zoom 0 is one tile: x runs west to east, y north to south', () => {
  close(lonToTileX(-180, 0), 0);
  close(lonToTileX(0, 0), 0.5);
  close(lonToTileX(180, 0), 1);
  close(latToTileY(0, 0), 0.5);
  close(latToTileY(MAX_MERCATOR_LAT, 0), 0, 1e-9);
  close(latToTileY(-MAX_MERCATOR_LAT, 0), 1, 1e-9);
});

test('tile coordinates and positions round-trip', () => {
  for (const [lat, lon] of [[57.35, -4.2], [-33.9, 151.2], [0, 0], [84, 179.9]]) {
    for (const z of [0, 5, 12, 14]) {
      close(tileYToLat(latToTileY(lat, z), z), lat, 1e-9);
      close(tileXToLon(lonToTileX(lon, z), z), lon, 1e-9);
    }
  }
});

test('latitudes past the Mercator limit clamp to the edge rows', () => {
  close(latToTileY(89, 3), 0);
  close(latToTileY(-89, 3), 8);
});

test('the tile holding a point, and its bounds', () => {
  // Inverness, at zoom 6, is in tile 6/31/19.
  assert.deepEqual(tileAt(57.48, -4.22, 6), { z: 6, x: 31, y: 19 });
  const b = tileBounds({ z: 6, x: 31, y: 19 });
  close(b.west, -5.625);
  close(b.east, 0);
  assert.ok(b.north > 57.48 && b.south < 57.48);
  close(b.north, tileYToLat(19, 6));
  close(b.south, tileYToLat(20, 6));
  // A point on the antimeridian or the pole still lands in a real tile.
  assert.deepEqual(tileAt(0, 180, 2), { z: 2, x: 3, y: 2 });
  assert.deepEqual(tileAt(90, 0, 2), { z: 2, x: 2, y: 0 });
});

test('keys, parents, children and ancestors', () => {
  const t = { z: 6, x: 31, y: 19 };
  assert.equal(tileKey(t), '6/31/19');
  assert.deepEqual(parent(t), { z: 5, x: 15, y: 9 });
  assert.equal(parent({ z: 0, x: 0, y: 0 }), null);
  assert.deepEqual(children(t), [
    { z: 7, x: 62, y: 38 }, { z: 7, x: 63, y: 38 },
    { z: 7, x: 62, y: 39 }, { z: 7, x: 63, y: 39 },
  ]);
  assert.deepEqual(ancestorAt({ z: 14, x: 8020, y: 5220 }, 12), { z: 12, x: 2005, y: 1305 });
  assert.deepEqual(ancestorAt(t, 9), t);
});

test('subRect places a tile inside an ancestor', () => {
  assert.deepEqual(subRect({ z: 6, x: 31, y: 19 }, 6), { ox: 0, oy: 0, size: 1 });
  assert.deepEqual(subRect({ z: 7, x: 63, y: 38 }, 6), { ox: 0.5, oy: 0, size: 0.5 });
  assert.deepEqual(subRect({ z: 8, x: 126, y: 79 }, 6), { ox: 0.5, oy: 0.75, size: 0.25 });
});

test('valid tiles are in range at a non-negative integer zoom', () => {
  assert.ok(validTile({ z: 0, x: 0, y: 0 }));
  assert.ok(validTile({ z: 3, x: 7, y: 7 }));
  assert.ok(!validTile({ z: 3, x: 8, y: 0 }));
  assert.ok(!validTile({ z: 3, x: -1, y: 0 }));
  assert.ok(!validTile({ z: -1, x: 0, y: 0 }));
  assert.ok(!validTile({ z: 2.5, x: 0, y: 0 }));
  assert.ok(!validTile({ z: 31, x: 0, y: 0 }));
});

test('ground resolution halves with each zoom and shrinks with latitude', () => {
  close(metresPerPixel(0, 0), 156543.03392804097, 1e-6);
  close(metresPerPixel(0, 1), metresPerPixel(0, 0) / 2, 1e-6);
  close(metresPerPixel(60, 0), metresPerPixel(0, 0) / 2, 1e-6);
});

test('zoomForSpan: the zoom that fits a ground span across a viewport', () => {
  // 256 px showing the whole equator is zoom 0.
  close(zoomForSpan(40075.016685578485, 0, 256), 0, 1e-9);
  close(zoomForSpan(40075.016685578485 / 8, 0, 256), 3, 1e-9);
  // At 60° a tile spans half as many km, so the same span needs a zoom less.
  close(zoomForSpan(40075.016685578485 / 8, 60, 256), 2, 1e-9);
});
