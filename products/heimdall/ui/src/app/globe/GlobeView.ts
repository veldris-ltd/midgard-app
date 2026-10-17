/* The 3D operating picture: a three.js globe with every entity at its latitude,
 * longitude and altitude, and a touch-first camera. [FR-P3-3] [FR-P3-7]
 *
 * Scene: a dark sphere, Natural Earth land outlines (1:110m worldwide, 1:50m with
 * borders over the North Atlantic and Europe, from basemap.ts), a 10° graticule and an
 * atmosphere rim. Entities are instanced by shape (aircraft arrows oriented by track,
 * balloons with a drop line to the ground, satellite diamonds, earthquake rings that
 * pulse, flood discs, plain dots for anything else), so thousands stay cheap. Markers
 * keep a constant size on screen: their scale is set from the camera distance each
 * frame.
 *
 * Rendering is on demand: a frame is drawn only when data or the camera changes, or
 * while an animation (fly-to, inertia, the earthquake pulse at ~15 fps) runs.
 *
 * Input (Pointer Events, `touch-action: none`):
 *   mouse  left-drag pans (grab the earth), right- or Ctrl/Shift-drag turns heading
 *          (horizontal) and tilt (vertical), wheel zooms about the cursor, double-click
 *          flies in to the point.
 *   touch  one finger pans, pinch zooms about the pinch centre, twist turns heading,
 *          two fingers dragged vertically together tilt, double-tap flies in.
 *   keys   arrows pan, + / - zoom, Q / E heading, W / S tilt, R resets to the UK.
 * Picking is done in screen space against every instance (nearest marker within 10 px
 * for a mouse, 28 px for touch, ignoring those behind the earth). */
import * as THREE from 'three';
import type { HeimdallHistoryPoint } from '@midgard/sdk';
import type { TrackShape } from '../trackStyle';
import { BORDER_PATH, LAND_PATH } from '../basemap';
import { GLOBE_LAND_110M } from '../globeLand';
import { GlobeCamera, UK_VIEW, maxTilt, type CameraState } from './GlobeCamera';
import { D2R, drawnHeight, enu, raySphere, toVec } from './geo';

export interface GlobeItem {
  id: string;
  shape: TrackShape;
  lat: number;
  lon: number;
  altM: number | null;
  /** Track over the ground, degrees. */
  hdg: number;
  /** CSS colour (token or literal). */
  colour: string;
  station?: boolean;
  magnitude?: number;
}

export interface GlobeTrail {
  colour: string;
  points: HeimdallHistoryPoint[];
}

export interface GlobeCallbacks {
  onSelect: (id: string) => void;
  onHover: (id: string | null) => void;
  /** After every drawn frame, so overlays can follow the camera. */
  onFrame: () => void;
  /** Follow-selected was switched off by the user panning. */
  onFollowEnd: () => void;
}

/** Marker sizes in CSS pixels. */
const PX: Record<TrackShape, number> = { aircraft: 13, balloon: 7, satellite: 8, quake: 9, flood: 8, point: 6 };
const SHAPES: TrackShape[] = ['aircraft', 'balloon', 'satellite', 'quake', 'flood', 'point'];
const GLOBE_COLOUR = '#0a0f16';
/** A screen tap: moved less than this (px) within this long (ms). */
const TAP_PX = { mouse: 5, touch: 12 };
const TAP_MS = 450;
const DOUBLE_TAP_MS = 320;

/** Parse basemap.ts's SVG path (x = lon, y = -lat; "M x y x y … Z") into rings. */
function svgRings(d: string): [number, number][][] {
  const rings: [number, number][][] = [];
  for (const part of d.split('M')) {
    const nums = part.replace(/Z/g, ' ').trim().split(/[\s,]+/).filter(Boolean).map(Number);
    if (nums.length < 4) continue;
    const ring: [number, number][] = [];
    for (let i = 0; i + 1 < nums.length; i += 2) ring.push([nums[i], -nums[i + 1]]);
    if (/Z\s*$/.test(part)) ring.push(ring[0]);
    rings.push(ring);
  }
  return rings;
}

/** Line strips (lon, lat) → LineSegments positions on a sphere of radius `r`. */
function stripsToSegments(strips: [number, number][][], r: number): Float32Array {
  let n = 0;
  for (const s of strips) n += Math.max(0, s.length - 1);
  const out = new Float32Array(n * 6);
  const a = new THREE.Vector3(), b = new THREE.Vector3();
  let k = 0;
  for (const s of strips) {
    for (let i = 0; i + 1 < s.length; i++) {
      toVec(s[i][1], s[i][0], r, a);
      toVec(s[i + 1][1], s[i + 1][0], r, b);
      out[k++] = a.x; out[k++] = a.y; out[k++] = a.z;
      out[k++] = b.x; out[k++] = b.y; out[k++] = b.z;
    }
  }
  return out;
}

