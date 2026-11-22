/* The subsolar point for the globe's day and night shading, and what the camera can
 * see. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { subsolarPoint } from './sun.ts';
import { isOccluded, toXyz } from './projection.ts';

const near = (a: number, b: number, eps: number, what: string) => assert.ok(Math.abs(a - b) <= eps, `${what}: ${a} vs ${b}`);

test('the sun is overhead near the tropic of Cancer at the June solstice, noon UTC', () => {
  const s = subsolarPoint(new Date(Date.UTC(2026, 5, 21, 12, 0, 0)));
  near(s.lat, 23.44, 0.2, 'lat');
  near(s.lon, 0, 1, 'lon');
});

test('the sun is overhead near the equator at the March equinox, and moves west 15° an hour', () => {
  const a = subsolarPoint(new Date(Date.UTC(2026, 2, 20, 12, 0, 0)));
  near(a.lat, 0, 0.5, 'lat');
  const b = subsolarPoint(new Date(Date.UTC(2026, 2, 20, 18, 0, 0)));
  near(b.lon, a.lon - 90, 0.1, 'six hours later');
});

test('the December solstice puts it on the tropic of Capricorn, longitudes in (-180, 180]', () => {
  const s = subsolarPoint(new Date(Date.UTC(2026, 11, 21, 0, 0, 0)));
  near(s.lat, -23.44, 0.2, 'lat');
  near(Math.abs(s.lon), 180, 1.5, 'midnight UTC: over the antimeridian');
  assert.ok(s.lon > -180 && s.lon <= 180);
});

test('a point on the far side of the globe is hidden; one on the near side is not', () => {
  const cam = toXyz(57, -2, 1000);
  assert.equal(isOccluded(cam, toXyz(57.5, 1.3, 10)), false);
  assert.equal(isOccluded(cam, toXyz(-57, 178, 10)), true);
  assert.equal(isOccluded(cam, toXyz(57, -2, 0)), false);
});

test('an aircraft just over the horizon is still seen above it', () => {
  const cam = toXyz(0, 0, 500);
  // About 22° from the camera's nadir, past the 500 km horizon (~21.9°), but 400 km up.
  assert.equal(isOccluded(cam, toXyz(0, 24, 400)), false);
  assert.equal(isOccluded(cam, toXyz(0, 24, 0)), true);
});
