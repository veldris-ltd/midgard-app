// Unit tests for the offline radar's manifest and time shift (`node --test`, types stripped).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { frameAt, parseManifest, recordedTileUrl } from '../src/app/radarReplay.ts';

const box = { west: -12, east: 4, south: 48, north: 62 };
const want = { zoom: 6, tileSize: 256, box };
const frame = (offset: number, n: number, missing: [number, number][] = []) => ({ offset, path: `frames/${n}`, missing });
const manifest = (frames = [frame(0, 0), frame(600, 1), frame(1200, 2)]) => ({
  version: 1, source: 'RainViewer', attribution: 'Weather data by RainViewer', link: 'https://www.rainviewer.com/',
  captured: 1792767141, firstTime: 1792759800, step: 600, zoom: 6, tileSize: 256, box: { ...box },
  tiles: { x0: 29, x1: 32, y0: 17, y1: 22 }, frames,
});

test('a captured manifest parses', () => {
  const m = parseManifest(manifest(), want);
  assert.equal(m.frames.length, 3);
  assert.equal(m.step, 600);
  assert.equal(m.attribution, 'Weather data by RainViewer');
});

test('a manifest for another box, zoom or shape is refused', () => {
  assert.throws(() => parseManifest({ ...manifest(), zoom: 7 }, want), /zoom/);
  assert.throws(() => parseManifest({ ...manifest(), box: { ...box, west: -11 } }, want), /box/);
  assert.throws(() => parseManifest({ ...manifest(), version: 2 }, want), /version/);
  assert.throws(() => parseManifest(null, want), /version/);
  assert.throws(() => parseManifest(manifest([]), want), /no frames/);
  assert.throws(() => parseManifest(manifest([frame(600, 0), frame(0, 1)]), want), /increasing/);
  assert.throws(() => parseManifest(manifest([{ offset: 0, path: '../../etc', missing: [] }]), want), /path/);
  assert.throws(() => parseManifest(manifest([{ offset: 0, path: 'frames/0', missing: [[1]] as never }]), want), /missing/);
  assert.throws(() => parseManifest({ ...manifest(), step: 0 }, want), /step/);
});

test('the newest recorded frame is now, then the frames loop forward', () => {
  const m = parseManifest(manifest(), want);
  const A = 2_000_000_000;
  // At the anchor: the newest frame, labelled now.
  assert.deepEqual([frameAt(m, A, A).index, frameAt(m, A, A).time], [2, A]);
  // Within its step: still the newest, with the same label.
  assert.deepEqual([frameAt(m, A, A + 599).index, frameAt(m, A, A + 599).time], [2, A]);
  // One step on: the loop starts again at the oldest, labelled ten minutes later.
  assert.deepEqual([frameAt(m, A, A + 600).index, frameAt(m, A, A + 600).time], [0, A + 600]);
  assert.deepEqual([frameAt(m, A, A + 1250).index, frameAt(m, A, A + 1250).time], [1, A + 1200]);
  assert.deepEqual([frameAt(m, A, A + 1800).index, frameAt(m, A, A + 1800).time], [2, A + 1800]);
  // A whole loop (3 frames x 10 min) later: the newest again, and time keeps rising.
  assert.deepEqual([frameAt(m, A, A + 1800 * 10 + 5).index, frameAt(m, A, A + 1800 * 10 + 5).time], [2, A + 18000]);
  // A clock before the anchor is held at the anchor.
  assert.equal(frameAt(m, A, A - 50).index, 2);
});

test('gaps in the recording hold the earlier frame', () => {
  const m = parseManifest(manifest([frame(0, 0), frame(1800, 3)]), want);
  const A = 1000;
  // Period 2400 s: oldest from +600 to +2400, newest at the anchor and after +2400.
  assert.equal(frameAt(m, A, A + 600).index, 0);
  assert.equal(frameAt(m, A, A + 2399).index, 0);
  assert.equal(frameAt(m, A, A + 2400).index, 1);
});

test('tiles are read from the app under /radar/', () => {
  assert.equal(recordedTileUrl(frame(0, 4), 31, 19), '/radar/frames/4/31/19.png');
});
