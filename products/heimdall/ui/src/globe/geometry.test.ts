/* Line geometry for the globe, and reading the design tokens' colours. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { graticule, lineSegments } from './geometry.ts';
import { parseCssColour } from './colours.ts';
import { fromXyz } from './projection.ts';

const at = (pos: Float32Array, i: number) => fromXyz({ x: pos[3 * i], y: pos[3 * i + 1], z: pos[3 * i + 2] }, 1.001);

test('lineSegments emits a pair of points per segment at the given radius', () => {
  const pos = lineSegments([Float64Array.from([0, 0, 0.5, 0, 0.5, 0.5])], 1.001, 1);
  assert.equal(pos.length, 2 * 2 * 3);
  const first = at(pos, 0);
  assert.ok(Math.abs(first.altKm) < 1e-3 && Math.abs(first.lat) < 1e-5 && Math.abs(first.lon) < 1e-5);
  const last = at(pos, 3);
  assert.ok(Math.abs(last.lat - 0.5) < 1e-5 && Math.abs(last.lon - 0.5) < 1e-5);
});

test('long segments are split so they follow the curve instead of cutting under it', () => {
  const pos = lineSegments([Float64Array.from([0, 0, 10, 0])], 1.001, 1);
  assert.equal(pos.length / 6, 10);
  for (let i = 0; i < pos.length / 3; i++) assert.ok(Math.abs(at(pos, i).lat) < 1e-5);
});

test('the graticule has meridians and parallels every 15°', () => {
  const lines = graticule(15, 2);
  // 24 meridians and 11 parallels (75° S to 75° N).
  assert.equal(lines.length, 24 + 11);
});

test('parseCssColour reads computed rgb(), rgba() and color(srgb) forms', () => {
  assert.deepEqual(parseCssColour('rgb(240, 168, 36)'), { r: 240 / 255, g: 168 / 255, b: 36 / 255, a: 1 });
  assert.deepEqual(parseCssColour('rgba(255, 255, 255, 0.07)'), { r: 1, g: 1, b: 1, a: 0.07 });
  assert.deepEqual(parseCssColour('rgb(10 11 13 / 50%)'), { r: 10 / 255, g: 11 / 255, b: 13 / 255, a: 0.5 });
  assert.deepEqual(parseCssColour('color(srgb 1 0.5 0 / 0.25)'), { r: 1, g: 0.5, b: 0, a: 0.25 });
  assert.equal(parseCssColour('var(--nope)'), null);
  assert.equal(parseCssColour(''), null);
});
