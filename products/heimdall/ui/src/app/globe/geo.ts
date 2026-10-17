/* Globe geometry: lat/lon/altitude to scene coordinates. [FR-P3-3]
 *
 * The scene's unit is one earth radius (6371 km), centred on the earth, y up through
 * the north pole, x through (0°, 90°E), z through (0°, 0°).
 *
 * Altitude: near the surface heights are stretched so the airspace has visible depth,
 * and the stretch fades out with height, so the drawn order always matches the true
 * order. Drawn km = h + (k - 1) * L * (1 - e^(-h / L)), with k the exaggeration and
 * L = 12 km: the slope is k at the ground and falls towards 1, so nothing higher is
 * ever drawn lower. At ×15 an airliner at 11 km is drawn at 112 km, a balloon at 33 km
 * at 191 km, and everything high is raised by at most (k - 1) * L = 168 km (the ISS at
 * 420 km is drawn at 588 km). The view labels this so it is not read as true scale. */
import * as THREE from 'three';

export const EARTH_KM = 6371;
export const D2R = Math.PI / 180;
export const R2D = 180 / Math.PI;
/** How deep the near-surface stretch reaches (see the altitude note above). */
export const STRETCH_KM = 12;

/** Drawn height above the surface in earth radii, from true metres. */
export function drawnHeight(altM: number | null | undefined, exaggeration: number): number {
  if (altM === null || altM === undefined || altM <= 0) return 0;
  const km = altM / 1000;
  return (km + (exaggeration - 1) * STRETCH_KM * (1 - Math.exp(-km / STRETCH_KM))) / EARTH_KM;
}

/** The scene point at lat/lon (degrees), `r` earth radii from the centre. */
export function toVec(lat: number, lon: number, r: number, out = new THREE.Vector3()): THREE.Vector3 {
  const p = lat * D2R, l = lon * D2R;
  return out.set(r * Math.cos(p) * Math.sin(l), r * Math.sin(p), r * Math.cos(p) * Math.cos(l));
}

/** Latitude and longitude (degrees) of a scene direction. */
export function toLatLon(v: THREE.Vector3): { lat: number; lon: number } {
  const r = v.length() || 1;
  return { lat: Math.asin(THREE.MathUtils.clamp(v.y / r, -1, 1)) * R2D, lon: Math.atan2(v.x, v.z) * R2D };
}

const Y = new THREE.Vector3(0, 1, 0);
const X = new THREE.Vector3(1, 0, 0);

/** The local east / north / up frame at a scene point. */
export function enu(p: THREE.Vector3, east: THREE.Vector3, north: THREE.Vector3, up: THREE.Vector3): void {
  up.copy(p).normalize();
  east.crossVectors(Math.abs(up.y) > 0.9999 ? X : Y, up).normalize();
  north.crossVectors(up, east).normalize();
}

/** Wrap a longitude to -180..180. */
export const wrapLon = (lon: number) => ((((lon + 180) % 360) + 360) % 360) - 180;

/** Great-circle angle between two lat/lon points, radians. */
export function arc(lat1: number, lon1: number, lat2: number, lon2: number): number {
  const p1 = lat1 * D2R, p2 = lat2 * D2R;
  const h = Math.sin((p2 - p1) / 2) ** 2 + Math.cos(p1) * Math.cos(p2) * Math.sin(((lon2 - lon1) * D2R) / 2) ** 2;
  return 2 * Math.asin(Math.min(1, Math.sqrt(h)));
}

/** The point a fraction `f` along the great circle between two points. */
export function slerpLatLon(lat1: number, lon1: number, lat2: number, lon2: number, f: number): { lat: number; lon: number } {
  const a = toVec(lat1, lon1, 1);
  const b = toVec(lat2, lon2, 1);
  const d = a.angleTo(b);
  if (d < 1e-9) return { lat: lat2, lon: lon2 };
  const s = Math.sin(d);
  const v = a.multiplyScalar(Math.sin((1 - f) * d) / s).add(b.multiplyScalar(Math.sin(f * d) / s));
  return toLatLon(v);
}

/** Where a ray first meets the sphere of radius `r` (null if it misses). */
export function raySphere(origin: THREE.Vector3, dir: THREE.Vector3, r = 1): number | null {
  const b = origin.dot(dir);
  const c = origin.lengthSq() - r * r;
  const disc = b * b - c;
  if (disc < 0) return null;
  const s = Math.sqrt(disc);
  const t0 = -b - s;
  if (t0 > 0) return t0;
  const t1 = -b + s;
  return t1 > 0 ? t1 : null;
}
