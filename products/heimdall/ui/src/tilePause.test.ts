/* Map tiles pause after track-driven camera moves in the public build only, and resume
 * on the user's own moves. [FR-H-1] [NFR-PRV-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { CLOSE_UP_PRESETS, PRESETS, buildRegions, sectorNorth } from './globe/presets.ts';
import { pausedAfter, regionMove } from './tilePause.ts';

const TRACKS = [{ lat: 57.1, lon: -2.1 }, { lat: 58.4, lon: 0.6 }];

test('public: a track move pauses, a user move resumes', () => {
  assert.equal(pausedAfter('track', true), true);
  assert.equal(pausedAfter('user', true), false);
});

test('mirror: nothing pauses', () => {
  assert.equal(pausedAfter('track', false), false);
  assert.equal(pausedAfter('user', false), false);
});

test('SECTOR NORTH fitted to the tracks is a track move; the fixed fallback, without tracks, is not', () => {
  assert.equal(regionMove(sectorNorth(TRACKS)), 'track');
  assert.equal(regionMove(sectorNorth([])), 'user');
});

test('the geographic presets, close-ups included, are user moves', () => {
  for (const r of [...PRESETS, ...CLOSE_UP_PRESETS]) assert.equal(regionMove(r), 'user', r.id);
  const regions = buildRegions(TRACKS, true);
  assert.deepEqual(regions.filter((r) => regionMove(r) === 'track').map((r) => r.id), ['sector-north']);
});

test('the sequence the console follows in public mode: first view, drag, fly to a track, a preset', () => {
  const steps: [string, 'track' | 'user', boolean][] = [
    ['first view on SECTOR NORTH', regionMove(sectorNorth(TRACKS)), true],
    ['a drag', 'user', false],
    ['fly to a track', 'track', true],
    ['zoom', 'user', false],
    ['SECTOR NORTH again', regionMove(sectorNorth(TRACKS)), true],
    ['SNOWDONIA', regionMove(CLOSE_UP_PRESETS[0]), false],
  ];
  for (const [what, move, paused] of steps) assert.equal(pausedAfter(move, true), paused, what);
});
