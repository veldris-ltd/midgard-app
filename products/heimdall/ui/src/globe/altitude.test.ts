/* The altitude parser reads the console's `alt` strings. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { FEET_TO_METRES, parseAltitude } from './altitude.ts';

test('flight levels are hundreds of feet', () => {
  assert.deepEqual(parseAltitude('FL340'), { feet: 34000, metres: 34000 * FEET_TO_METRES, trend: 'level' });
  assert.equal(parseAltitude('FL360')?.feet, 36000);
  assert.equal(parseAltitude('fl 095')?.feet, 9500);
});

test('a trend arrow is read, not dropped', () => {
  assert.deepEqual(parseAltitude('FL212 ↓'), { feet: 21200, metres: 21200 * FEET_TO_METRES, trend: 'descending' });
  assert.equal(parseAltitude('FL120↑')?.trend, 'climbing');
});

test('SURFACE, GND and SFC are zero', () => {
  for (const s of ['SURFACE', 'GND', 'sfc', ' gnd ']) {
    assert.deepEqual(parseAltitude(s), { feet: 0, metres: 0, trend: 'level' });
  }
});

test('plain feet and metres', () => {
  assert.equal(parseAltitude('2500 FT')?.feet, 2500);
  assert.equal(parseAltitude('2500ft')?.feet, 2500);
  const m = parseAltitude('800 M');
  assert.equal(m?.metres, 800);
  assert.ok(Math.abs(m!.feet - 800 / FEET_TO_METRES) < 1e-9);
});

test('anything else is unknown, not zero', () => {
  for (const s of ['', '—', 'HIGH', 'FL', 'FL-20', '12 NM', 'FL34O']) {
    assert.equal(parseAltitude(s), null, s);
  }
});