function graticule(stepDeg: number): [number, number][][] {
  const out: [number, number][][] = [];
  for (let lat = -90 + stepDeg; lat < 90; lat += stepDeg) {
    const ring: [number, number][] = [];
    for (let lon = -180; lon <= 180; lon += 2) ring.push([lon, lat]);
    out.push(ring);
  }
  for (let lon = -180; lon < 180; lon += stepDeg) {
    const mer: [number, number][] = [];
    for (let lat = -88; lat <= 88; lat += 2) mer.push([lon, lat]);
    out.push(mer);
  }
  return out;
}

const ATMOSPHERE_VERT = `
varying vec3 vNormal;
varying vec3 vView;
void main() {
  vec4 mv = modelViewMatrix * vec4(position, 1.0);
  vNormal = normalize(normalMatrix * normal);
  vView = normalize(-mv.xyz);
  gl_Position = projectionMatrix * mv;
}`;
const ATMOSPHERE_FRAG = `
uniform vec3 uColour;
varying vec3 vNormal;
varying vec3 vView;
void main() {
  // Back faces of a shell just outside the earth: facing the viewer (d near 1) only
  // behind the earth, edge-on (d = 0) at the shell's rim, so the glow hugs the limb
  // and fades outwards.
  float d = abs(dot(vNormal, vView));
  float a = smoothstep(0.0, 0.3, d) * 0.42;
  gl_FragColor = vec4(uColour, a);
}`;

interface Placed {
  item: GlobeItem;
  pos: THREE.Vector3;
  /** Local frame: right/forward/up for arrows, east/north/up for surface marks. */
  a: THREE.Vector3;
  b: THREE.Vector3;
  up: THREE.Vector3;
  shapeIndex: number;
}

export class GlobeView {
  private readonly renderer: THREE.WebGLRenderer;
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(40, 1, 1e-4, 20);
  readonly cam: GlobeCamera;
  private readonly ro: ResizeObserver;
  private readonly meshes = new Map<TrackShape, THREE.InstancedMesh>();
  private readonly geometries = new Map<TrackShape, THREE.BufferGeometry>();
  private readonly materials = new Map<TrackShape, THREE.MeshBasicMaterial>();
  private placed: Placed[] = [];
  private byId = new Map<string, Placed>();
  private items: GlobeItem[] = [];
  private trails: GlobeTrail[] = [];
  private path: HeimdallHistoryPoint[] | null = null;
  private pathColour = 'var(--accent-hover)';
  private selected: string | null = null;
  private exaggeration = 15;
  private follow = false;
  private width = 1;
  private height = 1;
  /** Canvas pixels covered by panels along the bottom: the view centres above them. */
  private bottom = 0;
  private frame = 0;
  private pulseTimer = 0;
  private disposed = false;
  private readonly colourCache = new Map<string, THREE.Color>();
  private readonly drops: THREE.LineSegments;
  private readonly trailLines: THREE.LineSegments;
  private readonly pathLine: THREE.Line;
  private readonly marker: THREE.Mesh;
  // input
  private readonly pointers = new Map<number, { x: number; y: number; x0: number; y0: number; t0: number; type: string }>();
  private mode: 'none' | 'pan' | 'turn' | 'two' | 'pinch' | 'tilt' = 'none';
  private grab: { lat: number; lon: number } | null = null;
  private two = { cx: 0, cy: 0, span: 0, angle: 0, cx0: 0, cy0: 0, span0: 0, moved: [0, 0, 0, 0] };
  private samples: { t: number; lat: number; lon: number }[] = [];
  private lastTap = { t: 0, x: 0, y: 0 };
  /** The current gesture has had two fingers down (so its last lift is not a tap). */
  private multi = false;
  private hoverPending: { x: number; y: number } | null = null;
  private hoverFrame = 0;
  private readonly listeners: [EventTarget, string, EventListener, AddEventListenerOptions?][] = [];

