/* The tilted globe camera: pitch and heading on top of the camera's position.
 * [FR-H-1]
 *
 * `lat`, `lon` and `altKm` stay what they were: the point under the camera and its
 * height, so the altitude limits and the floor apply to the camera itself. `pitch`
 * is the angle between the view and straight down (0° looks at the Earth's centre,
 * 75° towards the horizon) and `heading` the bearing the view faces, clockwise from
 * north. Tilt, turn and zoom orbit the point in the middle of the view, so it stays
 * put while the camera moves. `minKm` is the altitude floor in force: 50 km, or the
 * 1.5 km close-up while map tiles are drawn. `groundKm` is the terrain's height at
 * the point in view (km times HEIGHTS), so over hills the camera pivots on the ground
 * rather than on sea level under it. Pure, so node:test runs it. */
import {
  CAMERA_FOV_DEG,
  type CameraState,
  MIN_ALTITUDE_KM,
  TERRAIN_CLEARANCE_KM,
  clampAltitude,
  easeInOutCubic,
  flyDurationMs,
  interpolateCamera,
  normaliseCamera,
  panCamera,
} from './camera.ts';
import { EARTH_RADIUS_KM, type Vec3, fromXyz, toXyz, wrapLongitude } from './projection.ts';

export interface ViewState extends CameraState {
  /** 0° straight down, up to `maxPitchDeg(altKm)`. */
  pitch: number;
  /** Bearing of the view, 0–360°, clockwise from north. */
  heading: number;
}

/** The most the camera tilts, close in. */
export const MAX_PITCH_DEG = 75;
/** Far out, the view's centre stays this far below the horizon. */
const HORIZON_MARGIN_DEG = 6;

const RAD = Math.PI / 180;

export function wrapHeading(h: number): number {
  if (!Number.isFinite(h)) return 0;
  const w = ((h % 360) + 360) % 360;
  return w === 360 ? 0 : w;
}

/**
 * The tilt allowed at an altitude: 75° close in, less far out, so the middle of the
 * view always lands on the globe a little short of the horizon.
 */
export function maxPitchDeg(altKm: number, minKm = MIN_ALTITUDE_KM): number {
  const h = clampAltitude(altKm, minKm) / EARTH_RADIUS_KM;
  const tangent = Math.asin(1 / (1 + h)) / RAD;
  return Math.min(MAX_PITCH_DEG, Math.max(0, tangent - HORIZON_MARGIN_DEG));
}

export function normaliseView(v: CameraState & Partial<Pick<ViewState, 'pitch' | 'heading'>>, minKm = MIN_ALTITUDE_KM): ViewState {
  const c = normaliseCamera(v, minKm);
  const p = v.pitch;
  const pitch = typeof p === 'number' && Number.isFinite(p) ? Math.min(maxPitchDeg(c.altKm, minKm), Math.max(0, p)) : 0;
  return { ...c, pitch, heading: wrapHeading(v.heading ?? 0) };
}

const norm = (v: Vec3): Vec3 => {
  const l = Math.hypot(v.x, v.y, v.z);
  return { x: v.x / l, y: v.y / l, z: v.z / l };
};

/** North and east at a point, as unit tangents in the globe's frame. */
function tangents(lat: number, lon: number): { north: Vec3; east: Vec3 } {
  const p = lat * RAD, l = lon * RAD;
  return {
    north: { x: -Math.sin(p) * Math.cos(l), y: Math.cos(p), z: Math.sin(p) * Math.sin(l) },
    east: { x: -Math.sin(l), y: 0, z: -Math.cos(l) },
  };
}

/** Where the camera is (globe radii), which way it looks, and which way is up on screen. */
export function viewFrame(v: ViewState): { position: Vec3; forward: Vec3; up: Vec3 } {
  const position = toXyz(v.lat, v.lon, v.altKm);
  const radial = norm(position);
  const { north, east } = tangents(v.lat, v.lon);
  const h = v.heading * RAD, p = v.pitch * RAD;
  const ch = Math.cos(h), sh = Math.sin(h), cp = Math.cos(p), sp = Math.sin(p);
  const dir = { x: ch * north.x + sh * east.x, y: ch * north.y + sh * east.y, z: ch * north.z + sh * east.z };
  return {
    position,
    forward: { x: -cp * radial.x + sp * dir.x, y: -cp * radial.y + sp * dir.y, z: -cp * radial.z + sp * dir.z },
    up: { x: sp * radial.x + cp * dir.x, y: sp * radial.y + cp * dir.y, z: sp * radial.z + cp * dir.z },
  };
}

