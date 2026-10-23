// Unit tests for the close-up detail's vertex grid (`node --test`, types stripped).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { detailGrid, gridIndex } from '../src/app/globe/detailGrid.ts';

test('a whole view is about square: 256 each way', () => {
  const lat = 51.5, span = 0.8, cos = Math.cos((lat * Math.PI) / 180);
  assert.deepEqual(detailGrid({ west: -span / cos / 2, east: span / cos / 2, south: lat - span / 2, north: lat + span / 2 }), { gw: 256, gh: 256 });
});

test('narrow clipped strips keep the long side at 256 and the short side small', () => {
  // A tall sliver at the east edge of the recorded box (lon 3.9-4.0, lat 50-52).
  assert.deepEqual(detailGrid({ west: 3.9, east: 4, south: 50, north: 52 }), { gw: 16, gh: 256 });
  // A wide, thin strip at the south edge.
  assert.deepEqual(detailGrid({ west: -2, east: 2, south: 48, north: 48.05 }), { gw: 256, gh: 16 });
  // Half a view: half the cells on the short side.
  const g = detailGrid({ west: 0, east: 1, south: 50, north: 50 + Math.cos((50.25 * Math.PI) / 180) * 0.5 });
  assert.equal(g.gw, 256);
  assert.ok(Math.abs(g.gh - 128) <= 1, String(g.gh));
  // Never more than 257 x 257 vertices, whatever the box.
  for (const b of [{ west: 0, east: 1e-6, south: 0, north: 10 }, { west: 0, east: 10, south: 0, north: 1e-6 }]) {
    const { gw, gh } = detailGrid(b);
    assert.ok(gw <= 256 && gh <= 256 && gw >= 16 && gh >= 16);
  }
});

test('the index covers every cell with two triangles, in 32 bits', () => {
  const idx = gridIndex(256, 256);
  assert.ok(idx instanceof Uint32Array);
  assert.equal(idx.length, 256 * 256 * 6);
  assert.equal(Math.max(...idx.subarray(idx.length - 6)), 257 * 257 - 1);
  assert.deepEqual([...gridIndex(1, 1)], [0, 2, 1, 1, 2, 3]);
});
