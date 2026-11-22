/* Tracks become globe items: placed by lat/lon at their exaggerated altitude, coloured
 * as in 2D, and filtered by the shared layer switches. [FR-H-1] [AC-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import type { HeimdallTrack } from '@midgard/sdk';
import { FEET_TO_METRES } from './altitude.ts';
import { DEFAULT_EXAGGERATION, MAX_EXAGGERATION, MIN_EXAGGERATION, tracksToGlobeItems } from './items.ts';
import { EARTH_RADIUS_KM, fromXyz } from './projection.ts';

const base = { conf: 0.9, x: 50, y: 50, track: 'stable', hdg: 0, spd: 0, feeds: 1 } as const;
const TRACKS: HeimdallTrack[] = [
  { ...base, id: 'A', cls: 'AIRCRAFT', layer: 'air', alt: 'FL340', lat: 57.35, lon: -4.2 },
  { ...base, id: 'V', cls: 'VESSEL', layer: 'maritime', alt: 'SURFACE', lat: 56.05, lon: 0.45 },
  { ...base, id: 'U', cls: 'UNKNOWN', layer: 'air', alt: 'FL212 ↓', lat: 57.55, lon: 1.35, track: 'anomalous' },
  { ...base, id: 'G', cls: 'GROUND UNIT', layer: 'ground', alt: 'GND', lat: 55.75, lon: -3.05 },
  { ...base, id: 'N', cls: 'AIRCRAFT', layer: 'air', alt: 'FL100' },
  { ...base, id: 'Q', cls: 'AIRCRAFT', layer: 'air', alt: '???', lat: 50, lon: 0 },
];
const allOn = () => true;

test('the HEIGHTS setting defaults to ×4', () => {
  assert.equal(DEFAULT_EXAGGERATION, 4);
  assert.ok(MIN_EXAGGERATION >= 1 && MAX_EXAGGERATION > DEFAULT_EXAGGERATION);
});

test('tracks without lat/lon are left out; the rest keep their order', () => {
  assert.deepEqual(tracksToGlobeItems(TRACKS, allOn, 4).map((i) => i.id), ['A', 'V', 'U', 'G', 'Q']);
});

test('a switched-off layer hides its tracks', () => {
  const ids = tracksToGlobeItems(TRACKS, (l) => l !== 'air', 4).map((i) => i.id);
  assert.deepEqual(ids, ['V', 'G']);
});

test('items sit at lat/lon and their altitude times the HEIGHTS setting', () => {
  const [a] = tracksToGlobeItems(TRACKS, allOn, 4);
  const p = fromXyz(a.position);
  assert.ok(Math.abs(p.lat - 57.35) < 1e-9 && Math.abs(p.lon + 4.2) < 1e-9);
  const km = (34000 * FEET_TO_METRES) / 1000;
  assert.ok(Math.abs(p.altKm - km * 4) < 1e-6, `${p.altKm}`);
  assert.equal(a.altitudeKm, km);
  const g = fromXyz(a.ground);
  assert.ok(Math.abs(g.altKm) < 1e-9);
  const x1 = fromXyz(tracksToGlobeItems(TRACKS, allOn, 1)[0].position);
  assert.ok(Math.abs(x1.altKm - km) < 1e-6);
});

test('the HEIGHTS setting is clamped to its range', () => {
  const lo = fromXyz(tracksToGlobeItems(TRACKS, allOn, 0)[0].position).altKm;
  const hi = fromXyz(tracksToGlobeItems(TRACKS, allOn, 1e6)[0].position).altKm;
  const km = (34000 * FEET_TO_METRES) / 1000;
  assert.ok(Math.abs(lo - km * MIN_EXAGGERATION) < 1e-6);
  assert.ok(Math.abs(hi - km * MAX_EXAGGERATION) < 1e-6);
  assert.ok(hi < EARTH_RADIUS_KM);
});

test('surface tracks sit on the surface; an unreadable altitude is drawn there and flagged', () => {
  const items = tracksToGlobeItems(TRACKS, allOn, 4);
  const v = items.find((i) => i.id === 'V')!;
  assert.ok(Math.abs(fromXyz(v.position).altKm) < 1e-9);
  assert.equal(v.altitudeKnown, true);
  const q = items.find((i) => i.id === 'Q')!;
  assert.equal(q.altitudeKnown, false);
  assert.ok(Math.abs(fromXyz(q.position).altKm) < 1e-9);
});

test('anomalous tracks are --warn, the rest --accent, as in 2D', () => {
  const items = tracksToGlobeItems(TRACKS, allOn, 4);
  assert.equal(items.find((i) => i.id === 'U')!.tone, 'warn');
  assert.equal(items.find((i) => i.id === 'A')!.tone, 'accent');
  assert.equal(items.find((i) => i.id === 'U')!.trend, 'descending');
});

test('items carry their layer for the marker shape', () => {
  const items = tracksToGlobeItems(TRACKS, allOn, 4);
  assert.deepEqual(items.map((i) => i.layer), ['air', 'maritime', 'air', 'ground', 'air']);
});

test('airborne tracks get a stem from the marker down to the ground; surface tracks none', async () => {
  const items = tracksToGlobeItems(TRACKS, allOn, 4);
  const a = items.find((i) => i.id === 'A')!;
  assert.ok(a.stem);
  const [top, bottom] = a.stem!;
  assert.deepEqual(top, a.position);
  assert.deepEqual(bottom, a.ground);
  const km = (34000 * FEET_TO_METRES) / 1000;
  assert.ok(Math.abs(fromXyz(top).altKm - km * 4) < 1e-6);
  assert.ok(Math.abs(fromXyz(bottom).altKm) < 1e-9);
  const lt = fromXyz(top), lb = fromXyz(bottom);
  assert.ok(Math.abs(lt.lat - lb.lat) < 1e-9 && Math.abs(lt.lon - lb.lon) < 1e-9, 'the stem is vertical');
  assert.equal(items.find((i) => i.id === 'V')!.stem, null);
  assert.equal(items.find((i) => i.id === 'G')!.stem, null);
  // An unreadable altitude is drawn on the surface, so it has no stem either.
  assert.equal(items.find((i) => i.id === 'Q')!.stem, null);
});

test('aircraft and vessels point along their heading; ground units do not', async () => {
  const items = tracksToGlobeItems(
    TRACKS.map((t) => ({ ...t, hdg: 90 })),
    allOn,
    4,
  );
  assert.equal(items.find((i) => i.id === 'A')!.heading, 90);
  assert.equal(items.find((i) => i.id === 'V')!.heading, 90);
  assert.equal(items.find((i) => i.id === 'G')!.heading, null);
  const { headingTangent } = await import('./projection.ts');
  const east = headingTangent(0, 0, 90);
  assert.ok(Math.abs(east.z + 1) < 1e-12 && Math.abs(east.x) < 1e-12 && Math.abs(east.y) < 1e-12);
  const north = headingTangent(0, 0, 0);
  assert.ok(Math.abs(north.y - 1) < 1e-12);
});