/** The pivot's height: the ground there, held below the camera so the view still meets it. */
function pivotGround(altKm: number, groundKm: number): number {
  return Number.isFinite(groundKm) ? Math.min(Math.max(0, groundKm), 0.8 * altKm) : 0;
}

/**
 * The point in the middle of the view, on ground `groundKm` high (0: sea level), or
 * null if the view misses the globe.
 */
export function lookTarget(v: ViewState, groundKm = 0): { lat: number; lon: number } | null {
  const { position: o, forward: d } = viewFrame(v);
  const r = 1 + pivotGround(v.altKm, groundKm) / EARTH_RADIUS_KM;
  const b = o.x * d.x + o.y * d.y + o.z * d.z;
  const c = o.x * o.x + o.y * o.y + o.z * o.z - r * r;
  const disc = b * b - c;
  if (disc < 0) return null;
  const t = -b - Math.sqrt(disc);
  if (t < 0) return null;
  const g = fromXyz({ x: o.x + t * d.x, y: o.y + t * d.y, z: o.z + t * d.z });
  return { lat: g.lat, lon: g.lon };
}

function destination(lat: number, lon: number, distDeg: number, bearingDeg: number): { lat: number; lon: number } {
  const p1 = lat * RAD, l1 = lon * RAD, d = distDeg * RAD, b = bearingDeg * RAD;
  const p2 = Math.asin(Math.sin(p1) * Math.cos(d) + Math.cos(p1) * Math.sin(d) * Math.cos(b));
  const l2 = l1 + Math.atan2(Math.sin(b) * Math.sin(d) * Math.cos(p1), Math.cos(d) - Math.sin(p1) * Math.sin(p2));
  return { lat: p2 / RAD, lon: wrapLongitude(l2 / RAD) };
}

function bearing(a: { lat: number; lon: number }, b: { lat: number; lon: number }): number {
  const p1 = a.lat * RAD, p2 = b.lat * RAD, dl = (b.lon - a.lon) * RAD;
  const y = Math.sin(dl) * Math.cos(p2);
  const x = Math.cos(p1) * Math.sin(p2) - Math.sin(p1) * Math.cos(p2) * Math.cos(dl);
  return wrapHeading(Math.atan2(y, x) / RAD);
}

/**
 * The camera `altKm` up, tilted `pitch` and facing `heading`, placed so the middle of
 * its view is `target`. The pitch is held to the limit for the altitude.
 */
export function viewLookingAt(
  target: { lat: number; lon: number },
  altKm: number,
  pitch: number,
  heading: number,
  minKm = MIN_ALTITUDE_KM,
  groundKm = 0,
): ViewState {
  const alt = clampAltitude(altKm, minKm);
  const p = Math.min(maxPitchDeg(alt, minKm), Math.max(0, Number.isFinite(pitch) ? pitch : 0));
  const h = wrapHeading(heading);
  // Triangle centre-camera-target: the angle at the camera is the pitch.
  const k = ((1 + alt / EARTH_RADIUS_KM) * Math.sin(p * RAD)) / (1 + pivotGround(alt, groundKm) / EARTH_RADIUS_KM);
  const theta = (Math.asin(Math.min(1, k)) - p * RAD) / RAD;
  if (theta < 1e-12) return normaliseView({ lat: target.lat, lon: target.lon, altKm: alt, pitch: p, heading: h }, minKm);
  // Step back from the target; correct the back-bearing until the camera faces `heading`.
  let back = h + 180;
  let n = destination(target.lat, target.lon, theta, back);
  for (let i = 0; i < 8; i++) {
    const err = wrapLongitude(h - bearing(n, target));
    if (Math.abs(err) < 1e-10) break;
    back += err;
    n = destination(target.lat, target.lon, theta, back);
  }
  return normaliseView({ lat: n.lat, lon: n.lon, altKm: alt, pitch: p, heading: h }, minKm);
}

const pivot = (v: ViewState, groundKm = 0) => lookTarget(v, groundKm) ?? { lat: v.lat, lon: v.lon };

/** Tilt by `dDeg` about the point in view, at the same altitude. */
export function tiltView(v: ViewState, dDeg: number, minKm = MIN_ALTITUDE_KM, groundKm = 0): ViewState {
  const pitch = Math.min(maxPitchDeg(v.altKm, minKm), Math.max(0, v.pitch + dDeg));
  return viewLookingAt(pivot(v, groundKm), v.altKm, pitch, v.heading, minKm, groundKm);
}

/** Turn by `dDeg` (clockwise) about the point in view. */
export function rotateView(v: ViewState, dDeg: number, minKm = MIN_ALTITUDE_KM, groundKm = 0): ViewState {
  return viewLookingAt(pivot(v, groundKm), v.altKm, v.pitch, v.heading + dDeg, minKm, groundKm);
}

