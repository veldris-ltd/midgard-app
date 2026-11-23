/* Terrarium decoding, on synthetic pixels and on a recorded tile. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { readPng } from '../../test-support/png.ts';
import { type HeightGrid, decodeTerrarium, heightAt, heightRange, resampleGrid } from './terrarium.ts';

const close = (a: number, b: number, eps = 1e-6) => assert.ok(Math.abs(a - b) <= eps, `${a} != ${b}`);
const fixture = (name: string) => new Uint8Array(readFileSync(new URL(`../../fixtures/tiles/${name}`, import.meta.url)));

function pixels(values: [number, number, number][]): Uint8ClampedArray {
  const out = new Uint8ClampedArray(values.length * 4);
  values.forEach(([r, g, b], i) => out.set([r, g, b, 255], i * 4));
  return out;
}

test('height = R × 256 + G + B / 256 − 32768', () => {
  const g = decodeTerrarium(pixels([[128, 0, 0], [128, 100, 128], [127, 255, 0], [0, 0, 0]]), 2, 2);
  assert.equal(g.width, 2);
  assert.equal(g.height, 2);
  close(g.data[0], 0);
  close(g.data[1], 100.5);
  close(g.data[2], -1);
  close(g.data[3], -32768);
});

test('a short pixel buffer is refused', () => {
  assert.throws(() => decodeTerrarium(new Uint8ClampedArray(12), 2, 2), /pixels/);
});

const grid: HeightGrid = { width: 2, height: 2, data: new Float32Array([0, 100, 200, 300]) };

test('heightAt samples bilinearly between pixel centres and clamps at the edges', () => {
  close(heightAt(grid, 0.25, 0.25), 0);
  close(heightAt(grid, 0.75, 0.25), 100);
  close(heightAt(grid, 0.5, 0.5), 150);
  close(heightAt(grid, 0, 0), 0);
  close(heightAt(grid, 1, 1), 300);
  close(heightAt(grid, 0.5, 0.25), 50);
});

test('resampleGrid fills an (n+1)² vertex grid over a part of the tile', () => {
  const whole = resampleGrid(grid, { ox: 0, oy: 0, size: 1 }, 2);
  assert.equal(whole.length, 9);
  close(whole[0], 0);
  close(whole[4], 150);
  close(whole[8], 300);
  const quarter = resampleGrid(grid, { ox: 0.5, oy: 0.5, size: 0.5 }, 1);
  assert.equal(quarter.length, 4);
  close(quarter[0], 150);
  close(quarter[3], 300);
});

test('heightRange reports the lowest and highest heights', () => {
  assert.deepEqual(heightRange(grid), { min: 0, max: 300 });
});

test('the recorded tile over the Highlands decodes to plausible heights', () => {
  const png = readPng(fixture('terrarium-6-31-19.png'));
  assert.equal(png.width, 256);
  assert.equal(png.height, 256);
  const g = decodeTerrarium(png.data, png.width, png.height);
  const r = heightRange(g);
  // The tile runs from the Atlantic floor to the Cairngorms and Ben Nevis.
  assert.ok(r.min < -50 && r.min > -3000, `min ${r.min}`);
  assert.ok(r.max > 900 && r.max < 1400, `max ${r.max}`);
  // The North Sea off Caithness, at sea; the Cairngorm plateau and Ben Nevis, high.
  assert.ok(heightAt(g, 0.822, 0.175) < 0);
  assert.ok(heightAt(g, 0.348, 0.584) > 700);
  assert.ok(heightAt(g, 0.11, 0.673) > 700);
});
