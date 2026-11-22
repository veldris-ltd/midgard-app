/* Projection between geographic positions and the globe's xyz frame. [FR-H-1]
 *
 * The globe is a sphere of radius 1 (or `radius`) centred on the origin, in three.js's
 * right-handed, y-up frame: the north pole on +y, the equator on the xz plane, the
 * prime meridian on +x, and east towards -z, so east is on the right when the globe is
 * seen from +x. Altitude raises the radius in proportion to the Earth's mean radius.
 * Pure and dependency-free, so node:test runs it as it is. */

/** The Earth's mean radius (IUGG), in kilometres. */
export const EARTH_RADIUS_KM = 6371.0088;

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

export interface GeoPosition {
  lat: number;
  lon: number;
  altKm: number;
}

const RAD = Math.PI / 180;

/** Latitude and longitude in degrees, altitude in km above the sphere, to xyz. */
export function toXyz(lat: number, lon: number, altKm = 0, radius = 1): Vec3 {
  const r = radius * (1 + altKm / EARTH_RADIUS_KM);
  const phi = lat * RAD;
  const lambda = lon * RAD;
  const c = Math.cos(phi);
  return { x: r * c * Math.cos(lambda), y: r * Math.sin(phi), z: -r * c * Math.sin(lambda) };
}

/** xyz back to latitude, longitude in (-180, 180] and altitude in km. */
export function fromXyz(p: Vec3, radius = 1): GeoPosition {
  const horizontal = Math.hypot(p.x, p.z);
  const r = Math.hypot(horizontal, p.y);
  const lat = Math.atan2(p.y, horizontal) / RAD;
  let lon = Math.atan2(-p.z, p.x) / RAD;
  if (lon <= -180) lon += 360;
  return { lat, lon, altKm: (r / radius - 1) * EARTH_RADIUS_KM };
}

/** Wraps a longitude into (-180, 180]. */
export function wrapLongitude(lon: number): number {
  const w = ((((lon + 180) % 360) + 360) % 360) - 180;
  return w === -180 ? 180 : w;
}

/** Great-circle angle between two positions, in degrees. */
export function angularDistanceDeg(a: { lat: number; lon: number }, b: { lat: number; lon: number }): number {
  const p1 = a.lat * RAD;
  const p2 = b.lat * RAD;
  const dp = p2 - p1;
  const dl = (b.lon - a.lon) * RAD;
  const h = Math.sin(dp / 2) ** 2 + Math.cos(p1) * Math.cos(p2) * Math.sin(dl / 2) ** 2;
  return (2 * Math.asin(Math.min(1, Math.sqrt(h)))) / RAD;
}
