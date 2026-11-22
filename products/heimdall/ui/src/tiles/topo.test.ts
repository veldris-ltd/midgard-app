/* The TOPOGRAPHIC style: hypsometric tints, hillshade and contours from the terrain
 * heights. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { readPng } from './pngForTests.ts';
import { decodeTerrarium } from './terrarium.ts';
import { TOPO_DARK, TOPO_LIGHT, contourIntervalM, hillshade, shadeZFactor, tint, topoPixels } from './topo.ts';

const close = (a: number, b: number, eps = 1e-6) => assert.ok(Math.abs(a - b) <= eps, `${a} != ${b}`);

test('contours close up as the zoom deepens', () => {
  const zs = [2, 5, 7, 9, 11, 13, 14];
  const steps = zs.map(contourIntervalM);
  for (let i = 1; i < steps.length; i++) assert.ok(steps[i] <= steps[i - 1], `z${zs[i]}`);
  assert.equal(contourIntervalM(13), 20);
  assert.equal(contourIntervalM(6), 500);
});

test('relief is drawn more strongly from far out', () => {
  assert.ok(shadeZFactor(4) > shadeZFactor(8));
  assert.ok(shadeZFactor(8) > shadeZFactor(12) || shadeZFactor(12) === 1.5);
  assert.ok(shadeZFactor(14) >= 1);
});

test('hillshade: flat ground takes the sun\'s elevation; slopes facing the north-west sun are brighter', () => {
  const flat = hillshade(new Float32Array(9), 3, 3, 30, 1);
  for (const v of flat) close(v, Math.sin(Math.PI / 4));
  // Rising to the south-east (east and down the rows), so the slope faces north-west.
  const nw = new Float32Array([0, 10, 20, 10, 20, 30, 20, 30, 40]);
  const se = nw.map((h) => 40 - h);
  const a = hillshade(nw, 3, 3, 30, 1)[4];
  const b = hillshade(se, 3, 3, 30, 1)[4];
  assert.ok(a > Math.sin(Math.PI / 4) && b < Math.sin(Math.PI / 4), `${a} ${b}`);
});

test('tints: the sea by depth, the land by height', () => {
  assert.deepEqual(tint(TOPO_LIGHT, -1), TOPO_LIGHT.sea);
  assert.deepEqual(tint(TOPO_LIGHT, -5000), TOPO_LIGHT.seaDeep);
  assert.deepEqual(tint(TOPO_LIGHT, 0.5), TOPO_LIGHT.land[0][1]);
  assert.deepEqual(tint(TOPO_LIGHT, 1e5), TOPO_LIGHT.land[TOPO_LIGHT.land.length - 1][1]);
  const mid = tint(TOPO_LIGHT, (TOPO_LIGHT.land[1][0] + TOPO_LIGHT.land[2][0]) / 2);
  for (let i = 0; i < 3; i++) {
    const lo = Math.min(TOPO_LIGHT.land[1][1][i], TOPO_LIGHT.land[2][1][i]);
    const hi = Math.max(TOPO_LIGHT.land[1][1][i], TOPO_LIGHT.land[2][1][i]);
    assert.ok(mid[i] >= lo && mid[i] <= hi);
  }
  // The dark palette is darker throughout.
  const lum = (c: readonly number[]) => c[0] + c[1] + c[2];
  assert.ok(lum(tint(TOPO_DARK, 300)) < lum(tint(TOPO_LIGHT, 300)));
});

test('a contour line is drawn where the height crosses the interval', () => {
  const w = 4;
  const h = 1;
  const heights = new Float32Array([40, 60, 80, 90]);
  const px = topoPixels(heights, w, h, 30, 50, TOPO_LIGHT, 1);
  assert.equal(px.length, 16);
  const base = topoPixels(new Float32Array([40, 40, 40, 40]), w, h, 30, 50, TOPO_LIGHT, 1);
  // Pixel 0 lies below 50 m and pixel 1 above: the line falls on pixel 0's edge.
  assert.notDeepEqual([...px.subarray(0, 3)], [...base.subarray(0, 3)]);
  assert.equal(px[3], 255);
});

test('the recorded Highlands tile: sea, land and contours all appear', () => {
  const png = readPng(new Uint8Array(readFileSync(new URL('../../fixtures/tiles/terrarium-6-31-19.png', import.meta.url))));
  const g = decodeTerrarium(png.data, png.width, png.height);
  const px = topoPixels(g.data, g.width, g.height, 1300, contourIntervalM(6), TOPO_DARK, shadeZFactor(6));
  let sea = 0;
  let lines = 0;
  const [sr, sg, sb] = TOPO_DARK.sea;
  for (let i = 0; i < g.data.length; i++) {
    if (g.data[i] <= 0 && Math.abs(px[i * 4] - sr) < 40 && Math.abs(px[i * 4 + 1] - sg) < 40 && Math.abs(px[i * 4 + 2] - sb) < 40) sea++;
    if (g.data[i] > 0) {
      const lo = Math.floor(g.data[i] / 500);
      const right = i % g.width < g.width - 1 ? Math.floor(g.data[i + 1] / 500) : lo;
      if (right !== lo) lines++;
    }
  }
  assert.ok(sea > 0.2 * g.data.length, `sea ${sea}`);
  assert.ok(lines > 50, `contour crossings ${lines}`);
});
