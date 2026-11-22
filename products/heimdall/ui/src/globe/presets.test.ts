/* Camera presets, the REGION list and the screen-reader camera summary. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { MAX_ALTITUDE_KM, MIN_ALTITUDE_KM } from './camera.ts';
import { PRESETS, buildRegions, describeCamera, regionAt, sectorNorth } from './presets.ts';
import { lookTarget, maxPitchDeg } from './view.ts';

const TRACKS = [
  { lat: 57.35, lon: -4.2 },
  { lat: 58.25, lon: -2.3 },
  { lat: 56.05, lon: 0.45 },
  { lat: 57.55, lon: 1.35 },
  { lat: 55.75, lon: -3.05 },
  { lat: undefined, lon: undefined },
];

test('the fixed presets are WORLD, UK & IRELAND, NORTH SEA and CHANNEL', () => {
  assert.deepEqual(PRESETS.map((p) => p.label), ['WORLD', 'UK & IRELAND', 'NORTH SEA', 'CHANNEL']);
  for (const p of PRESETS) {
    assert.ok(p.camera.altKm >= MIN_ALTITUDE_KM && p.camera.altKm <= MAX_ALTITUDE_KM, p.label);
  }
});

test('WORLD is top-down; the regions open tilted 45–60°, within the limit, looking into their bounds', () => {
  const regions = buildRegions(TRACKS);
  for (const r of regions) {
    assert.ok(r.camera.pitch <= maxPitchDeg(r.camera.altKm) + 1e-9, r.label);
    if (r.id === 'world') {
      assert.equal(r.camera.pitch, 0);
      continue;
    }
    assert.ok(r.camera.pitch >= 45 && r.camera.pitch <= 60, `${r.label}: ${r.camera.pitch}`);
    const t = lookTarget(r.camera)!;
    const b = r.bounds!;
    assert.ok(t.lat > b.south && t.lat < b.north && t.lon > b.west && t.lon < b.east, `${r.label}: ${t.lat}, ${t.lon}`);
  }
});

test('SECTOR NORTH is fitted to the positioned tracks', () => {
  const s = sectorNorth(TRACKS);
  assert.equal(s.label, 'SECTOR NORTH');
  const t = lookTarget(s.camera)!;
  assert.ok(t.lat > 55.75 && t.lat < 58.25);
  assert.ok(t.lon > -4.2 && t.lon < 1.35);
  assert.ok(s.camera.altKm >= MIN_ALTITUDE_KM && s.camera.altKm < 2000, `${s.camera.altKm}`);
  assert.ok(s.bounds!.south <= 55.75 && s.bounds!.north >= 58.25);
  assert.ok(s.bounds!.west <= -4.2 && s.bounds!.east >= 1.35);
});

test('SECTOR NORTH falls back to a fixed view with no positioned tracks', () => {
  const s = sectorNorth([]);
  assert.equal(s.label, 'SECTOR NORTH');
  assert.ok(lookTarget(s.camera)!.lat > 50);
});

test('buildRegions lists the presets then SECTOR NORTH', () => {
  assert.deepEqual(buildRegions(TRACKS).map((r) => r.label), ['WORLD', 'UK & IRELAND', 'NORTH SEA', 'CHANNEL', 'SECTOR NORTH']);
  const ids = buildRegions(TRACKS).map((r) => r.id);
  assert.equal(new Set(ids).size, ids.length);
});

test('regionAt picks the smallest region holding the point', () => {
  const regions = buildRegions(TRACKS);
  assert.equal(regionAt(57.5, 0.5, regions)?.label, 'SECTOR NORTH');
  assert.equal(regionAt(50.2, -1.5, regions)?.label, 'CHANNEL');
  assert.equal(regionAt(53.0, -8.0, regions)?.label, 'UK & IRELAND');
  assert.equal(regionAt(54.0, 4.5, regions)?.label, 'NORTH SEA');
  assert.equal(regionAt(-30, 140, regions), null);
});

test('describeCamera reads well aloud, with the tilt and heading', () => {
  const regions = buildRegions(TRACKS);
  assert.equal(
    describeCamera({ lat: 50.25, lon: -1.5, altKm: 600, pitch: 0, heading: 0 }, regions),
    'Camera 600 km above 50.25° N, 1.50° W, looking straight down, heading 000°, over CHANNEL.',
  );
  assert.equal(
    describeCamera({ lat: -30, lon: 140, altKm: 20000, pitch: 0, heading: 0 }, regions),
    'Camera 20,000 km above 30.00° S, 140.00° E, looking straight down, heading 000°, over open globe.',
  );
  // Close in, the altitude keeps a decimal.
  assert.equal(
    describeCamera({ lat: 56.8, lon: -5, altKm: 1.54, pitch: 0, heading: 0 }, regions),
    'Camera 1.5 km above 56.80° N, 5.00° W, looking straight down, heading 000°, over SECTOR NORTH.',
  );
  const channel = regions.find((r) => r.id === 'channel')!.camera;
  assert.match(describeCamera(channel, regions), /^Camera 350 km above .*, tilted 55°, heading 075°, looking towards CHANNEL\.$/);
});