  constructor(private readonly host: HTMLElement, private readonly cb: GlobeCallbacks) {
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: 'high-performance' });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.setClearColor(0x000000, 0);
    const canvas = this.renderer.domElement;
    canvas.style.cssText = 'display:block;width:100%;height:100%;touch-action:none;outline:none;';
    host.appendChild(canvas);
    this.cam = new GlobeCamera(this.camera);

    // Earth, outlines, graticule, atmosphere.
    const globe = new THREE.Mesh(
      new THREE.SphereGeometry(1, 128, 96),
      new THREE.MeshBasicMaterial({ color: GLOBE_COLOUR, polygonOffset: true, polygonOffsetFactor: 1, polygonOffsetUnits: 1 }),
    );
    this.scene.add(globe);
    const lines = (pos: Float32Array, colour: string, opacity: number) => {
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
      const m = new THREE.LineSegments(g, new THREE.LineBasicMaterial({ color: this.colour(colour), transparent: true, opacity, depthWrite: false }));
      this.scene.add(m);
      return m;
    };
    const world = GLOBE_LAND_110M.split('|').map((s) => {
      const n = s.split(' ').map(Number);
      const out: [number, number][] = [];
      for (let i = 0; i + 1 < n.length; i += 2) out.push([n[i], n[i + 1]]);
      return out;
    });
    lines(stripsToSegments(graticule(10), 1.0003), 'var(--text-faint)', 0.16);
    lines(stripsToSegments([...world, ...svgRings(LAND_PATH)], 1.0006), 'var(--text-muted)', 0.6);
    lines(stripsToSegments(svgRings(BORDER_PATH), 1.0006), 'var(--text-faint)', 0.35);
    const atmosphere = new THREE.Mesh(
      new THREE.SphereGeometry(1.045, 96, 64),
      new THREE.ShaderMaterial({
        vertexShader: ATMOSPHERE_VERT,
        fragmentShader: ATMOSPHERE_FRAG,
        uniforms: { uColour: { value: this.colour('var(--info)') } },
        side: THREE.BackSide,
        transparent: true,
        depthWrite: false,
        blending: THREE.AdditiveBlending,
      }),
    );
    this.scene.add(atmosphere);

    // Entity markers, one instanced mesh per shape (grown on demand).
    this.geometries.set('aircraft', new THREE.ConeGeometry(0.5, 1, 3));
    this.geometries.set('balloon', new THREE.SphereGeometry(0.5, 12, 8));
    this.geometries.set('satellite', new THREE.OctahedronGeometry(0.5));
    this.geometries.set('quake', new THREE.RingGeometry(0.72, 1, 40));
    this.geometries.set('flood', new THREE.CircleGeometry(1, 28));
    this.geometries.set('point', new THREE.SphereGeometry(0.5, 8, 6));
    for (const s of SHAPES) {
      const flat = s === 'quake' || s === 'flood';
      this.materials.set(s, new THREE.MeshBasicMaterial({
        color: 0xffffff,
        transparent: flat,
        opacity: s === 'flood' ? 0.6 : s === 'quake' ? 0.9 : 1,
        side: flat ? THREE.DoubleSide : THREE.FrontSide,
        depthWrite: !flat,
      }));
    }

    const vcLines = () => {
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(0), 3));
      g.setAttribute('color', new THREE.BufferAttribute(new Float32Array(0), 3));
      return g;
    };
    this.drops = new THREE.LineSegments(vcLines(), new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, opacity: 0.45, depthWrite: false }));
    this.trailLines = new THREE.LineSegments(vcLines(), new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, opacity: 0.5, depthWrite: false }));
    this.pathLine = new THREE.Line(vcLines(), new THREE.LineBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.95, depthWrite: false }));
    this.marker = new THREE.Mesh(new THREE.RingGeometry(0.78, 1, 40), new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.95, depthTest: false, side: THREE.DoubleSide }));
    this.marker.renderOrder = 10;
    this.marker.visible = false;
    this.scene.add(this.drops, this.trailLines, this.pathLine, this.marker);

    this.ro = new ResizeObserver(() => this.resize());
    this.ro.observe(host);
    this.resize();
    this.bindInput(canvas);
    this.requestRender();
  }

  // ---- data ------------------------------------------------------------------

  /** A CSS colour (`var(--token)` or literal) as a three.js colour, resolved against the
   *  console's theme. */
  private colour(css: string): THREE.Color {
    const hit = this.colourCache.get(css);
    if (hit) return hit;
    let value = css;
    const m = /^var\((--[\w-]+)\)$/.exec(css.trim());
    if (m) value = getComputedStyle(this.host).getPropertyValue(m[1]).trim() || '#888888';
    const c = new THREE.Color();
    try {
      c.setStyle(value);
    } catch {
      c.set('#888888');
    }
    this.colourCache.set(css, c);
    return c;
  }

  setItems(items: GlobeItem[]): void {
    this.items = items;
    this.place();
    if (this.follow) this.followSelected();
    this.requestRender();
  }

  setTrails(trails: GlobeTrail[]): void {
    this.trails = trails;
    this.buildTrails();
    this.requestRender();
  }

  setSelected(id: string | null, path: HeimdallHistoryPoint[] | null, colour?: string): void {
    this.selected = id;
    this.path = path;
    if (colour) this.pathColour = colour;
    this.buildPath();
    if (this.follow) this.followSelected();
    this.requestRender();
  }

  setExaggeration(x: number): void {
    if (x === this.exaggeration) return;
    this.exaggeration = x;
    this.place();
    this.buildTrails();
    this.buildPath();
    this.requestRender();
  }

  setFollow(on: boolean): void {
    this.follow = on;
    if (on) this.followSelected(true);
    this.requestRender();
  }

  private followSelected(animate = false): void {
    const p = this.selected ? this.byId.get(this.selected) : undefined;
    if (!p) return;
    if (animate) this.cam.flyTo({ lat: p.item.lat, lon: p.item.lon }, 800);
    else if (!this.cam.animating) {
      this.cam.state.lat = p.item.lat;
      this.cam.state.lon = p.item.lon;
    }
  }

  /** Scene positions and local frames for every item; instance colours. */
  private place(): void {
    const counts = new Map<TrackShape, number>();
    for (const s of SHAPES) counts.set(s, 0);
    for (const it of this.items) counts.set(it.shape, counts.get(it.shape)! + 1);
    for (const s of SHAPES) this.ensure(s, counts.get(s)!);
    const next = new Map<TrackShape, number>();
    for (const s of SHAPES) next.set(s, 0);
    const placed: Placed[] = [];
    const byId = new Map<string, Placed>();
    const east = new THREE.Vector3(), north = new THREE.Vector3();
    for (const it of this.items) {
      const surface = it.shape === 'quake' || it.shape === 'flood';
      const r = 1 + (surface ? 0.0009 : drawnHeight(it.altM, this.exaggeration) + 0.0004);
      const pos = toVec(it.lat, it.lon, r);
      const up = new THREE.Vector3();
      enu(pos, east, north, up);
      let a: THREE.Vector3, b: THREE.Vector3;
      if (it.shape === 'aircraft') {
        const h = it.hdg * D2R;
        b = north.clone().multiplyScalar(Math.cos(h)).addScaledVector(east, Math.sin(h)); // forward
        a = new THREE.Vector3().crossVectors(b, up); // right
      } else {
        a = east.clone();
        b = north.clone();
      }
      const idx = next.get(it.shape)!;
      next.set(it.shape, idx + 1);
      const p: Placed = { item: it, pos, a, b, up, shapeIndex: idx };
      this.meshes.get(it.shape)!.setColorAt(idx, this.colour(it.colour));
      placed.push(p);
      byId.set(it.id, p);
    }
    for (const s of SHAPES) {
      const mesh = this.meshes.get(s)!;
      mesh.count = counts.get(s)!;
      if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
    }
    this.placed = placed;
    this.byId = byId;
    this.buildDrops();
  }

  private ensure(shape: TrackShape, n: number): void {
    const have = this.meshes.get(shape);
    if (have && have.instanceMatrix.count >= n) return;
    const cap = Math.max(64, 2 ** Math.ceil(Math.log2(Math.max(1, n))));
    if (have) {
      this.scene.remove(have);
      have.dispose();
    }
    const mesh = new THREE.InstancedMesh(this.geometries.get(shape)!, this.materials.get(shape)!, cap);
    mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
    mesh.setColorAt(0, new THREE.Color(1, 1, 1));
    mesh.frustumCulled = false;
    mesh.count = 0;
    if (shape === 'quake' || shape === 'flood') mesh.renderOrder = 1;
    this.meshes.set(shape, mesh);
    this.scene.add(mesh);
  }

  private setLines(obj: THREE.LineSegments | THREE.Line, pos: number[], col: number[]): void {
    const g = obj.geometry;
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(pos), 3));
    g.setAttribute('color', new THREE.BufferAttribute(new Float32Array(col), 3));
    g.computeBoundingSphere();
  }

  private buildDrops(): void {
    const pos: number[] = [], col: number[] = [];
    const ground = new THREE.Vector3();
    for (const p of this.placed) {
      if (p.item.shape !== 'balloon') continue;
      toVec(p.item.lat, p.item.lon, 1.0006, ground);
      const c = this.colour(p.item.colour);
      pos.push(ground.x, ground.y, ground.z, p.pos.x, p.pos.y, p.pos.z);
      col.push(c.r, c.g, c.b, c.r, c.g, c.b);
    }
    this.setLines(this.drops, pos, col);
  }

  private pointAt(pt: HeimdallHistoryPoint, out: THREE.Vector3): THREE.Vector3 {
    return toVec(pt[1], pt[2], 1 + drawnHeight(pt[3], this.exaggeration) + 0.0004, out);
  }

  private buildTrails(): void {
    const pos: number[] = [], col: number[] = [];
    const a = new THREE.Vector3(), b = new THREE.Vector3();
    for (const tr of this.trails) {
      if (tr.points.length < 2) continue;
      const c = this.colour(tr.colour);
      this.pointAt(tr.points[0], a);
      for (let i = 1; i < tr.points.length; i++) {
        this.pointAt(tr.points[i], b);
        // Fade towards the tail.
        const f0 = 0.25 + 0.75 * ((i - 1) / (tr.points.length - 1));
        const f1 = 0.25 + 0.75 * (i / (tr.points.length - 1));
        pos.push(a.x, a.y, a.z, b.x, b.y, b.z);
        col.push(c.r * f0, c.g * f0, c.b * f0, c.r * f1, c.g * f1, c.b * f1);
        a.copy(b);
      }
    }
    this.setLines(this.trailLines, pos, col);
  }

  private buildPath(): void {
    const pos: number[] = [], col: number[] = [];
    const v = new THREE.Vector3();
    const c = this.colour(this.pathColour);
    for (const pt of this.path ?? []) {
      this.pointAt(pt, v);
      pos.push(v.x, v.y, v.z);
      col.push(c.r, c.g, c.b);
    }
    this.setLines(this.pathLine, pos, col);
    (this.pathLine.material as THREE.LineBasicMaterial).color.copy(c);
    this.pathLine.visible = pos.length >= 6;
  }

  // ---- frames ----------------------------------------------------------------

  requestRender(): void {
    if (this.frame || this.disposed) return;
    this.frame = requestAnimationFrame((t) => this.draw(t));
  }

  private resize(): void {
    const w = Math.max(1, this.host.clientWidth);
    const h = Math.max(1, this.host.clientHeight);
    this.width = w;
    this.height = h;
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.setSize(w, h, false);
    this.frameView();
    this.requestRender();
  }

  /** Keep the look point centred in the part of the canvas not under the bottom panels:
   *  render the top of a taller virtual view whose centre sits above them. */
  setInsets(bottom: number): void {
    const b = Math.round(THREE.MathUtils.clamp(bottom, 0, this.height * 0.6));
    if (b === this.bottom) return;
    this.bottom = b;
    this.frameView();
    this.requestRender();
  }

  private frameView(): void {
    const { width: w, height: h, bottom: b } = this;
    this.camera.aspect = w / (h + b);
    if (b > 0) this.camera.setViewOffset(w, h + b, 0, b, w, h);
    else this.camera.clearViewOffset();
    this.camera.updateProjectionMatrix();
  }

  /** World units per CSS pixel at a distance from the camera. */
  private perPx(dist: number): number {
    return (2 * dist * Math.tan((this.camera.fov * D2R) / 2)) / (this.height + this.bottom);
  }

  private draw(nowMs: number): void {
    this.frame = 0;
    if (this.disposed) return;
    const animating = this.cam.step(nowMs);
    this.cam.apply();
    let quakes = false;
    const m = new THREE.Matrix4();
    const scale = new THREE.Vector3();
    const camPos = this.camera.position;
    for (const p of this.placed) {
      const s = p.item.shape;
      const d = camPos.distanceTo(p.pos);
      let px = PX[s];
      if (s === 'satellite' && p.item.station) px = 14;
      if (s === 'quake') {
        quakes = true;
        const pulse = 1 + 0.22 * Math.sin(nowMs / 420 + p.shapeIndex);
        px = (4 + 2.2 * Math.max(0, p.item.magnitude ?? 3)) * pulse;
      }
      const k = px * this.perPx(d);
      if (s === 'aircraft') scale.set(k * 0.62, k, k * 0.3);
      else scale.set(k, k, k);
      m.makeBasis(p.a, p.b, p.up);
      m.scale(scale);
      m.setPosition(p.pos);
      this.meshes.get(s)!.setMatrixAt(p.shapeIndex, m);
    }
    for (const mesh of this.meshes.values()) mesh.instanceMatrix.needsUpdate = true;

    const sel = this.selected ? this.byId.get(this.selected) : undefined;
    this.marker.visible = !!sel;
    if (sel) {
      this.marker.position.copy(sel.pos);
      this.marker.quaternion.copy(this.camera.quaternion);
      this.marker.scale.setScalar(15 * this.perPx(camPos.distanceTo(sel.pos)));
      (this.marker.material as THREE.MeshBasicMaterial).color.copy(this.colour('var(--grey-100)'));
    }

    this.renderer.render(this.scene, this.camera);
    // The camera, readable by tests and assistive tooling without reaching into three.js.
    const c = this.cam.state;
    this.host.dataset.camera = `${c.lat.toFixed(3)},${c.lon.toFixed(3)},${(c.dist * 6371).toFixed(0)}km,${c.heading.toFixed(1)},${c.tilt.toFixed(1)}`;
    this.cb.onFrame();
    if (animating) this.requestRender();
    else if (quakes && !document.hidden) {
      window.clearTimeout(this.pulseTimer);
      this.pulseTimer = window.setTimeout(() => this.requestRender(), 66);
    }
  }

  // ---- picking + overlays ----------------------------------------------------

  private occluded(pos: THREE.Vector3): boolean {
    const cam = this.camera.position;
    const dir = pos.clone().sub(cam);
    const len = dir.length();
    dir.divideScalar(len);
    const t = raySphere(cam, dir, 0.9995);
    return t !== null && t < len;
  }

  /** CSS-pixel position of an item in the canvas, or null when off screen / behind the earth. */
  screenOf(id: string): { x: number; y: number } | null {
    const p = this.byId.get(id);
    if (!p || this.occluded(p.pos)) return null;
    const v = p.pos.clone().project(this.camera);
    if (v.z > 1 || v.x < -1.1 || v.x > 1.1 || v.y < -1.1 || v.y > 1.1) return null;
    return { x: ((v.x + 1) / 2) * this.width, y: ((1 - v.y) / 2) * this.height };
  }

  /** The nearest visible item within `tol` CSS px of a canvas position. */
  pick(x: number, y: number, tol: number): string | null {
    let best: string | null = null;
    let bestD = tol * tol;
    const v = new THREE.Vector3();
    for (const p of this.placed) {
      v.copy(p.pos).project(this.camera);
      if (v.z > 1) continue;
      const sx = ((v.x + 1) / 2) * this.width - x;
      const sy = ((1 - v.y) / 2) * this.height - y;
      const d = sx * sx + sy * sy;
      if (d < bestD && !this.occluded(p.pos)) {
        bestD = d;
        best = p.item.id;
      }
    }
    return best;
  }

  // ---- on-screen controls ------------------------------------------------------

  zoomBy(f: number): void {
    this.cam.flyTo({ dist: this.cam.state.dist * f }, 350);
    this.requestRender();
  }
  northUp(): void {
    this.cam.flyTo({ heading: 0 }, 600);
    this.requestRender();
  }
  home(): void {
    this.endFollow();
    this.cam.flyTo({ ...UK_VIEW }, 1400);
    this.requestRender();
  }
  /** Top-down ↔ oblique. */
  toggleTilt(): void {
    const s = this.cam.state;
    this.cam.flyTo({ tilt: s.tilt > 5 ? 0 : Math.min(50, maxTilt(s.dist)) }, 600);
    this.requestRender();
  }
  get state(): CameraState {
    return this.cam.state;
  }
  flyToItem(id: string): void {
    const p = this.byId.get(id);
    if (!p) return;
    this.cam.flyTo({ lat: p.item.lat, lon: p.item.lon, dist: Math.min(this.cam.state.dist, 0.25) }, 1200);
    this.requestRender();
  }

  private endFollow(): void {
    if (!this.follow) return;
    this.follow = false;
    this.cb.onFollowEnd();
  }

  // ---- input -------------------------------------------------------------------

  private on(t: EventTarget, type: string, fn: EventListener, opts?: AddEventListenerOptions): void {
    t.addEventListener(type, fn, opts);
    this.listeners.push([t, type, fn, opts]);
  }

  private ndc(x: number, y: number): [number, number] {
    return [(x / this.width) * 2 - 1, -(y / this.height) * 2 + 1];
  }

  private local(e: { clientX: number; clientY: number }): [number, number] {
    const r = this.renderer.domElement.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }

  private bindInput(el: HTMLCanvasElement): void {
    el.tabIndex = 0;
    el.setAttribute('aria-label', '3D operating picture. Drag to pan, scroll or pinch to zoom, arrow keys pan, plus and minus zoom, Q and E turn, W and S tilt, R resets.');
    this.on(el, 'contextmenu', (e) => e.preventDefault());
    this.on(el, 'pointerdown', (e) => this.down(e as PointerEvent));
    this.on(el, 'pointermove', (e) => this.move(e as PointerEvent));
    this.on(el, 'pointerup', (e) => this.up(e as PointerEvent));
    this.on(el, 'pointercancel', (e) => this.up(e as PointerEvent, true));
    this.on(el, 'pointerleave', (e) => { if ((e as PointerEvent).pointerType === 'mouse') this.cb.onHover(null); });
    this.on(el, 'wheel', (e) => this.wheel(e as WheelEvent), { passive: false });
    this.on(el, 'dblclick', (e) => {
      const [x, y] = this.local(e as MouseEvent);
      this.flyInAt(x, y);
    });
    this.on(el, 'keydown', (e) => this.key(e as KeyboardEvent));
  }

  private flyInAt(x: number, y: number): void {
    const [nx, ny] = this.ndc(x, y);
    const at = this.cam.surfaceAt(nx, ny);
    if (!at) return;
    this.endFollow();
    this.cam.flyTo({ lat: at.lat, lon: at.lon, dist: this.cam.state.dist * 0.4 }, 900);
    this.requestRender();
  }

  private down(e: PointerEvent): void {
    const el = this.renderer.domElement;
    el.focus({ preventScroll: true });
    try { el.setPointerCapture(e.pointerId); } catch { /* not capturable */ }
    const [x, y] = this.local(e);
    this.pointers.set(e.pointerId, { x, y, x0: x, y0: y, t0: performance.now(), type: e.pointerType });
    this.cam.stop();
    this.samples = [];
    if (this.pointers.size === 1) {
      const turn = e.button === 2 || e.ctrlKey || e.shiftKey || e.metaKey;
      this.mode = turn ? 'turn' : 'pan';
      const [nx, ny] = this.ndc(x, y);
      this.grab = turn ? null : this.cam.surfaceAt(nx, ny);
    } else if (this.pointers.size === 2) {
      this.multi = true;
      this.startTwo();
    }
    this.requestRender();
  }

  private startTwo(): void {
    const [p, q] = [...this.pointers.values()];
    const cx = (p.x + q.x) / 2, cy = (p.y + q.y) / 2;
    this.two = { cx, cy, cx0: cx, cy0: cy, span: Math.hypot(q.x - p.x, q.y - p.y), span0: Math.hypot(q.x - p.x, q.y - p.y), angle: Math.atan2(q.y - p.y, q.x - p.x), moved: [p.x, p.y, q.x, q.y] };
    const [nx, ny] = this.ndc(cx, cy);
    this.grab = this.cam.surfaceAt(nx, ny);
    this.mode = 'two';
  }

  private move(e: PointerEvent): void {
    const [x, y] = this.local(e);
    const p = this.pointers.get(e.pointerId);
    if (!p) {
      // Hover (mouse only), at most once a frame.
      if (e.pointerType === 'mouse' && e.buttons === 0) {
        this.hoverPending = { x, y };
        if (!this.hoverFrame) {
          this.hoverFrame = requestAnimationFrame(() => {
            this.hoverFrame = 0;
            if (this.hoverPending) this.cb.onHover(this.pick(this.hoverPending.x, this.hoverPending.y, 10));
          });
        }
      }
      return;
    }
    const dx = x - p.x, dy = y - p.y;
    p.x = x;
    p.y = y;
    const s = this.cam.state;
    if (this.mode === 'pan' && this.pointers.size === 1) {
      if (Math.hypot(x - p.x0, y - p.y0) < 3) return;
      this.endFollow();
      const [nx, ny] = this.ndc(x, y);
      if (!this.grab || !this.cam.keepUnder(this.grab, nx, ny)) {
        // Off the globe: pan by the drag distance instead.
        this.cam.panScreen(-dx / this.height, -dy / this.height);
        this.grab = this.cam.surfaceAt(nx, ny);
      }
      const now = performance.now();
      this.samples.push({ t: now, lat: s.lat, lon: s.lon });
      while (this.samples.length > 2 && now - this.samples[0].t > 90) this.samples.shift();
    } else if (this.mode === 'turn') {
      s.heading -= dx * 0.3;
      s.tilt -= dy * 0.3;
    } else if (this.pointers.size === 2 && (this.mode === 'two' || this.mode === 'pinch' || this.mode === 'tilt')) {
      this.endFollow();
      this.twoMove();
    }
    this.requestRender();
  }

  private twoMove(): void {
    const [p, q] = [...this.pointers.values()];
    const cx = (p.x + q.x) / 2, cy = (p.y + q.y) / 2;
    const span = Math.max(1, Math.hypot(q.x - p.x, q.y - p.y));
    const angle = Math.atan2(q.y - p.y, q.x - p.x);
    const t = this.two;
    const s = this.cam.state;
    if (this.mode === 'two') {
      // Decide what the gesture is once the fingers have moved a little: both moving
      // vertically together is a tilt; anything else pinches, twists and pans.
      const [px0, py0, qx0, qy0] = t.moved;
      const dpy = p.y - py0, dqy = q.y - qy0, dpx = p.x - px0, dqx = q.x - qx0;
      // Each finger's move arrives as its own event, so wait until both have moved
      // (or one has clearly moved alone, a one-finger pinch).
      const tp = Math.hypot(dpx, dpy), tq = Math.hypot(dqx, dqy);
      if (Math.max(tp, tq) < 10 || (Math.min(tp, tq) < 5 && Math.max(tp, tq) < 30)) return;
      const together = Math.sign(dpy) === Math.sign(dqy) && Math.abs(dpy) > 2 * Math.abs(dpx) && Math.abs(dqy) > 2 * Math.abs(dqx);
      this.mode = together && Math.abs(span - t.span0) < 0.5 * Math.min(Math.abs(dpy), Math.abs(dqy)) ? 'tilt' : 'pinch';
    }
    if (this.mode === 'tilt') {
      s.tilt -= (cy - t.cy) * 0.35;
    } else {
      s.dist *= t.span / span;
      let da = angle - t.angle;
      if (da > Math.PI) da -= 2 * Math.PI;
      if (da < -Math.PI) da += 2 * Math.PI;
      s.heading -= (da * 180) / Math.PI;
      const [nx, ny] = this.ndc(cx, cy);
      if (this.grab) this.cam.keepUnder(this.grab, nx, ny);
      else this.cam.apply();
    }
    t.cx = cx;
    t.cy = cy;
    t.span = span;
    t.angle = angle;
  }

  private up(e: PointerEvent, cancelled = false): void {
    const p = this.pointers.get(e.pointerId);
    if (!p) return;
    this.pointers.delete(e.pointerId);
    const now = performance.now();
    const touch = p.type !== 'mouse';
    const tap = !cancelled && !this.multi && this.pointers.size === 0 &&
      Math.hypot(p.x - p.x0, p.y - p.y0) < (touch ? TAP_PX.touch : TAP_PX.mouse) && now - p.t0 < TAP_MS;
    if (tap && (e.button === 0 || touch)) {
      if (touch && now - this.lastTap.t < DOUBLE_TAP_MS && Math.hypot(p.x - this.lastTap.x, p.y - this.lastTap.y) < 40) {
        this.lastTap.t = 0;
        this.flyInAt(p.x, p.y);
      } else {
        this.lastTap = { t: now, x: p.x, y: p.y };
        const id = this.pick(p.x, p.y, touch ? 28 : 10);
        if (id) this.cb.onSelect(id);
      }
    } else if (this.mode === 'pan' && this.pointers.size === 0 && this.samples.length >= 2) {
      const a = this.samples[0], b = this.samples[this.samples.length - 1];
      const dt = b.t - a.t;
      if (dt > 0 && now - b.t < 60) {
        let dLon = b.lon - a.lon;
        if (dLon > 180) dLon -= 360;
        if (dLon < -180) dLon += 360;
        this.cam.coast((b.lat - a.lat) / dt, dLon / dt);
      }
    }
    if (this.pointers.size === 1) {
      // Back to one finger: pan from where it is now.
      const [rest] = [...this.pointers.values()];
      rest.x0 = rest.x;
      rest.y0 = rest.y;
      this.mode = 'pan';
      const [nx, ny] = this.ndc(rest.x, rest.y);
      this.grab = this.cam.surfaceAt(nx, ny);
      this.samples = [];
    } else if (this.pointers.size === 0) {
      this.mode = 'none';
      this.grab = null;
      this.multi = false;
    }
    this.requestRender();
  }

  private wheel(e: WheelEvent): void {
    e.preventDefault();
    this.cam.stop();
    const [x, y] = this.local(e);
    const [nx, ny] = this.ndc(x, y);
    const grab = this.cam.surfaceAt(nx, ny);
    const lines = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1;
    const f = Math.exp(THREE.MathUtils.clamp(e.deltaY * lines, -400, 400) * 0.0016);
    this.cam.state.dist *= f;
    if (grab && !this.follow) this.cam.keepUnder(grab, nx, ny);
    this.requestRender();
  }

  private key(e: KeyboardEvent): void {
    const s = this.cam.state;
    let used = true;
    switch (e.key) {
      case 'ArrowUp': this.endFollow(); this.cam.panScreen(0, -0.08); break;
      case 'ArrowDown': this.endFollow(); this.cam.panScreen(0, 0.08); break;
      case 'ArrowLeft': this.endFollow(); this.cam.panScreen(-0.08, 0); break;
      case 'ArrowRight': this.endFollow(); this.cam.panScreen(0.08, 0); break;
      case '+': case '=': this.zoomBy(0.7); break;
      case '-': case '_': this.zoomBy(1 / 0.7); break;
      case 'q': case 'Q': s.heading -= 10; break;
      case 'e': case 'E': s.heading += 10; break;
      case 'w': case 'W': s.tilt += 6; break;
      case 's': case 'S': s.tilt -= 6; break;
      case 'r': case 'R': this.home(); break;
      default: used = false;
    }
    if (used) {
      e.preventDefault();
      this.requestRender();
    }
  }

  // ---- teardown ------------------------------------------------------------------

  dispose(): void {
    this.disposed = true;
    cancelAnimationFrame(this.frame);
    cancelAnimationFrame(this.hoverFrame);
    window.clearTimeout(this.pulseTimer);
    this.ro.disconnect();
    for (const [t, type, fn, opts] of this.listeners) t.removeEventListener(type, fn, opts);
    this.scene.traverse((o) => {
      const any = o as THREE.Mesh;
      if (any.geometry) any.geometry.dispose();
      const mat = any.material as THREE.Material | THREE.Material[] | undefined;
      if (Array.isArray(mat)) mat.forEach((m) => m.dispose());
      else mat?.dispose();
      if (o instanceof THREE.InstancedMesh) o.dispose();
    });
    for (const g of this.geometries.values()) g.dispose();
    for (const m of this.materials.values()) m.dispose();
    this.renderer.dispose();
    this.renderer.domElement.remove();
  }
}
