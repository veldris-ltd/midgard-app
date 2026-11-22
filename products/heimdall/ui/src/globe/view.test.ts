/* The tilted camera: pitch and heading, their limits, and the moves that keep the
 * point in view. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { MAX_ALTITUDE_KM, MIN_ALTITUDE_KM } from './camera.ts';
import { fromXyz } from './projection.ts';
import {
  MAX_PITCH_DEG,
  interpolateView,
  lookTarget,
  maxPitchDeg,
  normaliseView,
  panView,
  rotateView,
  tiltView,
  viewFlyDurationMs,
  viewFrame,
  viewLookingAt,
  zoomView,
} from './view.ts';

const close = (a: number, b: number, eps: number, what = '') => assert.ok(Math.abs(a - b) <= eps, `${what} ${a} vs ${b}`);
const dot = (a: { x: number; y: number; z: number }, b: { x: number; y: number; z: number }) => a.x * b.x + a.y * b.y + a.z * b.z;
const headingDiff = (a: number, b: number) => Math.abs(((a - b + 540) % 360) - 180);

test('top-down, north up: the camera looks at the centre and north is up the screen', () => {
  const f = viewFrame({ lat: 0, lon: 0, altKm: 1000, pitch: 0, heading: 0 });
  close(f.forward.x, -1, 1e-12); close(f.forward.y, 0, 1e-12); close(f.forward.z, 0, 1e-12);
  close(f.up.y, 1, 1e-12);
  close(dot(f.forward, f.up), 0, 1e-12);
});

test('pitch tips the view towards the heading; up stays square to it', () => {
  const f = viewFrame({ lat: 0, lon: 0, altKm: 1000, pitch: 50, heading: 90 });
  // Heading 90 is east, which is -z in the globe's frame.
  assert.ok(f.forward.z < -0.7, `${f.forward.z}`);
  close(dot(f.forward, f.up), 0, 1e-12);
  close(Math.hypot(f.up.x, f.up.y, f.up.z), 1, 1e-12);
  const t = lookTarget({ lat: 0, lon: 0, altKm: 1000, pitch: 50, heading: 90 })!;
  close(t.lat, 0, 1e-9, 'lat');
  assert.ok(t.lon > 0, 'the target is east');
});

test('the tilt limit: full tilt close in, less far out, never past the horizon', () => {
  assert.equal(maxPitchDeg(MIN_ALTITUDE_KM), MAX_PITCH_DEG);
  assert.equal(MAX_PITCH_DEG, 75);
  let prev = Infinity;
  for (let alt = MIN_ALTITUDE_KM; alt <= MAX_ALTITUDE_KM; alt *= 1.2) {
    const m = maxPitchDeg(alt);
    assert.ok(m <= prev + 1e-9 && m >= 0, `${alt}: ${m}`);
    prev = m;
    assert.ok(lookTarget({ lat: 50, lon: 0, altKm: alt, pitch: m, heading: 30 }), `${alt}: looking past the horizon`);
  }
  assert.ok(maxPitchDeg(MAX_ALTITUDE_KM) < 5);
});

test('normaliseView clamps pitch to the limit for the altitude and wraps heading', () => {
  const v = normaliseView({ lat: 0, lon: 0, altKm: 20000, pitch: 70, heading: -90 });
  assert.equal(v.pitch, maxPitchDeg(20000));
  assert.equal(v.heading, 270);
  assert.equal(normaliseView({ lat: 0, lon: 0, altKm: 100, pitch: -5, heading: 720 }).pitch, 0);
  assert.equal(normaliseView({ lat: 0, lon: 0, altKm: 100, pitch: Number.NaN, heading: Number.NaN }).heading, 0);
});

test('viewLookingAt puts the target in the middle of the view at that pitch and heading', () => {
  for (const [lat, lon, alt, pitch, heading] of [[57, -2, 600, 50, 0], [50.1, -2, 350, 55, 80], [-33, 151, 2000, 30, 225], [54, 179.5, 900, 50, 90]]) {
    const v = viewLookingAt({ lat, lon }, alt, pitch, heading);
    const t = lookTarget(v)!;
    close(t.lat, lat, 1e-6, 'lat');
    close(headingDiff(t.lon, lon), 0, 1e-6, 'lon');
    close(v.pitch, pitch, 1e-9, 'pitch');
    close(headingDiff(v.heading, heading), 0, 1e-6, 'heading');
    close(v.altKm, alt, 1e-9, 'alt');
  }
});

test('tilt and rotate orbit the point in view; the altitude, and so the floor, is kept', () => {
  const v = viewLookingAt({ lat: 57, lon: -2 }, 600, 40, 10);
  const tilted = tiltView(v, 15);
  close(tilted.pitch, 55, 1e-9);
  close(tilted.altKm, 600, 1e-9);
  close(lookTarget(tilted)!.lat, 57, 1e-6);
  close(lookTarget(tilted)!.lon, -2, 1e-6);
  assert.equal(tiltView(v, 90).pitch, maxPitchDeg(600));
  assert.equal(tiltView(v, -90).pitch, 0);
  const turned = rotateView(v, 100);
  close(headingDiff(turned.heading, 110), 0, 1e-6);
  close(turned.pitch, 40, 1e-9);
  close(lookTarget(turned)!.lat, 57, 1e-6);
  close(lookTarget(turned)!.lon, -2, 1e-6);
});

test('zoom keeps the point in view when tilted, and lowers the tilt limit when it climbs', () => {
  const v = viewLookingAt({ lat: 57, lon: -2 }, 600, 50, 0);
  const z = zoomView(v, 0.5);
  close(z.altKm, 300, 1e-9);
  close(lookTarget(z)!.lat, 57, 1e-6);
  close(lookTarget(z)!.lon, -2, 1e-6);
  const far = zoomView(v, 40);
  assert.ok(far.pitch <= maxPitchDeg(far.altKm));
  assert.equal(zoomView({ lat: 1, lon: 2, altKm: 1000, pitch: 0, heading: 0 }, 0.5).lat, 1);
});

test('drag follows the screen: with the view turned east, dragging down moves the camera east', () => {
  const v = { lat: 50, lon: 0, altKm: 1000, pitch: 30, heading: 90 };
  const p = panView(v, 0, 100, 800);
  assert.ok(p.lon > 0.5 && Math.abs(p.lat - 50) < 0.2, `${p.lat}, ${p.lon}`);
  assert.equal(p.heading, 90);
  assert.equal(p.pitch, 30);
  const north = panView({ ...v, heading: 0 }, 0, 100, 800);
  assert.ok(north.lat > 50);
});

test('a flight lands exactly, turns the short way, and keeps the tilt inside the limit', () => {
  const a = { lat: 54, lon: -4, altKm: 900, pitch: 50, heading: 350 };
  const b = viewLookingAt({ lat: -30, lon: 140 }, 600, 55, 20);
  assert.deepEqual(interpolateView(a, b, 0), a);
  assert.deepEqual(interpolateView(a, b, 1), b);
  for (let t = 0.05; t < 1; t += 0.05) {
    const m = interpolateView(a, b, t);
    assert.ok(m.pitch <= maxPitchDeg(m.altKm) + 1e-9, `${t}`);
    assert.ok(m.altKm >= MIN_ALTITUDE_KM && m.altKm <= MAX_ALTITUDE_KM);
  }
  const turn = interpolateView({ ...a, heading: 350 }, { ...a, heading: 10 }, 0.5);
  assert.ok(turn.heading > 355 || turn.heading < 5, `${turn.heading}`);
});

test('a change of tilt or heading alone still animates, and not under reduced motion', () => {
  const a = { lat: 54, lon: -4, altKm: 900, pitch: 0, heading: 0 };
  assert.ok(viewFlyDurationMs(a, { ...a, pitch: 45 }, false) >= 600);
  assert.equal(viewFlyDurationMs(a, { ...a, pitch: 45 }, true), 0);
  assert.equal(viewFlyDurationMs(a, a, false), 0);
});

test('the camera position is the point under it: lat/lon/alt are the camera, not the target', () => {
  const v = viewLookingAt({ lat: 57, lon: -2 }, 600, 50, 0);
  const pos = fromXyz(viewFrame(v).position);
  close(pos.lat, v.lat, 1e-9); close(pos.lon, v.lon, 1e-9); close(pos.altKm, 600, 1e-6);
  assert.ok(v.lat < 57, 'looking north, the camera stands south of the point');
});

test('tilted to the limit, the point in view and the horizon stay inside the far plane, and markers outside the near one', async () => {
  const { clipPlanes, cameraFloorKm } = await import('./camera.ts');
  const { EARTH_RADIUS_KM, toXyz } = await import('./projection.ts');
  const markerKm = 13.716 * 20;
  for (let alt = cameraFloorKm(markerKm); alt <= MAX_ALTITUDE_KM; alt *= 1.3) {
    const v = { lat: 56, lon: -3, altKm: alt, pitch: maxPitchDeg(alt), heading: 40 };
    const { near, far } = clipPlanes(alt);
    const { position: o } = viewFrame(v);
    const t = lookTarget(v)!;
    const p = toXyz(t.lat, t.lon, markerKm);
    const d = Math.hypot(p.x - o.x, p.y - o.y, p.z - o.z);
    assert.ok(d < far && d > near, `${alt}: ${d} outside ${near}..${far}`);
    const horizon = Math.sqrt((1 + alt / EARTH_RADIUS_KM) ** 2 - 1);
    assert.ok(horizon < far);
  }
});
