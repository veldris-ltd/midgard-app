/* The globe camera: where it looks, from how far, at what heading and tilt, plus
 * animated fly-to and inertia. Input handling lives in GlobeView; this is the state
 * and the arithmetic. [FR-P3-3] [FR-P3-7]
 *
 * The camera looks at a point on the surface (`lat`, `lon`) from `dist` earth radii away.
 * `heading` turns the view about the local vertical (0 = north up the screen) and `tilt`
 * leans it from straight down (0°) towards the horizon. Zoom is clamped from 50 km above
 * the surface out to five earth radii, and the tilt allowed shrinks as the camera pulls
 * back so the globe stays in view. */
import * as THREE from 'three';
import { D2R, EARTH_KM, arc, enu, raySphere, slerpLatLon, toLatLon, toVec, wrapLon } from './geo';

export interface CameraState {
  lat: number;
  lon: number;
  /** Distance from the looked-at point, earth radii. */
  dist: number;
  heading: number;
  tilt: number;
}

export const MIN_DIST = 50 / EARTH_KM;
export const MAX_DIST = 5;
/** The opening framing: the UK and Ireland, oblique, looking north. */
/** Over the UK and Ireland, tilted enough that altitude reads as depth while the
 *  whole region stays in view. [FR-P3-7] */
/** The Baltic, where the open (keyless) ship feed reports. [FR-P3-7] */
export const BALTIC_VIEW: CameraState = { lat: 59.3, lon: 22.5, dist: 1400 / EARTH_KM, heading: 0, tilt: 45 };

export const UK_VIEW: CameraState = { lat: 54.2, lon: -3.6, dist: 2000 / EARTH_KM, heading: -8, tilt: 48 };

/** Most tilt allowed at a distance: near the ground almost to the horizon, none from afar. */
export const maxTilt = (dist: number) => THREE.MathUtils.clamp(78 - (dist - 0.3) * 24, 0, 78);

const ease = (x: number) => (x < 0.5 ? 4 * x * x * x : 1 - (-2 * x + 2) ** 3 / 2);

export class GlobeCamera {
  state: CameraState = { ...UK_VIEW };
  private fly: { from: CameraState; to: CameraState; t0: number; ms: number; hop: number } | null = null;
  /** Pan velocity after a release, degrees per ms. */
  private spin: { dLat: number; dLon: number; last: number } | null = null;

  constructor(readonly camera: THREE.PerspectiveCamera) {}

  clamp(): void {
    const s = this.state;
    s.lat = THREE.MathUtils.clamp(s.lat, -89.5, 89.5);
    s.lon = wrapLon(s.lon);
    s.dist = THREE.MathUtils.clamp(s.dist, MIN_DIST, MAX_DIST);
    s.heading = ((s.heading % 360) + 360) % 360;
    s.tilt = THREE.MathUtils.clamp(s.tilt, 0, maxTilt(s.dist));
  }

  /** Place the three.js camera from the state. */
  apply(): void {
    this.clamp();
    const s = this.state;
    const p = toVec(s.lat, s.lon, 1);
    const east = new THREE.Vector3(), north = new THREE.Vector3(), up = new THREE.Vector3();
    enu(p, east, north, up);
    const h = s.heading * D2R, t = s.tilt * D2R;
    const fwd = north.clone().multiplyScalar(Math.cos(h)).addScaledVector(east, Math.sin(h));
    const back = up.clone().multiplyScalar(Math.cos(t)).addScaledVector(fwd, -Math.sin(t));
    const cam = this.camera;
    cam.position.copy(p).addScaledVector(back, s.dist);
    cam.up.copy(fwd).multiplyScalar(Math.cos(t)).addScaledVector(up, Math.sin(t));
    cam.lookAt(p);
    // Depth range sized to the view: near enough for 50 km, far enough for GEO.
    const alt = cam.position.length() - 1;
    cam.near = Math.max(1e-4, alt * 0.05);
    cam.far = cam.position.length() + 8;
    cam.updateProjectionMatrix();
    cam.updateMatrixWorld();
  }