/** Zoom by a factor of the altitude, keeping the point in view where it is. */
export function zoomView(v: ViewState, factor: number, minKm = MIN_ALTITUDE_KM, groundKm = 0): ViewState {
  const alt = clampAltitude(v.altKm * factor, minKm);
  if (!(v.pitch > 0)) return normaliseView({ ...v, altKm: alt }, minKm);
  return viewLookingAt(pivot(v, groundKm), alt, Math.min(v.pitch, maxPitchDeg(alt, minKm)), v.heading, minKm, groundKm);
}

/** Drag by (dx, dy) pixels: the ground follows the pointer in the turned, tilted view. */
export function panView(v: ViewState, dxPx: number, dyPx: number, viewportPx: number, fovDeg = CAMERA_FOV_DEG, minKm = MIN_ALTITUDE_KM): ViewState {
  const h = v.heading * RAD;
  const scale = Math.min(3, 1 / Math.cos(v.pitch * RAD));
  const dx = (dxPx * Math.cos(h) - dyPx * Math.sin(h)) * scale;
  const dy = (dxPx * Math.sin(h) + dyPx * Math.cos(h)) * scale;
  const c = panCamera(v, dx, dy, viewportPx, fovDeg, minKm);
  return normaliseView({ ...c, pitch: v.pitch, heading: v.heading }, minKm);
}

/** The view `t` (0–1) of the way from `a` to `b`: positions as `interpolateCamera`, tilt and heading eased with it. */
export function interpolateView(a: ViewState, b: ViewState, t: number, minKm = MIN_ALTITUDE_KM): ViewState {
  if (t <= 0) return { lat: a.lat, lon: a.lon, altKm: a.altKm, pitch: a.pitch, heading: a.heading };
  if (t >= 1) return { lat: b.lat, lon: b.lon, altKm: b.altKm, pitch: b.pitch, heading: b.heading };
  const c = interpolateCamera(a, b, t, minKm);
  const e = easeInOutCubic(t);
  return normaliseView({
    ...c,
    pitch: a.pitch + (b.pitch - a.pitch) * e,
    heading: a.heading + wrapLongitude(b.heading - a.heading) * e,
  }, minKm);
}

/** Fly time as `flyDurationMs`, with a turn or a tilt alone still animated. */
export function viewFlyDurationMs(a: ViewState, b: ViewState, reducedMotion: boolean): number {
  if (reducedMotion) return 0;
  const d = flyDurationMs(a, b, false);
  const turn = Math.abs(b.pitch - a.pitch) + Math.abs(wrapLongitude(b.heading - a.heading));
  if (d === 0 && turn > 0.5) return Math.round(600 + 900 * Math.min(1, turn / 180));
  return d;
}

/** Whether the sight line from the camera to the point in view clears the terrain. */
function sightClear(v: ViewState, groundAt: (lat: number, lon: number) => number): boolean {
  const sea = lookTarget(v);
  if (!sea) return true;
  const g = pivotGround(v.altKm, groundAt(sea.lat, sea.lon));
  const at = lookTarget(v, g) ?? sea;
  const p = toXyz(v.lat, v.lon, v.altKm);
  const t = toXyz(at.lat, at.lon, g);
  const SAMPLES = 24;
  for (let i = 1; i < SAMPLES; i++) {
    const f = i / SAMPLES;
    const s = fromXyz({ x: p.x + (t.x - p.x) * f, y: p.y + (t.y - p.y) * f, z: p.z + (t.z - p.z) * f });
    // The clearance tapers to nothing at the target, which sits on the ground.
    if (groundAt(s.lat, s.lon) + TERRAIN_CLEARANCE_KM * (1 - f) > s.altKm) return false;
  }
  return true;
}

/**
 * The most the camera may tilt where it stands without its line of sight to the point
 * in view dropping below the terrain: a hill between them tips the view down to look
 * over it. `groundAt` gives the terrain under a point, km times HEIGHTS. Straight down
 * nothing is in the way.
 */
export function sightPitchLimitDeg(v: ViewState, groundAt: (lat: number, lon: number) => number): number {
  if (!(v.pitch > 0) || sightClear(v, groundAt)) return v.pitch;
  let lo = 0, hi = v.pitch;
  for (let i = 0; i < 14; i++) {
    const mid = (lo + hi) / 2;
    if (sightClear({ ...v, pitch: mid }, groundAt)) lo = mid;
    else hi = mid;
  }
  return lo;
}
