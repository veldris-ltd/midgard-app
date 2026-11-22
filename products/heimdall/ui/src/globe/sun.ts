/* The subsolar point, for the globe's day and night shading. [FR-H-1]
 *
 * The low-precision solar position from the Astronomical Almanac (good to about 0.01°
 * in declination between 1950 and 2050), with Greenwich mean sidereal time for the
 * longitude. Shading needs nothing finer. */
import { wrapLongitude } from './projection.ts';

const RAD = Math.PI / 180;
const J2000_MS = Date.UTC(2000, 0, 1, 12, 0, 0);

export function subsolarPoint(date: Date): { lat: number; lon: number } {
  const n = (date.getTime() - J2000_MS) / 86400000;
  const L = 280.46 + 0.9856474 * n;
  const g = (357.528 + 0.9856003 * n) * RAD;
  const lambda = (L + 1.915 * Math.sin(g) + 0.02 * Math.sin(2 * g)) * RAD;
  const epsilon = (23.439 - 0.0000004 * n) * RAD;
  const decl = Math.asin(Math.sin(epsilon) * Math.sin(lambda));
  const ra = Math.atan2(Math.cos(epsilon) * Math.sin(lambda), Math.cos(lambda)) / RAD;
  const gmst = 280.46061837 + 360.98564736629 * n;
  return { lat: decl / RAD, lon: wrapLongitude(ra - gmst) };
}