  /** The surface point under a viewport position (normalised -1..1), or null off the globe. */
  surfaceAt(ndcX: number, ndcY: number): { lat: number; lon: number } | null {
    const ray = new THREE.Raycaster();
    ray.setFromCamera(new THREE.Vector2(ndcX, ndcY), this.camera);
    const t = raySphere(ray.ray.origin, ray.ray.direction, 1);
    if (t === null) return null;
    return toLatLon(ray.ray.origin.clone().addScaledVector(ray.ray.direction, t));
  }

  /** Move the look point so that `grab` (a surface point) sits under the viewport
   *  position again: the "grab the earth" pan. Two passes converge for tilted views. */
  keepUnder(grab: { lat: number; lon: number }, ndcX: number, ndcY: number): boolean {
    for (let i = 0; i < 2; i++) {
      this.apply();
      const now = this.surfaceAt(ndcX, ndcY);
      if (!now) return false;
      let dLon = grab.lon - now.lon;
      if (dLon > 180) dLon -= 360;
      if (dLon < -180) dLon += 360;
      this.state.lat += grab.lat - now.lat;
      this.state.lon += dLon;
    }
    this.apply();
    return true;
  }

  /** Pan by a fraction of the view height, in screen directions (for the keyboard). */
  panScreen(dx: number, dy: number): void {
    const span = this.state.dist * 2 * Math.tan((this.camera.fov * D2R) / 2);
    const h = this.state.heading * D2R;
    const north = (-dy * Math.cos(h) - dx * Math.sin(h)) * span;
    const east = (dx * Math.cos(h) - dy * Math.sin(h)) * span;
    this.state.lat += (north * 180) / Math.PI;
    this.state.lon += (east * 180) / Math.PI / Math.max(0.05, Math.cos(this.state.lat * D2R));
    this.clamp();
  }

  stop(): void {
    this.fly = null;
    this.spin = null;
  }

  flyTo(to: Partial<CameraState>, ms = 1200): void {
    const from = { ...this.state };
    const goal: CameraState = { ...from, ...to };
    goal.dist = THREE.MathUtils.clamp(goal.dist, MIN_DIST, MAX_DIST);
    goal.tilt = THREE.MathUtils.clamp(goal.tilt, 0, maxTilt(goal.dist));
    // A long hop pulls back mid-flight so the route stays readable.
    const hop = Math.max(0, arc(from.lat, from.lon, goal.lat, goal.lon) * 1.1 - Math.max(from.dist, goal.dist));
    this.spin = null;
    this.fly = { from, to: goal, t0: performance.now(), ms, hop };
  }

  /** Carry on panning after a release, at this velocity (degrees per ms). */
  coast(dLat: number, dLon: number): void {
    if (Math.hypot(dLat, dLon) < 1e-6) return;
    this.spin = { dLat, dLon, last: performance.now() };
  }

  /** Advance animations; true while one is still running. */
  step(nowMs: number): boolean {
    if (this.fly) {
      const { from, to, t0, ms, hop } = this.fly;
      const f = Math.min(1, (nowMs - t0) / ms);
      const e = ease(f);
      const ll = slerpLatLon(from.lat, from.lon, to.lat, to.lon, e);
      let dh = to.heading - from.heading;
      if (dh > 180) dh -= 360;
      if (dh < -180) dh += 360;
      this.state = {
        lat: ll.lat,
        lon: ll.lon,
        dist: Math.exp(Math.log(from.dist) + (Math.log(to.dist) - Math.log(from.dist)) * e) + hop * Math.sin(Math.PI * e),
        heading: from.heading + dh * e,
        tilt: from.tilt + (to.tilt - from.tilt) * e,
      };
      if (f >= 1) this.fly = null;
      return true;
    }
    if (this.spin) {
      const dt = Math.min(64, nowMs - this.spin.last);
      this.spin.last = nowMs;
      this.state.lat += this.spin.dLat * dt;
      this.state.lon += this.spin.dLon * dt;
      const decay = Math.exp(-dt / 280);
      this.spin.dLat *= decay;
      this.spin.dLon *= decay;
      if (Math.hypot(this.spin.dLat, this.spin.dLon) < 2e-6 * Math.max(0.02, this.state.dist)) this.spin = null;
      return true;
    }
    return false;
  }

  get animating(): boolean {
    return !!this.fly || !!this.spin;
  }
}
