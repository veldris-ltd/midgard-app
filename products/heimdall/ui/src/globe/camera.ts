/* The globe camera as data: where it looks from, its limits, drag and zoom, and the
 * eased fly-to. [FR-H-1]
 *
 * The camera sits `altKm` above the point (`lat`, `lon`) and looks at the globe's
 * centre, so dragging orbits it and zooming changes its altitude. The renderer turns
 * this state into a three.js camera; everything here is pure. */
import { EARTH_RADIUS_KM, angularDistanceDeg, wrapLongitude } from './projection.ts';

export interface CameraState {
  lat: number;
  lon: number;
  altKm: number;
}

/** The floor without map tiles: no closer than this to the surface. */
export const MIN_ALTITUDE_KM = 50;
/** With terrain tiles the camera may come down to this, over the sea or low ground. */
export const CLOSE_UP_ALTITUDE_KM = 1.5;
/** ...and stays at least this far above the terrain under it. */
export const TERRAIN_CLEARANCE_KM = 0.15;
export const MAX_ALTITUDE_KM = 40000;
/** Vertical field of view of the globe camera, in degrees. */
export const CAMERA_FOV_DEG = 45;
/** The camera keeps clear of the poles, where "north" stops meaning anything. */
const MAX_CAMERA_LAT = 89;

const RAD = Math.PI / 180;
const KM_PER_DEG = (Math.PI * EARTH_RADIUS_KM) / 180;

/** Altitude within the absolute limits. The renderer applies the floor in force on top
 *  (`cameraFloorKm` without tiles, `terrainFloorKm` with them). */
export function clampAltitude(altKm: number): number {
  if (Number.isNaN(altKm)) return MAX_ALTITUDE_KM;
  return Math.min(MAX_ALTITUDE_KM, Math.max(CLOSE_UP_ALTITUDE_KM, altKm));
}

/**
 * The floor over terrain: the 1.5 km close-up, or 150 m above the ground under the
 * camera (`groundKm`, already multiplied by HEIGHTS), whichever is higher.
 */
export function terrainFloorKm(groundKm: number): number {
  const g = Number.isFinite(groundKm) ? Math.max(0, groundKm) : 0;
  return clampAltitude(Math.max(CLOSE_UP_ALTITUDE_KM, g + TERRAIN_CLEARANCE_KM));
}

export function normaliseCamera(c: CameraState): CameraState {
  const lat = Number.isFinite(c.lat) ? Math.max(-MAX_CAMERA_LAT, Math.min(MAX_CAMERA_LAT, c.lat)) : 0;
  const lon = Number.isFinite(c.lon) ? wrapLongitude(c.lon) : 0;
  return { lat, lon, altKm: clampAltitude(c.altKm) };
}

/** Zoom by a factor of the altitude: below 1 closer, above 1 farther. */
export function zoomCamera(c: CameraState, factor: number): CameraState {
  return normaliseCamera({ ...c, altKm: c.altKm * factor });
}

/**
 * Drag by (dx, dy) pixels in a viewport `viewportPx` tall. The ground under the
 * pointer follows it, so dragging right turns the globe east and the camera west,
 * and dragging down moves the camera north. The rate scales with altitude.
 */
export function panCamera(c: CameraState, dxPx: number, dyPx: number, viewportPx: number, fovDeg = CAMERA_FOV_DEG): CameraState {
  const viewKm = 2 * c.altKm * Math.tan((fovDeg * RAD) / 2);
  // Past a hemisphere the view no longer grows with altitude.
  const degPerPx = Math.min(180, viewKm / KM_PER_DEG) / Math.max(1, viewportPx);
  const lonScale = Math.max(0.2, Math.cos(c.lat * RAD));
  return normaliseCamera({
    lat: c.lat + dyPx * degPerPx,
    lon: c.lon - (dxPx * degPerPx) / lonScale,
    altKm: c.altKm,
  });
}

export function easeInOutCubic(t: number): number {
  if (!(t > 0)) return 0;
  if (t >= 1) return 1;
  return t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2;
}

/**
 * The camera `t` (0–1, linear time) of the way from `a` to `b`, eased. Longitude
 * takes the short way round, altitude moves geometrically, and a long flight rises
 * mid-way so the globe stays in view.
 */
export function interpolateCamera(a: CameraState, b: CameraState, t: number): CameraState {
  if (t <= 0) return { lat: a.lat, lon: a.lon, altKm: a.altKm };
  if (t >= 1) return { lat: b.lat, lon: b.lon, altKm: b.altKm };
  const e = easeInOutCubic(t);
  const dLon = wrapLongitude(b.lon - a.lon);
  const base = Math.exp(Math.log(a.altKm) + (Math.log(b.altKm) - Math.log(a.altKm)) * e);
  const peak = clampAltitude(angularDistanceDeg(a, b) * KM_PER_DEG * 0.6);
  const lift = Math.max(0, peak - Math.max(a.altKm, b.altKm)) * Math.sin(Math.PI * e);
  return normaliseCamera({ lat: a.lat + (b.lat - a.lat) * e, lon: a.lon + dLon * e, altKm: base + lift });
}

/** How long a fly-to lasts: none under reduced motion, otherwise 0.6–2.4 s by distance. */
export function flyDurationMs(a: CameraState, b: CameraState, reducedMotion: boolean): number {
  if (reducedMotion) return 0;
  const angle = angularDistanceDeg(a, b);
  const zoom = Math.abs(Math.log(clampAltitude(b.altKm) / clampAltitude(a.altKm)));
  if (angle < 1e-6 && zoom < 1e-6) return 0;
  const effort = Math.min(1, angle / 120 + zoom / Math.log(MAX_ALTITUDE_KM / MIN_ALTITUDE_KM));
  return Math.round(600 + 1800 * effort);
}

/** Clearance kept between the camera and the highest marker below it. */
const MARKER_CLEARANCE_KM = 25;

/**
 * The lowest the camera may go: the 50 km limit, or higher when the HEIGHTS setting
 * lifts a marker near it, so no marker ever sits behind the camera or inside its
 * near plane.
 */
export function cameraFloorKm(highestMarkerKm: number): number {
  if (!(highestMarkerKm > 0)) return MIN_ALTITUDE_KM;
  return clampAltitude(Math.max(MIN_ALTITUDE_KM, highestMarkerKm + MARKER_CLEARANCE_KM));
}

/**
 * Near and far clip planes, in globe radii, for a camera `altKm` up. The near plane
 * is 2% of the altitude, so a marker 25 km under the lowest camera is still drawn,
 * and never more than half the camera's clearance over the terrain (`clearanceKm`),
 * so the ground under a close-up is never clipped; the far plane reaches past the
 * horizon.
 */
export function clipPlanes(altKm: number, clearanceKm = Infinity): { near: number; far: number } {
  const alt = clampAltitude(altKm);
  const nearKm = Math.min(alt * 0.02, Math.max(0, clearanceKm) / 2);
  return { near: Math.max(1e-6, nearKm / EARTH_RADIUS_KM), far: alt / EARTH_RADIUS_KM + 2.5 };
}
