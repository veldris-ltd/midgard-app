// Unit tests for the offline traffic's manifest and tile choice (`node --test`, types stripped).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseTrafficManifest, recordedTiles, recordedTrafficUrl } from '../src/app/trafficReplay.ts';

const box = { west: -12, east: 4, south: 48, north: 62 };
const want = { overviewZoom: 7, box };
const level = (z: number, r: [number, number, number, number], tiles: [number, number][], missing: [number, number][] = []) =>
  ({ z, x0: r[0], x1: r[1], y0: r[2], y1: r[3], tiles, missing });
const manifest = (levels = [
  level(7, [59, 65, 35, 44], [[63, 42], [62, 41], [59, 35]]),
  level(9, [238, 261, 142, 177], [[255, 170], [255, 171], [240, 150]], [[254, 170]]),
  level(11, [955, 1046, 571, 711], [[1023, 681], [1022, 681]]),
]) => ({
  version: 1, source: 'TomTom Traffic Flow (relative), one snapshot', attribution: '©TomTom',
  link: 'https://www.tomtom.com/', captured: 1792767141, overviewZoom: 7, box: { ...box }, levels,
});

test('a captured manifest parses', () => {
  const m = parseTrafficManifest(manifest(), want);
  assert.equal(m.levels.length, 3);
  assert.equal(m.attribution, '©TomTom');
  assert.equal(m.captured, 1792767141);
});

test('a manifest for another box, overview zoom or shape is refused', () => {
  assert.throws(() => parseTrafficManifest({ ...manifest(), version: 2 }, want), /version/);
  assert.throws(() => parseTrafficManifest({ ...manifest(), overviewZoom: 6 }, want), /overview zoom/);
  assert.throws(() => parseTrafficManifest({ ...manifest(), box: { ...box, west: -11 } }, want), /box/);
  assert.throws(() => parseTrafficManifest(manifest([]), want), /no levels/);
  assert.throws(() => parseTrafficManifest(manifest([level(9, [1, 2, 1, 2], []), level(7, [1, 2, 1, 2], [])]), want), /increasing/);
  assert.throws(() => parseTrafficManifest(manifest([level(9, [1, 2, 1, 2], [])]), want), /overview level/);
  assert.throws(() => parseTrafficManifest(manifest([level(7, [2, 1, 1, 2], [])]), want), /range/);
  assert.throws(() => parseTrafficManifest(manifest([level(7, [1, 2, 1, 2], [[1, 'x' as unknown as number]])]), want), /tile list/);
  assert.throws(() => parseTrafficManifest(null, want), /version/);
  // An over-long or missing caption falls back to TomTom's.
  assert.equal(parseTrafficManifest({ ...manifest(), attribution: 'x'.repeat(500) }, want).attribution, '©TomTom');
});

test('the overview reads the overview level, every stored tile in the box', () => {
  const m = parseTrafficManifest(manifest(), want);
  const pick = recordedTiles(m, box, 7, 80);
  assert.equal(pick.z, 7);
  assert.equal(pick.tiles.length, 3);
});

test('close in: the finest level at or below the zoom wanted, only tiles in the box', () => {
  const m = parseTrafficManifest(manifest(), want);
  // Central London, about 0.5 degrees across: zoom 12 wanted, zoom 11 recorded.
  const london = { west: -0.4, east: 0.15, south: 51.35, north: 51.65 };
  const pick = recordedTiles(m, london, 12, 80);
  assert.equal(pick.z, 11);
  assert.deepEqual(pick.tiles, [[1023, 681], [1022, 681]]);
  // Zoom 10 wanted: zoom 9 (no zoom 10 recorded); the failed tile is not read.
  assert.deepEqual(recordedTiles(m, london, 10, 80), { z: 9, tiles: [[255, 170]] });
  // Too many zoom-9 tiles for a wide box: back to the overview.
  assert.equal(recordedTiles(m, { west: -8, east: 2, south: 50, north: 56 }, 9, 80).z, 7);
});

test('outside the recording: no tiles', () => {
  const m = parseTrafficManifest(manifest(), want);
  const auckland = { west: 174.6, east: 175, south: -37, north: -36.7 };
  assert.deepEqual(recordedTiles(m, auckland, 12, 80).tiles, []);
});

test('tile URLs stay on the app', () => {
  assert.equal(recordedTrafficUrl(11, 1023, 681), '/traffic/11/1023/681.pbf');
});
