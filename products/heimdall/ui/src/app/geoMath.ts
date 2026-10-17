/* Small geodesy for the operating picture's tools, shared by the 2D map and the globe:
 * great-circle distance and initial bearing (the MEASURE tool), circles of a given
 * radius about a point (range rings) and the subsolar point (day/night shading).
 * Spherical earth, which is well within what these tools are read to. [FR-H-1] [FR-P3-3] */

const D2R = Math.PI / 180;
const R2D = 180 / Math.PI;
export const EARTH_NM = 3440.065;
export const NM_KM = 1.852;

export interface LatLon {
  lat: number;
  lon: number;
}

/** Great-circle distance in nautical miles. */
export function distanceNm(a: LatLon, b: LatLon): number {
  const p1 = a.lat * D2R, p2 = b.lat * D2R;
  const h = Math.sin((p2 - p1) / 2) ** 2 + Math.cos(p1) * Math.cos(p2) * Math.sin(((b.lon - a.lon) * D2R) / 2) ** 2;
  return 2 * EARTH_NM * Math.asin(Math.min(1, Math.sqrt(h)));
}

/** Initial (forward) bearing from a to b, degrees true 0–360. */
export function bearingDeg(a: LatLon, b: LatLon): number {
  const p1 = a.lat * D2R, p2 = b.lat * D2R, dl = (b.lon - a.lon) * D2R;
  const y = Math.sin(dl) * Math.cos(p2);
  const x = Math.cos(p1) * Math.sin(p2) - Math.sin(p1) * Math.cos(p2) * Math.cos(dl);
  return (Math.atan2(y, x) * R2D + 360) % 360;
}

/** The point `nm` from a along a bearing. */
export function destination(a: LatLon, brgDeg: number, nm: number): LatLon {
  const d = nm / EARTH_NM, brg = brgDeg * D2R, p1 = a.lat * D2R, l1 = a.lon * D2R;
  const p2 = Math.asin(Math.sin(p1) * Math.cos(d) + Math.cos(p1) * Math.sin(d) * Math.cos(brg));
  const l2 = l1 + Math.atan2(Math.sin(brg) * Math.sin(d) * Math.cos(p1), Math.cos(d) - Math.sin(p1) * Math.sin(p2));
  return { lat: p2 * R2D, lon: ((((l2 * R2D) + 540) % 360) - 180) };
}

/** A closed ring of points `nm` from the centre. */
export function circle(c: LatLon, nm: number, steps = 72): LatLon[] {
  const out: LatLon[] = [];
  for (let i = 0; i <= steps; i++) out.push(destination(c, (i / steps) * 360, nm));
  return out;
}

/** Points along the great circle from a to b (for drawing the measured line). */
export function greatCircle(a: LatLon, b: LatLon, steps = 48): LatLon[] {
  const d = distanceNm(a, b);
  if (d < 1e-6) return [a, b];
  // Stepping along the initial bearing drifts on long lines, so interpolate on the sphere.
  const p1 = a.lat * D2R, l1 = a.lon * D2R, p2 = b.lat * D2R, l2 = b.lon * D2R;
  const ang = d / EARTH_NM;
  const out: LatLon[] = [];
  for (let i = 0; i <= steps; i++) {
    const f = i / steps;
    const A = Math.sin((1 - f) * ang) / Math.sin(ang), B = Math.sin(f * ang) / Math.sin(ang);
    const x = A * Math.cos(p1) * Math.cos(l1) + B * Math.cos(p2) * Math.cos(l2);
    const y = A * Math.cos(p1) * Math.sin(l1) + B * Math.cos(p2) * Math.sin(l2);
    const z = A * Math.sin(p1) + B * Math.sin(p2);
    out.push({ lat: Math.atan2(z, Math.hypot(x, y)) * R2D, lon: Math.atan2(y, x) * R2D });
  }
  return out;
}

/** The point where the sun is overhead at a time (unix ms), by the standard low-precision
 *  solar position (Astronomical Almanac, good to about 0.01° here): the sun's ecliptic
 *  longitude gives its declination and right ascension, and Greenwich mean sidereal time
 *  turns the right ascension into a longitude. */
export function subsolarPoint(ms: number): LatLon {
  const n = ms / 86400000 + 2440587.5 - 2451545.0; // days since J2000.0
  const L = (280.46 + 0.9856474 * n) % 360;
  const g = ((357.528 + 0.9856003 * n) % 360) * D2R;
  const lambda = (L + 1.915 * Math.sin(g) + 0.02 * Math.sin(2 * g)) * D2R;
  const eps = (23.439 - 0.0000004 * n) * D2R;
  const dec = Math.asin(Math.sin(eps) * Math.sin(lambda));
  const ra = Math.atan2(Math.cos(eps) * Math.sin(lambda), Math.cos(lambda)) * R2D;
  const gmst = (280.46061837 + 360.98564736629 * n) % 360;
  const lon = ((((ra - gmst) % 360) + 540) % 360) - 180;
  return { lat: dec * R2D, lon };
}

/** "12.4 NM · 23.0 km · 047°" */
export function fmtMeasure(a: LatLon, b: LatLon): { nm: string; km: string; brg: string } {
  const nm = distanceNm(a, b);
  return {
    nm: `${nm < 100 ? nm.toFixed(1) : Math.round(nm).toLocaleString()} NM`,
    km: `${nm * NM_KM < 100 ? (nm * NM_KM).toFixed(1) : Math.round(nm * NM_KM).toLocaleString()} km`,
    brg: `${String(Math.round(bearingDeg(a, b)) % 360).padStart(3, '0')}°`,
  };
}
