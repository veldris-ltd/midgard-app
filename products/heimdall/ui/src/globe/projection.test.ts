/* Projection between latitude, longitude and altitude and the globe's xyz frame.
 * Run with `pnpm --filter @midgard/console-heimdall test`. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { EARTH_RADIUS_KM, fromXyz, toXyz } from './projection.ts';

const close = (a: number, b: number, eps = 1e-9) => assert.ok(Math.abs(a - b) <= eps, `${a} != ${b}`);

test('the equator and prime meridian sit on +x, the north pole on +y', () => {
  const p = toXyz(0, 0);
  close(p.x, 1); close(p.y, 0); close(p.z, 0);
  const n = toXyz(90, 0);
  close(n.x, 0); close(n.y, 1); close(n.z, 0);
});

test('east is -z, so it is on the right when looking at the globe from +x', () => {
  const e = toXyz(0, 90);
  close(e.x, 0); close(e.y, 0); close(e.z, -1);
  const w = toXyz(0, -90);
  close(w.z, 1);
});

test('altitude raises the radius in proportion to the Earth radius', () => {
  const p = toXyz(0, 0, EARTH_RADIUS_KM);
  close(p.x, 2);
  const q = toXyz(10, 20, 100, 3);
  close(Math.hypot(q.x, q.y, q.z), 3 * (1 + 100 / EARTH_RADIUS_KM));
});

test('fromXyz inverts toXyz', () => {
  for (const [lat, lon, alt] of [[51.5, -0.12, 0], [57.55, 1.35, 6.46], [-33.9, 151.2, 400], [0, 180, 12], [-89, -179.5, 35000]]) {
    const back = fromXyz(toXyz(lat, lon, alt));
    close(back.lat, lat, 1e-9);
    close(Math.abs(back.lon) === 180 ? Math.abs(back.lon) : back.lon, Math.abs(lon) === 180 ? 180 : lon, 1e-9);
    close(back.altKm, alt, 1e-6);
  }
});

test('fromXyz returns longitudes in (-180, 180]', () => {
  const back = fromXyz(toXyz(0, 180));
  close(back.lon, 180);
  const b2 = fromXyz(toXyz(0, 190));
  close(b2.lon, -170, 1e-9);
});
