/* Camera limits, pan, zoom and the eased fly-to. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  CLOSE_UP_ALTITUDE_KM,
  MAX_ALTITUDE_KM,
  MIN_ALTITUDE_KM,
  clampAltitude,
  easeInOutCubic,
  flyDurationMs,
  interpolateCamera,
  normaliseCamera,
  panCamera,
  zoomCamera,
} from './camera.ts';

test('altitude stays between the 1.5 km close-up and 40,000 km; 50 km is the floor without tiles', () => {
  assert.equal(MIN_ALTITUDE_KM, 50);
  assert.equal(CLOSE_UP_ALTITUDE_KM, 1.5);
  assert.equal(MAX_ALTITUDE_KM, 40000);
  assert.equal(clampAltitude(1), 1.5);
  assert.equal(clampAltitude(1e9), 40000);
  assert.equal(clampAltitude(1234), 1234);
  assert.equal(clampAltitude(Number.NaN), MAX_ALTITUDE_KM);
});

test('normaliseCamera wraps longitude and keeps clear of the poles', () => {
  const c = normaliseCamera({ lat: 95, lon: 190, altKm: 1 });
  assert.equal(c.lat, 89);
  assert.equal(c.lon, -170);
  assert.equal(c.altKm, 1.5);
  assert.equal(normaliseCamera({ lat: -100, lon: -540, altKm: 100 }).lat, -89);
  assert.equal(normaliseCamera({ lat: 0, lon: -540, altKm: 100 }).lon, 180);
});

test('zoom multiplies altitude within the limits', () => {
  const c = { lat: 0, lon: 0, altKm: 1000 };
  assert.equal(zoomCamera(c, 0.5).altKm, 500);
  assert.equal(zoomCamera(c, 1e-6).altKm, 1.5);
  assert.equal(zoomCamera(c, 1e6).altKm, 40000);
});

test('dragging right moves the view west, dragging down moves it north', () => {
  const c = { lat: 50, lon: 0, altKm: 1000 };
  const right = panCamera(c, 100, 0, 800, 45);
  assert.ok(right.lon < 0, 'drag right: camera goes west');
  assert.equal(right.lat, 50);
  const down = panCamera(c, 0, 100, 800, 45);
  assert.ok(down.lat > 50, 'drag down: camera goes north');
});

test('pan speed scales with altitude', () => {
  const low = panCamera({ lat: 0, lon: 0, altKm: 100 }, 0, 100, 800, 45);
  const high = panCamera({ lat: 0, lon: 0, altKm: 10000 }, 0, 100, 800, 45);
  assert.ok(high.lat > low.lat * 10);
});

test('easeInOutCubic is 0 → 1, symmetric and monotonic', () => {
  assert.equal(easeInOutCubic(0), 0);
  assert.equal(easeInOutCubic(1), 1);
  assert.equal(easeInOutCubic(0.5), 0.5);
  assert.equal(easeInOutCubic(-1), 0);
  assert.equal(easeInOutCubic(2), 1);
  let prev = 0;
  for (let i = 1; i <= 100; i++) {
    const v = easeInOutCubic(i / 100);
    assert.ok(v >= prev);
    prev = v;
  }
});

test('interpolateCamera ends exactly on the target and crosses the antimeridian the short way', () => {
  const a = { lat: 10, lon: 170, altKm: 1000 };
  const b = { lat: 20, lon: -170, altKm: 2000 };
  assert.deepEqual(interpolateCamera(a, b, 0), a);
  assert.deepEqual(interpolateCamera(a, b, 1), b);
  const mid = interpolateCamera(a, b, 0.5);
  assert.ok(Math.abs(mid.lon) > 170, `went the long way: ${mid.lon}`);
  assert.ok(mid.altKm >= 1000 && mid.altKm <= MAX_ALTITUDE_KM);
});

test('a long flight lifts the camera mid-way, within the limits', () => {
  const a = { lat: 54, lon: -4, altKm: 500 };
  const b = { lat: -30, lon: 140, altKm: 500 };
  const mid = interpolateCamera(a, b, 0.5);
  assert.ok(mid.altKm > 5000, `${mid.altKm}`);
  assert.ok(mid.altKm <= MAX_ALTITUDE_KM);
});

test('fly-to takes no time when reduced motion is asked for', () => {
  const a = { lat: 54, lon: -4, altKm: 500 };
  const b = { lat: 20, lon: 0, altKm: 20000 };
  assert.equal(flyDurationMs(a, b, true), 0);
  const d = flyDurationMs(a, b, false);
  assert.ok(d >= 600 && d <= 2400, `${d}`);
  assert.equal(flyDurationMs(a, a, false), 0);
});

test('every marker stays drawable: at the highest HEIGHTS, FL450, from every camera altitude allowed', async () => {
  const { cameraFloorKm, clipPlanes } = await import('./camera.ts');
  const { MAX_EXAGGERATION } = await import('./items.ts');
  const { EARTH_RADIUS_KM } = await import('./projection.ts');
  const { FEET_TO_METRES } = await import('./altitude.ts');
  const R = EARTH_RADIUS_KM;
  const fl450Km = (45000 * FEET_TO_METRES) / 1000;
  for (let k = 1; k <= MAX_EXAGGERATION; k++) {
    const markerKm = fl450Km * k;
    const floor = cameraFloorKm(markerKm);
    assert.ok(floor >= MIN_ALTITUDE_KM && floor > markerKm, `x${k}: floor ${floor}`);
    for (let alt = floor; alt <= MAX_ALTITUDE_KM; alt *= 1.05) {
      const { near, far } = clipPlanes(alt);
      // Straight below the camera: the closest a marker can be.
      const nearest = (alt - markerKm) / R;
      assert.ok(nearest > near, `x${k} at ${alt.toFixed(1)} km: marker ${nearest} inside near ${near}`);
      // Over the camera's horizon and the marker's own: the farthest it can still be seen.
      const horizon = (h: number) => Math.sqrt((1 + h / R) ** 2 - 1);
      assert.ok(horizon(alt) + horizon(markerKm) < far, `x${k} at ${alt.toFixed(1)} km: beyond far`);
    }
  }
});

test('the floor is the 50 km limit until markers stand higher', async () => {
  const { cameraFloorKm } = await import('./camera.ts');
  assert.equal(cameraFloorKm(0), MIN_ALTITUDE_KM);
  assert.equal(cameraFloorKm(10), MIN_ALTITUDE_KM);
  assert.ok(cameraFloorKm(274) > 274);
  assert.ok(cameraFloorKm(1e9) <= MAX_ALTITUDE_KM);
  assert.equal(cameraFloorKm(Number.NaN), MIN_ALTITUDE_KM);
});

test('close in over terrain: 1.5 km at the least, and always 150 m above the ground below', async () => {
  const { terrainFloorKm, TERRAIN_CLEARANCE_KM } = await import('./camera.ts');
  assert.equal(TERRAIN_CLEARANCE_KM, 0.15);
  assert.equal(terrainFloorKm(0), 1.5);
  assert.equal(terrainFloorKm(1.2), 1.5);
  // Ben Nevis at ×4: 5.38 km of rendered ground.
  assert.ok(Math.abs(terrainFloorKm(5.38) - 5.53) < 1e-9);
  assert.equal(terrainFloorKm(Number.NaN), 1.5);
  assert.equal(terrainFloorKm(-3), 1.5);
});

test('the near plane stays inside the 150 m clearance at every close-up altitude', async () => {
  const { clipPlanes, terrainFloorKm } = await import('./camera.ts');
  const { EARTH_RADIUS_KM } = await import('./projection.ts');
  // Up to Everest at the highest HEIGHTS, ×20.
  for (const ground of [0, 0.5, 1.3, 5.4, 26.9, 177]) {
    const alt = terrainFloorKm(ground);
    const { near } = clipPlanes(alt, alt - ground);
    assert.ok(near * EARTH_RADIUS_KM < alt - ground, `ground ${ground}: near ${near * EARTH_RADIUS_KM} km`);
  }
});
