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
 * for a mouse, 28 px for touch, ignoring those behind the earth).
 *
 * Overlays [FR-H-1] [FR-P3-3]: SIGMETs as translucent volumes from their base to their
 * top (on the same height stretch as aircraft); weather warnings, wind farms and road
 * disruptions as fills painted into one texture draped over the UK terrain box, with
 * outlines; the RainViewer radar image draped the same way; the night side darkened
 * from the subsolar point (a shader tweak on the globe and terrain, with a faint warm
 * terminator); the MEASURE line and range rings drawn on the ground. */
import * as THREE from 'three';
import { mergeGeometries } from 'three/examples/jsm/utils/BufferGeometryUtils.js';
import { LineSegments2 } from 'three/examples/jsm/lines/LineSegments2.js';
import { LineSegmentsGeometry } from 'three/examples/jsm/lines/LineSegmentsGeometry.js';
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js';
import type { HeimdallHistoryPoint } from '@midgard/sdk';
import type { TrackShape } from '../trackStyle';
import { BORDER_PATH, LAND_PATH } from '../basemap';
import { LAKES, MOTORWAYS, RAILWAYS, RIVERS, ROADS } from '../hydro';
import { URBAN } from '../places';
import { GLOBE_LAND_110M } from '../globeLand';
import { TERRAIN } from './terrainUK';
import { buildDetail, zoomFor, type Box } from './detailPatch';
import { loadTrafficLines, TRAFFIC_COLOURS, type TrafficLines } from '../traffic';
import { BALTIC_VIEW, GlobeCamera, UK_VIEW, maxTilt, type CameraState } from './GlobeCamera';
import { D2R, EARTH_KM, drawnHeight, enu, raySphere, toVec } from './geo';
import { circle, greatCircle, subsolarPoint, type LatLon } from '../geoMath';

/** An area drawn on the globe: a SIGMET volume, or a fill draped on the ground. */
export interface GlobeArea {
  id: string;
  /** `volume`: extruded from baseM to topM; `drape`: on the ground. */
  mode: 'volume' | 'drape';
  /** Outline, [lat, lon] pairs. */
  ring: [number, number][];
  /** CSS colour (token or literal). */
  colour: string;
  /** Fill opacity (0–1). */
  fill: number;
  baseM?: number;
  topM?: number;
}

/** Camera presets for the controls and the wall display's tour. [FR-P3-7] */
export const PRESETS: Record<'uk' | 'london' | 'scotland' | 'baltic' | 'globe' | 'balkans' | 'mideast' | 'gulf' | 'russia' | 'taiwan' | 'singapore' | 'australia' | 'nz' | 'canada' | 'india' | 'safrica' | 'kaliningrad' | 'moscow' | 'stpete' | 'blacksea' | 'murmansk' | 'fareast', CameraState> = {
  uk: UK_VIEW,
  london: { lat: 51.48, lon: -0.15, dist: 140 / EARTH_KM, heading: 0, tilt: 50 },
  scotland: { lat: 57.2, lon: -4.6, dist: 750 / EARTH_KM, heading: 0, tilt: 42 },
  baltic: BALTIC_VIEW,
  globe: { lat: 42, lon: -8, dist: 3.1, heading: 0, tilt: 0 },
  // Regions the wider feeds cover.
  balkans: { lat: 43.5, lon: 20.5, dist: 1500 / EARTH_KM, heading: 0, tilt: 40 },
  mideast: { lat: 32.5, lon: 36.5, dist: 1500 / EARTH_KM, heading: 0, tilt: 40 },
  gulf: { lat: 26, lon: 52, dist: 1600 / EARTH_KM, heading: 0, tilt: 40 },
  russia: { lat: 56.5, lon: 37, dist: 2400 / EARTH_KM, heading: 0, tilt: 35 },
  taiwan: { lat: 23.8, lon: 120.9, dist: 900 / EARTH_KM, heading: 0, tilt: 45 },
  singapore: { lat: 1.35, lon: 103.82, dist: 180 / EARTH_KM, heading: 0, tilt: 45 },
  australia: { lat: -32, lon: 149, dist: 2600 / EARTH_KM, heading: 0, tilt: 35 },
  nz: { lat: -40.5, lon: 174, dist: 1900 / EARTH_KM, heading: 0, tilt: 35 },
  canada: { lat: 46, lon: -76, dist: 2600 / EARTH_KM, heading: 0, tilt: 35 },
  india: { lat: 23, lon: 78, dist: 3200 / EARTH_KM, heading: 0, tilt: 30 },
  safrica: { lat: -27, lon: 26, dist: 2200 / EARTH_KM, heading: 0, tilt: 35 },
  kaliningrad: { lat: 54.7, lon: 20.5, dist: 420 / EARTH_KM, heading: 0, tilt: 45 },
  moscow: { lat: 55.75, lon: 37.6, dist: 600 / EARTH_KM, heading: 0, tilt: 45 },
  stpete: { lat: 59.9, lon: 30.3, dist: 500 / EARTH_KM, heading: 0, tilt: 45 },
  blacksea: { lat: 45, lon: 35, dist: 1400 / EARTH_KM, heading: 0, tilt: 40 },
  murmansk: { lat: 68.9, lon: 33.1, dist: 900 / EARTH_KM, heading: 0, tilt: 40 },
  fareast: { lat: 47, lon: 135, dist: 2800 / EARTH_KM, heading: 0, tilt: 35 },
};
/** The region menu: label and preset, in menu order. */
export const REGIONS: [string, PresetId][] = [
  ['UK & IRELAND', 'uk'], ['BALTIC', 'baltic'], ['BALKANS', 'balkans'], ['EAST MEDITERRANEAN', 'mideast'], ['GULF', 'gulf'],
  ['WESTERN RUSSIA', 'russia'], ['· MOSCOW', 'moscow'], ['· ST PETERSBURG', 'stpete'], ['· KALININGRAD', 'kaliningrad'], ['· MURMANSK / KOLA', 'murmansk'], ['· BLACK SEA / CRIMEA', 'blacksea'], ['· RUSSIAN FAR EAST', 'fareast'], ['TAIWAN', 'taiwan'], ['SINGAPORE', 'singapore'], ['INDIA', 'india'],
  ['EASTERN AUSTRALIA', 'australia'], ['NEW ZEALAND', 'nz'], ['EASTERN CANADA', 'canada'], ['SOUTH AFRICA', 'safrica'], ['WHOLE GLOBE', 'globe'],
];
export type PresetId = keyof typeof PRESETS;

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
  /** Runways, [lat, lon, lat, lon, width m], drawn on the ground. */
  runways?: [number, number, number, number, number][];
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
  /** A tap on the ground while surface picking is on (the MEASURE tool). */
  onSurface?: (at: LatLon) => void;
  /** Any direct input on the canvas (pauses the wall display's tour). */
  onInteract?: () => void;
  /** The ground point under the mouse as it moves (null off the globe). */
  onPointerGround?: (at: LatLon | null) => void;
}

/** Marker sizes in CSS pixels. */
const PX: Record<TrackShape, number> = {
  aircraft: 17, vessel: 13, train: 10, camera: 9, airport: 10, balloon: 7, satellite: 8, quake: 9, flood: 8, point: 6,
  sigmet: 9, warning: 9, windfarm: 15, platform: 7, road: 8, airspace: 4, fire: 6,
};
const SHAPES: TrackShape[] = ['aircraft', 'vessel', 'train', 'camera', 'airport', 'balloon', 'satellite', 'quake', 'flood', 'point', 'sigmet', 'warning', 'windfarm', 'platform', 'road', 'airspace', 'fire'];
/** Flat marks laid on the ground (unlit, translucent). */
const FLAT = new Set<TrackShape>(['quake', 'flood', 'airport', 'warning', 'windfarm', 'road', 'airspace', 'fire', 'train', 'camera']);
/** Marks placed on the terrain surface rather than at an altitude. */
const SURFACE = new Set<TrackShape>([...FLAT, 'platform']);
/** The draped overlays' texture size over the UK terrain box (about 0.4 km a pixel). */
const DRAPE_W = 2048;
/** Built-up area outlines, [lat, lon] rings (places.ts). */
const URBAN_RINGS: [number, number][][] = URBAN.split('|').map((r) => r.split(' ').map((p) => p.split(',').map(Number) as [number, number]));
/** The height the rain radar floats at: a typical rain-cloud base, metres. */
const RAIN_CLOUD_M = 2500;
export type MapStyle = 'tactical' | 'satellite' | 'dark';
const DRAPE_H = 1792;
const GLOBE_COLOUR = '#1a2636';
/** A screen tap: moved less than this (px) within this long (ms). */
const TAP_PX = { mouse: 5, touch: 12 };
/** Narrowest a runway is drawn, metres, so it shows before you are right on top of it. */
const RUNWAY_MIN_M = 150;
/** How far a trail's ribbon hangs below it: a fifth of its drawn height, at most about
 *  12 km as drawn (in earth radii), so it reads as depth and never reaches the ground. */
const TRAIL_DROP = 12 / 6371;
const TRAIL_DROP_SHARE = 0.2;
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

/** A small aircraft, nose along +y, up along +z, one unit long: fuselage, nose, swept
 *  wings, tailplane and fin, so it reads as an aircraft in 3D and shades under light. */
function aircraftGeometry(): THREE.BufferGeometry {
  const box = (w: number, l: number, h: number, x: number, y: number, z: number, sweep = 0) => {
    const g = new THREE.BoxGeometry(w, l, h);
    if (sweep) {
      // sweep the wing back: shift each vertex aft in proportion to its span
      const pos = g.getAttribute('position');
      for (let i = 0; i < pos.count; i++) pos.setY(i, pos.getY(i) - Math.abs(pos.getX(i)) * sweep);
    }
    g.translate(x, y, z);
    return g;
  };
  const fuselage = new THREE.CylinderGeometry(0.06, 0.05, 0.86, 10);
  fuselage.translate(0, -0.05, 0);
  const nose = new THREE.ConeGeometry(0.06, 0.16, 10);
  nose.translate(0, 0.46, 0);
  const parts = [
    fuselage,
    nose,
    box(1.0, 0.18, 0.025, 0, 0.06, 0, 0.35),
    box(0.4, 0.1, 0.02, 0, -0.4, 0.01, 0.3),
    box(0.02, 0.14, 0.2, 0, -0.41, 0.1),
  ].map((g) => g.toNonIndexed());
  // The cylinder and cone are built along y already; all parts share one frame.
  const merged = mergeGeometries(parts, false)!;
  merged.computeVertexNormals();
  return merged;
}

/** A small ship, bow along +y, up along +z, one unit long: a pointed hull with a
 *  superstructure aft, so it reads as a vessel and shows which way it is heading. */
function vesselGeometry(): THREE.BufferGeometry {
  const outline = new THREE.Shape();
  outline.moveTo(0, 0.5);
  outline.lineTo(0.17, 0.2);
  outline.lineTo(0.17, -0.45);
  outline.lineTo(-0.17, -0.45);
  outline.lineTo(-0.17, 0.2);
  outline.closePath();
  const hull = new THREE.ExtrudeGeometry(outline, { depth: 0.1, bevelEnabled: false });
  const bridge = new THREE.BoxGeometry(0.26, 0.18, 0.14);
  bridge.translate(0, -0.28, 0.17);
  const merged = mergeGeometries([hull.toNonIndexed(), bridge.toNonIndexed()], false)!;
  merged.computeVertexNormals();
  return merged;
}

/** A wind turbine seen from above: three blades about a hub, flat in the xy plane. */
function turbineGeometry(): THREE.BufferGeometry {
  const parts: THREE.BufferGeometry[] = [];
  for (let k = 0; k < 3; k++) {
    const blade = new THREE.Shape();
    blade.moveTo(-0.09, 0.12);
    blade.lineTo(0.0, 1.0);
    blade.lineTo(0.12, 0.12);
    blade.closePath();
    const g = new THREE.ShapeGeometry(blade);
    g.rotateZ((k * 2 * Math.PI) / 3);
    parts.push(g.toNonIndexed());
  }
  parts.push(new THREE.CircleGeometry(0.2, 12).toNonIndexed());
  return mergeGeometries(parts, false)!;
}

/** A ring without its repeated closing point, with long edges split (≤ `stepDeg`) so
 *  it follows the curve of the earth. */
function densify(ring: [number, number][], stepDeg: number): [number, number][] {
  const r = ring.length > 1 && ring[0][0] === ring[ring.length - 1][0] && ring[0][1] === ring[ring.length - 1][1] ? ring.slice(0, -1) : ring;
  const out: [number, number][] = [];
  for (let i = 0; i < r.length; i++) {
    const a = r[i], b = r[(i + 1) % r.length];
    const n = Math.max(1, Math.ceil(Math.max(Math.abs(b[0] - a[0]), Math.abs(b[1] - a[1])) / stepDeg));
    for (let k = 0; k < n; k++) out.push([a[0] + ((b[0] - a[0]) * k) / n, a[1] + ((b[1] - a[1]) * k) / n]);
  }
  return out;
}

/** Day and night: darken the night side of a material from the subsolar direction, with
 *  a smooth twilight and a faint warm terminator. Works on basic and Lambert materials. */
function shadeNight(mat: THREE.Material, sun: { value: THREE.Vector3 }, on: { value: number }): void {
  mat.onBeforeCompile = (sh) => {
    sh.uniforms.uSun = sun;
    sh.uniforms.uNight = on;
    sh.vertexShader = sh.vertexShader
      .replace('#include <common>', '#include <common>\nvarying vec3 vEarthN;')
      .replace('#include <project_vertex>', '#include <project_vertex>\nvEarthN = (modelMatrix * vec4(transformed, 1.0)).xyz;');
    sh.fragmentShader = sh.fragmentShader
      .replace('#include <common>', '#include <common>\nuniform vec3 uSun;\nuniform float uNight;\nvarying vec3 vEarthN;')
      .replace('#include <dithering_fragment>', [
        '#include <dithering_fragment>',
        'float sunDot = dot(normalize(vEarthN), uSun);',
        'float day = smoothstep(-0.12, 0.05, sunDot);',
        'gl_FragColor.rgb *= mix(1.0, mix(0.38, 1.0, day), uNight);',
        'gl_FragColor.rgb += uNight * vec3(0.09, 0.05, 0.015) * (1.0 - smoothstep(0.0, 0.07, abs(sunDot + 0.02)));',
      ].join('\n'));
  };
  mat.needsUpdate = true;
}

/** Line strips (lon, lat) → LineSegments positions on a sphere of radius `r`. */
/** Terrain height in metres at a point (bilinear in the bundled grid; 0 outside it). */
/** The terrain grid in use: the bundled 5 km grid at first, replaced by the 2.5 km grid
 *  (public/imagery/terrain-uk-0025.i16, Terrain Tiles zoom 9) once it has loaded. Both
 *  cover the same box; heights in whole metres, sea 0, rows from the north edge. */
let GRID = (() => {
  const bin = atob(TERRAIN.data);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return { west: TERRAIN.west, north: TERRAIN.north, step: TERRAIN.step, nx: TERRAIN.nx, ny: TERRAIN.ny, data: new Int16Array(bytes.buffer) };
})();
const HIRES = { step: 0.025, nx: 641, ny: 561, file: 'terrain-uk-0025.i16' };
function terrainAt(lat: number, lon: number): number {
  const { west, north, step, nx, ny, data: terrainGrid } = GRID;
  const fx = (lon - west) / step, fy = (north - lat) / step;
  if (fx < 0 || fy < 0 || fx > nx - 1 || fy > ny - 1) return 0;
  const x0 = Math.floor(fx), y0 = Math.floor(fy), x1 = Math.min(nx - 1, x0 + 1), y1 = Math.min(ny - 1, y0 + 1);
  const tx = fx - x0, ty = fy - y0;
  const h = (x: number, y: number) => terrainGrid[y * nx + x];
  return (h(x0, y0) * (1 - tx) + h(x1, y0) * tx) * (1 - ty) + (h(x0, y1) * (1 - tx) + h(x1, y1) * tx) * ty;
}

/** Muted relief colours by height: dark lowland to pale upland, so it stays a backdrop. */
const RELIEF: [number, THREE.Color][] = [
  [0, new THREE.Color('#2a3d30')],
  [150, new THREE.Color('#35482f')],
  [400, new THREE.Color('#4d543c')],
  [800, new THREE.Color('#6e624c')],
  [1200, new THREE.Color('#9a907c')],
];
function reliefColour(h: number, out: THREE.Color): THREE.Color {
  for (let i = 1; i < RELIEF.length; i++) {
    if (h <= RELIEF[i][0]) return out.copy(RELIEF[i - 1][1]).lerp(RELIEF[i][1], (h - RELIEF[i - 1][0]) / (RELIEF[i][0] - RELIEF[i - 1][0]));
  }
  return out.copy(RELIEF[RELIEF.length - 1][1]);
}

function stripsToSegments(strips: [number, number][][], r: number | ((lat: number, lon: number) => number)): Float32Array {
  const rad = typeof r === 'number' ? () => r : r;
  let n = 0;
  for (const s of strips) n += Math.max(0, s.length - 1);
  const out = new Float32Array(n * 6);
  const a = new THREE.Vector3(), b = new THREE.Vector3();
  let k = 0;
  for (const s of strips) {
    for (let i = 0; i + 1 < s.length; i++) {
      toVec(s[i][1], s[i][0], rad(s[i][1], s[i][0]), a);
      toVec(s[i + 1][1], s[i + 1][0], rad(s[i + 1][1], s[i + 1][0]), b);
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
  private readonly materials = new Map<TrackShape, THREE.MeshBasicMaterial | THREE.MeshLambertMaterial>();
  private placed: Placed[] = [];
  private byId = new Map<string, Placed>();
  private items: GlobeItem[] = [];
  private trails: GlobeTrail[] = [];
  private path: HeimdallHistoryPoint[] | null = null;
  private pathColour = 'var(--accent-hover)';
  private selected: string | null = null;
  private exaggeration = 4;
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
  /** A translucent wall from the selected track's recorded path down to the ground. */
  private readonly pathCurtain: THREE.Mesh;
  private readonly terrain: THREE.Mesh;
  private readonly globeMaterial: THREE.MeshBasicMaterial;
  /** Textured map (tactical relief or satellite imagery) on the globe and terrain, or the
   *  plain dark map. */
  private imagery = false;
  private mapStyle: MapStyle = 'dark';
  private textures: Partial<Record<string, THREE.Texture>> = {};
  /** Airport runway centre lines, on the ground. */
  private readonly runwayLines: THREE.LineSegments;
  /** Runways as strips of their real width (at least RUNWAY_MIN_M), on the ground. */
  private readonly runwayStrips: THREE.Mesh;
  private readonly drapedStrips: [number, number][][][];
  private readonly fatLines: { obj: LineSegments2; strips: [number, number][][] }[] = [];
  private readonly draped: THREE.LineSegments[];
  /** Short ribbons hanging a little below every trail, so trails read in 3D. */
  private readonly trailRibbons: THREE.Mesh;
  private readonly pathLine: THREE.Line;
  /** The path of the track under the pointer, with a lighter curtain. */
  private readonly hoverLine: THREE.Line;
  private readonly hoverCurtain: THREE.Mesh;
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
  // overlays [FR-H-1] [FR-P3-3]
  private areas: GlobeArea[] = [];
  /** SIGMET volumes (walls and caps). */
  private readonly areaFill: THREE.Mesh;
  /** Every area outline, coloured per area. */
  private readonly areaLines: THREE.LineSegments;
  /** The UK terrain box at ground height (all cells), for draped textures. */
  private readonly drapeGeometry = new THREE.BufferGeometry();
  private readonly areaDrape: THREE.Mesh;
  private readonly drapeCanvas: HTMLCanvasElement;
  private readonly drapeTexture: THREE.CanvasTexture;
  private readonly radarMesh: THREE.Mesh;
  /** Live traffic as crisp lines: the UK overview, or finer lines for the area in view. */
  private trafficOverview: TrafficLines | null = null;
  private trafficClose: { key: string; lines: TrafficLines } | null = null;
  private trafficWant = '';
  private trafficAbort: AbortController | null = null;
  private trafficTimer = 0;
  private readonly trafficFat: { cls: keyof TrafficLines | 'casing'; obj: LineSegments2 }[] = [];
  /** Close-up detail laid on the terrain for the area in view (see detailPatch.ts). */
  private detail: { mesh: THREE.Mesh; key: string } | null = null;
  private detailWant = '';
  private detailTimer = 0;
  private detailAbort: AbortController | null = null;
  /** Live traffic is on: the close-up detail draws it at full resolution. */
  private trafficOn = false;
  private radarTexture: THREE.Texture | null = null;
  private readonly sun = { value: new THREE.Vector3(1, 0, 0) };
  private readonly night = { value: 1 };
  private readonly measureLine: THREE.LineSegments;
  private measurePts: LatLon[] = [];
  private rings: { centre: LatLon; nms: number[] } | null = null;
  private markerScale = 1;
  private pickSurface = false;

  constructor(private readonly host: HTMLElement, private readonly cb: GlobeCallbacks) {
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: 'high-performance' });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.setClearColor(0x000000, 0);
    const canvas = this.renderer.domElement;
    canvas.style.cssText = 'display:block;width:100%;height:100%;touch-action:none;outline:none;';
    host.appendChild(canvas);
    this.cam = new GlobeCamera(this.camera);

    // Earth, outlines, graticule, atmosphere.
    this.globeMaterial = new THREE.MeshBasicMaterial({ color: GLOBE_COLOUR, polygonOffset: true, polygonOffsetFactor: 1, polygonOffsetUnits: 1 });
    const globe = new THREE.Mesh(new THREE.SphereGeometry(1, 128, 96), this.globeMaterial);
    // three's sphere starts its texture at longitude -90° in this scene's frame; turn it
    // so the equirectangular image's left edge sits on the antimeridian.
    globe.rotation.y = -Math.PI / 2;
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
    lines(stripsToSegments(world, 1.0006), 'var(--text-muted)', 0.6);
    // Europe's coast and borders at 1:50m, draped over the terrain (rebuilt when the
    // height stretch changes).
    // Rivers and lakes (Natural Earth 1:10m) are draped like the coast, in water blue.
    const hydro = (text: string) => text.split('|').map((ln) => ln.split(' ').map((p) => { const [la, lo] = p.split(',').map(Number); return [lo, la] as [number, number]; }));
    this.drapedStrips = [svgRings(LAND_PATH), svgRings(BORDER_PATH)];
    this.draped = [lines(new Float32Array(0), 'var(--text-muted)', 0.6), lines(new Float32Array(0), 'var(--text-faint)', 0.4)];
    // Roads and rivers as screen-space lines of a fixed pixel width, draped on the terrain:
    // sharp at any zoom (a painted texture blurs when magnified).
    // Styled like a British road atlas: motorways blue and primary roads green, each over a
    // dark casing; railways a black line with white dashes. Casings draw first.
    const fat = (strips: [number, number][][], colour: string, px: number, opacity: number, order: number, dash?: number) => {
      const material = new LineMaterial({ color: new THREE.Color(colour).getHex(), linewidth: px, transparent: true, opacity, depthWrite: false });
      if (dash) {
        material.dashed = true;
        material.dashSize = dash;
        material.gapSize = dash;
      }
      const obj = new LineSegments2(new LineSegmentsGeometry(), material);
      obj.frustumCulled = false;
      obj.renderOrder = order;
      this.scene.add(obj);
      this.fatLines.push({ obj, strips });
    };
    const roads = hydro(ROADS), motorways = hydro(MOTORWAYS), rail = hydro(RAILWAYS);
    // Built-up areas: a fine outline here, a light fill painted into the drape (buildAreas).
    fat(URBAN_RINGS.map((r) => r.map(([la, lo]) => [lo, la] as [number, number])), '#d8b878', 1.0, 0.55, 2);
    fat(hydro(LAKES), '#6eb9f5', 1.4, 0.9, 2);
    fat(hydro(RIVERS), '#6eb9f5', 1.8, 0.95, 2);
    fat(roads, '#0f1a12', 3.4, 0.8, 3);
    fat(roads, '#3fb35c', 1.8, 0.95, 4);
    fat(motorways, '#0b1630', 4.8, 0.85, 5);
    fat(motorways, '#3b82e0', 3.0, 1, 6);
    fat(rail, '#111111', 3.2, 0.9, 7);
    // white dashes about 1.5 km long (scene units are earth radii)
    fat(rail, '#f2f2f2', 1.6, 0.95, 8, 1.5 / 6371);

    // Shaded relief for the UK and Ireland, lit by the same lights as the markers.
    this.terrain = new THREE.Mesh(
      new THREE.BufferGeometry(),
      new THREE.MeshLambertMaterial({ vertexColors: true, polygonOffset: true, polygonOffsetFactor: 1, polygonOffsetUnits: 1 }),
    );
    this.scene.add(this.terrain);
    this.runwayLines = lines(new Float32Array(0), 'var(--grey-100)', 0.85);
    this.runwayStrips = new THREE.Mesh(
      new THREE.BufferGeometry(),
      new THREE.MeshBasicMaterial({ color: this.colour('var(--grey-100)'), transparent: true, opacity: 0.85, side: THREE.DoubleSide, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2 }),
    );
    this.runwayStrips.renderOrder = 1;
    this.scene.add(this.runwayStrips);

    // Draped overlays over the terrain box: area fills (painted into a canvas) and radar.
    this.drapeCanvas = document.createElement('canvas');
    this.drapeCanvas.width = DRAPE_W;
    this.drapeCanvas.height = DRAPE_H;
    this.drapeTexture = new THREE.CanvasTexture(this.drapeCanvas);
    this.drapeTexture.colorSpace = THREE.SRGBColorSpace;
    const drapeMat = (map: THREE.Texture | null, units: number) => new THREE.MeshBasicMaterial({
      map, transparent: true, depthWrite: false, side: THREE.DoubleSide, polygonOffset: true, polygonOffsetFactor: -1, polygonOffsetUnits: units,
    });
    this.areaDrape = new THREE.Mesh(this.drapeGeometry, drapeMat(this.drapeTexture, -3));
    this.areaDrape.renderOrder = 1;
    this.areaDrape.visible = false;
    // Rain is weather in the air: the radar floats as a translucent sheet at cloud height
    // (RAIN_CLOUD_M, stretched like everything else) instead of being painted on the ground.
    this.radarMesh = new THREE.Mesh(new THREE.BufferGeometry(), new THREE.MeshBasicMaterial({ transparent: true, depthWrite: false, side: THREE.DoubleSide }));
    this.buildRadarSheet();
    (this.radarMesh.material as THREE.MeshBasicMaterial).opacity = 0.75;
    this.radarMesh.renderOrder = 1;
    this.radarMesh.visible = false;
    this.scene.add(this.radarMesh, this.areaDrape);
    // Traffic lines over the roads: a dark casing, then the class colours (heavy on top).
    for (const [cls, colour, px, order] of [['casing', '#06090c', 4.2, 9], ['light', TRAFFIC_COLOURS.light, 2.2, 10], ['moderate', TRAFFIC_COLOURS.moderate, 2.6, 11], ['closed', TRAFFIC_COLOURS.closed, 2.8, 12], ['heavy', TRAFFIC_COLOURS.heavy, 3.0, 13]] as const) {
      const obj = new LineSegments2(new LineSegmentsGeometry(), new LineMaterial({ color: new THREE.Color(colour).getHex(), linewidth: px, transparent: true, opacity: cls === 'casing' ? 0.7 : 1, depthWrite: false }));
      obj.frustumCulled = false;
      obj.renderOrder = order;
      obj.visible = false;
      this.scene.add(obj);
      this.trafficFat.push({ cls, obj });
    }
    this.buildTerrain();
    this.loadHiresTerrain();
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

    // Light for the solid markers: soft all round, plus a key light that travels with
    // the camera (from above its left shoulder), so shading reads the same from any angle.
    this.scene.add(new THREE.AmbientLight(0xffffff, 1.1));
    const key = new THREE.DirectionalLight(0xffffff, 2.2);
    key.position.set(-0.6, 1, 0.4);
    this.camera.add(key);
    this.scene.add(this.camera);

    // Entity markers, one instanced mesh per shape (grown on demand).
    this.geometries.set('aircraft', aircraftGeometry());
    this.geometries.set('vessel', vesselGeometry());
    // A train: a flat badge lying on the ground (the feed gives a station, not a heading).
    const badge = new THREE.Shape();
    const bw = 0.5, bh = 0.32, br = 0.12;
    badge.moveTo(-bw + br, -bh);
    badge.lineTo(bw - br, -bh); badge.quadraticCurveTo(bw, -bh, bw, -bh + br);
    badge.lineTo(bw, bh - br); badge.quadraticCurveTo(bw, bh, bw - br, bh);
    badge.lineTo(-bw + br, bh); badge.quadraticCurveTo(-bw, bh, -bw, bh - br);
    badge.lineTo(-bw, -bh + br); badge.quadraticCurveTo(-bw, -bh, -bw + br, -bh);
    this.geometries.set('train', new THREE.ShapeGeometry(badge));
    // A camera: a small body with a lens cone, flat on the ground, facing north.
    const cam = new THREE.Shape();
    cam.moveTo(-0.45, -0.3); cam.lineTo(0.2, -0.3); cam.lineTo(0.2, -0.12); cam.lineTo(0.5, -0.3);
    cam.lineTo(0.5, 0.3); cam.lineTo(0.2, 0.12); cam.lineTo(0.2, 0.3); cam.lineTo(-0.45, 0.3); cam.closePath();
    this.geometries.set('camera', new THREE.ShapeGeometry(cam));
    this.geometries.set('balloon', new THREE.SphereGeometry(0.5, 12, 8));
    this.geometries.set('satellite', new THREE.OctahedronGeometry(0.5));
    this.geometries.set('quake', new THREE.RingGeometry(0.72, 1, 40));
    this.geometries.set('flood', new THREE.CircleGeometry(1, 28));
    this.geometries.set('airport', new THREE.RingGeometry(0.55, 1, 4, 1, Math.PI / 4));
    this.geometries.set('point', new THREE.SphereGeometry(0.5, 8, 6));
    this.geometries.set('sigmet', new THREE.OctahedronGeometry(0.5));
    this.geometries.set('warning', new THREE.CircleGeometry(1, 3));
    this.geometries.set('windfarm', turbineGeometry());
    this.geometries.set('platform', new THREE.BoxGeometry(0.7, 0.7, 0.9).translate(0, 0, 0.45));
    this.geometries.set('road', new THREE.CircleGeometry(1, 4));
    this.geometries.set('airspace', new THREE.RingGeometry(0.6, 1, 16));
    this.geometries.set('fire', new THREE.CircleGeometry(1, 16));
    for (const s of SHAPES) {
      const flat = FLAT.has(s);
      // Solid markers are lit so they shade as 3D objects; flat surface markers are not.
      const Mat = flat ? THREE.MeshBasicMaterial : THREE.MeshLambertMaterial;
      this.materials.set(s, new Mat({
        color: 0xffffff,
        transparent: flat,
        opacity: s === 'flood' ? 0.6 : s === 'airspace' ? 0.5 : s === 'quake' || s === 'warning' || s === 'road' || s === 'fire' ? 0.9 : 1,
        side: flat ? THREE.DoubleSide : THREE.FrontSide,
        depthWrite: !flat,
      }));
      this.materials.get(s)!.userData.base = this.materials.get(s)!.opacity;
    }

    const vcLines = () => {
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(0), 3));
      g.setAttribute('color', new THREE.BufferAttribute(new Float32Array(0), 3));
      return g;
    };
    this.drops = new THREE.LineSegments(vcLines(), new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, opacity: 0.45, depthWrite: false }));
    this.trailLines = new THREE.LineSegments(vcLines(), new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, opacity: 0.16, depthWrite: false }));
    this.pathLine = new THREE.Line(vcLines(), new THREE.LineBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.95, depthWrite: false }));
    const curtain = (opacity: number) => {
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(0), 3));
      g.setAttribute('color', new THREE.BufferAttribute(new Float32Array(0), 4));
      const mesh = new THREE.Mesh(g, new THREE.MeshBasicMaterial({ vertexColors: true, transparent: true, opacity, side: THREE.DoubleSide, depthWrite: false }));
      mesh.renderOrder = 2;
      return mesh;
    };
    this.pathCurtain = curtain(1);
    this.trailRibbons = curtain(1);
    this.hoverCurtain = curtain(1);
    this.hoverLine = new THREE.Line(vcLines(), new THREE.LineBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.9, depthWrite: false }));
    this.marker = new THREE.Mesh(new THREE.RingGeometry(0.78, 1, 40), new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.95, depthTest: false, side: THREE.DoubleSide }));
    this.marker.renderOrder = 10;
    this.marker.visible = false;
    this.scene.add(this.trailRibbons, this.hoverCurtain, this.hoverLine, this.pathCurtain, this.drops, this.trailLines, this.pathLine, this.marker);

    // SIGMET volumes, area outlines, the measured line and range rings.
    this.areaFill = curtain(1);
    this.areaFill.renderOrder = 3;
    this.areaLines = new THREE.LineSegments(vcLines(), new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, opacity: 0.85, depthWrite: false }));
    this.measureLine = new THREE.LineSegments(vcLines(), new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, opacity: 0.95, depthWrite: false, depthTest: false }));
    this.measureLine.renderOrder = 9;
    this.scene.add(this.areaFill, this.areaLines, this.measureLine);

    // Night side shading on the globe and the terrain.
    shadeNight(this.globeMaterial, this.sun, this.night);
    shadeNight(this.terrain.material as THREE.Material, this.sun, this.night);
    this.setSunTime(Date.now());

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
    this.buildDrops();
    if (this.follow) this.followSelected();
    this.requestRender();
  }

  setHoverPath(path: HeimdallHistoryPoint[] | null, colour?: string): void {
    const pos: number[] = [], col: number[] = [];
    const v = new THREE.Vector3();
    const c = this.colour(colour ?? 'var(--grey-100)');
    for (const pt of path ?? []) {
      this.pointAt(pt, v);
      pos.push(v.x, v.y, v.z);
      col.push(c.r, c.g, c.b);
    }
    this.setLines(this.hoverLine, pos, col);
    (this.hoverLine.material as THREE.LineBasicMaterial).color.copy(c);
    this.hoverLine.visible = pos.length >= 6;
    this.setCurtain(this.hoverCurtain, path && path.length > 1 ? [{ points: path, colour: c, alpha: 0.25, fade: false }] : []);
    this.requestRender();
  }

  /** Long trails: markers step back (dimmer) and trails come forward, so the paths lead. */
  setPathsFirst(on: boolean): void {
    for (const s of SHAPES) {
      const m = this.materials.get(s)!;
      const base = (m.userData.base as number | undefined) ?? 1;
      m.transparent = on || FLAT.has(s);
      m.opacity = on ? base * 0.35 : base;
      m.needsUpdate = true;
    }
    (this.trailLines.material as THREE.LineBasicMaterial).opacity = on ? 0.55 : 0.16;
    this.requestRender();
  }

  /** The map style: TACTICAL (monochrome hillshade relief with contours, a dark blue-grey
   *  globe), SATELLITE (NASA Blue Marble, hillshaded) or DARK (plain relief colours). The
   *  images are bundled with the console (public/imagery), so all three work offline;
   *  each loads the first time its style is chosen. [FR-P3-3] */
  setMapStyle(style: MapStyle): void {
    if (style === this.mapStyle) return;
    this.mapStyle = style;
    const on = style !== 'dark';
    this.imagery = on;
    const base = (import.meta as { env?: { BASE_URL?: string } }).env?.BASE_URL ?? '/';
    const tex = (name: string) => {
      if (!this.textures[name]) {
        const t = new THREE.TextureLoader().load(`${base}imagery/${name}`, () => this.requestRender());
        t.colorSpace = THREE.SRGBColorSpace;
        t.anisotropy = Math.min(8, this.renderer.capabilities.getMaxAnisotropy());
        this.textures[name] = t;
      }
      return this.textures[name]!;
    };
    const tactical = style === 'tactical';
    this.globeMaterial.map = on ? tex(tactical ? 'earth-tactical.jpg' : 'earth-4096.jpg') : null;
    this.globeMaterial.color.set(on ? (tactical ? 0xffffff : 0xf2f2f2) : GLOBE_COLOUR);
    this.globeMaterial.needsUpdate = true;
    const tm = this.terrain.material as THREE.MeshLambertMaterial;
    tm.map = on ? tex(tactical ? 'uk-ie-tactical.jpg' : 'uk-ie-2560.jpg') : null;
    tm.vertexColors = !on;
    // The relief is already shaded in the image; the scene light only adds a little.
    tm.color.set(on ? (tactical ? 0xffffff : 0xb4b4b4) : 0xffffff);
    // Roads and rails step back a little on the tactical map, so tracks lead.
    for (const f of this.fatLines) {
      const m = f.obj.material as LineMaterial;
      m.opacity = (m.userData.baseOpacity ??= m.opacity) * (tactical ? 0.75 : 1);
    }
    tm.needsUpdate = true;
    this.buildTerrain();
    this.requestRender();
  }

  /** The radar's sheet: a grid over the terrain box at cloud height, mapped like the box. */
  private buildRadarSheet(): void {
    const { west, east, south, north } = TERRAIN;
    const n = 96;
    const pos = new Float32Array((n + 1) * (n + 1) * 3), uv = new Float32Array((n + 1) * (n + 1) * 2);
    const r = 1 + drawnHeight(RAIN_CLOUD_M, this.exaggeration);
    const v = new THREE.Vector3();
    for (let j = 0; j <= n; j++) {
      for (let i = 0; i <= n; i++) {
        toVec(north - (j / n) * (north - south), west + (i / n) * (east - west), r, v);
        const k = j * (n + 1) + i;
        pos[k * 3] = v.x; pos[k * 3 + 1] = v.y; pos[k * 3 + 2] = v.z;
        uv[k * 2] = i / n; uv[k * 2 + 1] = 1 - j / n;
      }
    }
    const index: number[] = [];
    for (let j = 0; j < n; j++) for (let i = 0; i < n; i++) { const a = j * (n + 1) + i; index.push(a, a + n + 1, a + 1, a + 1, a + n + 1, a + n + 2); }
    const g = this.radarMesh.geometry;
    g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    g.setAttribute('uv', new THREE.BufferAttribute(uv, 2));
    g.setIndex(index);
    g.computeBoundingSphere();
  }

  setExaggeration(x: number): void {
    if (x === this.exaggeration) return;
    this.exaggeration = x;
    this.buildRadarSheet();
    this.buildTrafficLines();
    this.buildTerrain();
    this.place();
    this.buildTrails();
    this.buildPath();
    this.buildAreas();
    this.buildMeasure();
    this.requestRender();
  }

  // ---- overlays [FR-H-1] [FR-P3-3] ------------------------------------------------

  /** Areas: SIGMET volumes, and fills draped on the ground. */
  setAreas(areas: GlobeArea[]): void {
    this.areas = areas;
    this.buildAreas();
    this.requestRender();
  }

  /** Live traffic for the UK overview, or null to hide traffic. Close in, finer lines
   *  for the area in view replace it (updateTraffic). While it shows, the road lines fade
   *  so the traffic colours lead. [FR-H-1] */
  setTraffic(lines: TrafficLines | null): void {
    this.trafficOverview = lines;
    this.trafficOn = !!lines;
    if (!lines) {
      this.trafficAbort?.abort();
      this.trafficClose = null;
      this.trafficWant = '';
    }
    for (const f of this.fatLines.slice(2, 6)) {
      const lm = f.obj.material as LineMaterial;
      lm.userData.baseOpacity ??= lm.opacity;
      lm.opacity = lm.userData.baseOpacity * (lines ? 0.3 : 1);
    }
    this.buildTrafficLines();
    this.scheduleDetail();
  }

  private buildTrafficLines(): void {
    const lines = this.trafficOn ? (this.trafficClose?.lines ?? this.trafficOverview) : null;
    const drape = (lat: number, lon: number) => 1 + drawnHeight(Math.max(0, terrainAt(lat, lon)), this.exaggeration) + 0.00035;
    const strips = (ls: [number, number][][]) => ls.map((l) => l.map(([la, lo]) => [lo, la] as [number, number]));
    for (const f of this.trafficFat) {
      f.obj.visible = !!lines;
      if (!lines) continue;
      const src = f.cls === 'casing' ? [...lines.light, ...lines.moderate, ...lines.heavy, ...lines.closed] : lines[f.cls];
      // A fresh geometry each time: an instanced geometry keeps the segment count it was
      // first drawn with, so a longer list would be cut short.
      f.obj.geometry.dispose();
      const g = new LineSegmentsGeometry();
      g.setPositions(stripsToSegments(strips(src), drape));
      f.obj.geometry = g;
    }
    this.requestRender();
  }

  /** Close in with traffic on: finer flow tiles for the area in view (refreshed every 2
   *  minutes while it stays in view); zoomed out, back to the overview. */
  private updateTraffic(box: Box | null): void {
    window.clearTimeout(this.trafficTimer);
    if (!this.trafficOn || !box) {
      this.trafficAbort?.abort();
      this.trafficWant = '';
      if (this.trafficClose) { this.trafficClose = null; this.buildTrafficLines(); }
      return;
    }
    const z = Math.max(9, Math.min(13, zoomFor(box, 1600)));
    const key = `${box.west.toFixed(3)}:${box.south.toFixed(3)}:${z}`;
    if (key === this.trafficWant) return;
    this.trafficWant = key;
    this.trafficAbort?.abort();
    const ctrl = new AbortController();
    this.trafficAbort = ctrl;
    const read = () => loadTrafficLines(box, z, ctrl.signal).then((lines) => {
      if (ctrl.signal.aborted || this.trafficWant !== key) return;
      this.trafficClose = { key, lines };
      this.buildTrafficLines();
      this.trafficTimer = window.setTimeout(() => { if (this.trafficWant === key) read().catch(() => undefined); }, 120_000);
    });
    read().catch(() => { if (this.trafficWant === key) this.trafficWant = ''; });
  }

  /** The radar image over the terrain box (RADAR_BOX), or null to hide it. */
  setRadar(canvas: HTMLCanvasElement | null): void {
    this.radarTexture?.dispose();
    this.radarTexture = null;
    const m = this.radarMesh.material as THREE.MeshBasicMaterial;
    if (canvas) {
      this.radarTexture = new THREE.CanvasTexture(canvas);
      this.radarTexture.colorSpace = THREE.SRGBColorSpace;
    }
    m.map = this.radarTexture;
    m.needsUpdate = true;
    this.radarMesh.visible = !!canvas;
    this.requestRender();
  }

  /** Shade the night side for this moment (unix ms): live time, or the replay time. */
  setSunTime(ms: number): void {
    const s = subsolarPoint(ms);
    toVec(s.lat, s.lon, 1, this.sun.value).normalize();
    this.requestRender();
  }

  setNightShading(on: boolean): void {
    this.night.value = on ? 1 : 0;
    this.requestRender();
  }

  /** The MEASURE tool's points (0, 1 or 2) and the range rings round a point. */
  setMeasure(points: LatLon[], rings: { centre: LatLon; nms: number[] } | null): void {
    this.measurePts = points;
    this.rings = rings;
    this.buildMeasure();
    this.requestRender();
  }

  /** While on, a tap reports the ground point (onSurface) instead of selecting. */
  setPickSurface(on: boolean): void {
    this.pickSurface = on;
  }

  /** Markers drawn larger (the wall display). */
  setMarkerScale(k: number): void {
    this.markerScale = k;
    this.requestRender();
  }

  flyToPreset(id: PresetId, ms = 2200): void {
    this.endFollow();
    this.cam.flyTo({ ...PRESETS[id] }, ms);
    this.requestRender();
  }

  /** Fly to a point (an alert with no track in the picture). */
  flyToPoint(lat: number, lon: number): void {
    this.endFollow();
    this.cam.flyTo({ lat, lon, dist: Math.min(this.cam.state.dist, 0.12) }, 1200);
    this.requestRender();
  }

  /** CSS-pixel position of a ground point, or null when hidden. */
  screenOfPoint(lat: number, lon: number): { x: number; y: number } | null {
    const p = toVec(lat, lon, 1 + drawnHeight(Math.max(0, terrainAt(lat, lon)), this.exaggeration) + 0.0008);
    if (this.occluded(p)) return null;
    const v = p.project(this.camera);
    if (v.z > 1 || v.x < -1.1 || v.x > 1.1 || v.y < -1.1 || v.y > 1.1) return null;
    return { x: ((v.x + 1) / 2) * this.width, y: ((1 - v.y) / 2) * this.height };
  }

  private buildAreas(): void {
    const fillPos: number[] = [], fillCol: number[] = [];
    const linePos: number[] = [], lineCol: number[] = [];
    const ctx = this.drapeCanvas.getContext('2d')!;
    ctx.clearRect(0, 0, DRAPE_W, DRAPE_H);
    const { west, east, south, north } = TERRAIN;
    // Built-up areas, lightly tinted, under every other fill.
    ctx.fillStyle = 'rgba(216, 184, 120, 0.2)';
    for (const ring of URBAN_RINGS) {
      ctx.beginPath();
      ring.forEach(([la, lo], i) => {
        const x = ((lo - west) / (east - west)) * DRAPE_W, y = ((north - la) / (north - south)) * DRAPE_H;
        if (i) ctx.lineTo(x, y); else ctx.moveTo(x, y);
      });
      ctx.closePath();
      ctx.fill();
    }
    let draped = 1;
    const drape = (lat: number, lon: number) => 1 + drawnHeight(Math.max(0, terrainAt(lat, lon)), this.exaggeration) + 0.0007;
    const a = new THREE.Vector3(), b = new THREE.Vector3(), a2 = new THREE.Vector3(), b2 = new THREE.Vector3();
    const line = (p: THREE.Vector3, q: THREE.Vector3, c: THREE.Color, k = 1) => {
      linePos.push(p.x, p.y, p.z, q.x, q.y, q.z);
      lineCol.push(c.r * k, c.g * k, c.b * k, c.r * k, c.g * k, c.b * k);
    };
    for (const ar of this.areas) {
      const c = this.colour(ar.colour);
      const ring = densify(ar.ring, ar.mode === 'volume' ? 0.25 : 0.05);
      if (ring.length < 3) continue;
      if (ar.mode === 'volume') {
        const rb = 1 + drawnHeight(ar.baseM ?? 0, this.exaggeration) + 0.0004;
        const rt = 1 + drawnHeight(ar.topM ?? ar.baseM ?? 3000, this.exaggeration) + 0.0004;
        const tri = (p: THREE.Vector3, q: THREE.Vector3, r: THREE.Vector3, al: number) => {
          fillPos.push(p.x, p.y, p.z, q.x, q.y, q.z, r.x, r.y, r.z);
          for (let k = 0; k < 3; k++) fillCol.push(c.r, c.g, c.b, al);
        };
        // walls, with the outline at the top, the base, and down each corner
        for (let i = 0; i < ring.length; i++) {
          const p = ring[i], q = ring[(i + 1) % ring.length];
          toVec(p[0], p[1], rb, a); toVec(q[0], q[1], rb, b); toVec(p[0], p[1], rt, a2); toVec(q[0], q[1], rt, b2);
          tri(a, b, b2, ar.fill); tri(a, b2, a2, ar.fill);
          line(a2, b2, c);
          line(a, b, c, 0.6);
        }
        for (const v of ar.ring) { toVec(v[0], v[1], rb, a); toVec(v[0], v[1], rt, a2); line(a, a2, c, 0.5); }
        // caps
        const contour = ring.map(([la, lo]) => new THREE.Vector2(lo, la));
        const faces = THREE.ShapeUtils.triangulateShape(contour, []);
        for (const r of [rt, rb]) {
          const al = r === rt ? ar.fill * 1.2 : ar.fill * 0.5;
          for (const [i, j, k] of faces) {
            tri(toVec(ring[i][0], ring[i][1], r, new THREE.Vector3()), toVec(ring[j][0], ring[j][1], r, new THREE.Vector3()), toVec(ring[k][0], ring[k][1], r, new THREE.Vector3()), al);
          }
        }
      } else {
        for (let i = 0; i < ring.length; i++) {
          const p = ring[i], q = ring[(i + 1) % ring.length];
          line(toVec(p[0], p[1], drape(p[0], p[1]), a), toVec(q[0], q[1], drape(q[0], q[1]), b), c);
        }
        // the fill, painted into the draped texture where it falls in the terrain box
        if (ar.fill > 0 && ar.ring.every(([la, lo]) => la >= south && la <= north && lo >= west && lo <= east)) {
          ctx.beginPath();
          ar.ring.forEach(([la, lo], i) => {
            const x = ((lo - west) / (east - west)) * DRAPE_W, y = ((north - la) / (north - south)) * DRAPE_H;
            if (i) ctx.lineTo(x, y); else ctx.moveTo(x, y);
          });
          ctx.closePath();
          ctx.fillStyle = `rgba(${Math.round(c.r * 255)}, ${Math.round(c.g * 255)}, ${Math.round(c.b * 255)}, ${ar.fill})`;
          ctx.fill();
          draped++;
        }
      }
    }
    this.drapeTexture.needsUpdate = true;
    this.areaDrape.visible = draped > 0;
    const g = this.areaFill.geometry;
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(fillPos), 3));
    g.setAttribute('color', new THREE.BufferAttribute(new Float32Array(fillCol), 4));
    g.computeBoundingSphere();
    this.areaFill.visible = fillPos.length > 0;
    this.setLines(this.areaLines, linePos, lineCol);
  }

  private buildMeasure(): void {
    const pos: number[] = [], col: number[] = [];
    const drape = (p: LatLon) => 1 + drawnHeight(Math.max(0, terrainAt(p.lat, p.lon)), this.exaggeration) + 0.0009;
    const a = new THREE.Vector3(), b = new THREE.Vector3();
    const strip = (pts: LatLon[], c: THREE.Color) => {
      for (let i = 0; i + 1 < pts.length; i++) {
        toVec(pts[i].lat, pts[i].lon, drape(pts[i]), a);
        toVec(pts[i + 1].lat, pts[i + 1].lon, drape(pts[i + 1]), b);
        pos.push(a.x, a.y, a.z, b.x, b.y, b.z);
        col.push(c.r, c.g, c.b, c.r, c.g, c.b);
      }
    };
    const white = this.colour('var(--grey-100)');
    if (this.measurePts.length === 2) strip(greatCircle(this.measurePts[0], this.measurePts[1], 96), white);
    // a small cross at each measured point
    for (const p of this.measurePts) {
      for (const brg of [0, 90]) strip([circle(p, 1.2, 4)[brg === 0 ? 0 : 1], circle(p, 1.2, 4)[brg === 0 ? 2 : 3]], white);
    }
    if (this.rings) {
      const c = this.colour('var(--accent-hover)');
      for (const nm of this.rings.nms) strip(circle(this.rings.centre, nm, 120), c);
    }
    this.setLines(this.measureLine, pos, col);
    this.measureLine.visible = pos.length > 0;
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
      // Surface markers sit on the terrain, not under it.
      const surface = SURFACE.has(it.shape);
      // Surface markers sit just above the draped roads and rivers (about 2 km as drawn),
      // close enough to read as on the ground at any tilt.
      const r = 1 + (surface ? drawnHeight(Math.max(0, terrainAt(it.lat, it.lon)), this.exaggeration) + 0.0003 : drawnHeight(it.altM, this.exaggeration) + 0.0004);
      const pos = toVec(it.lat, it.lon, r);
      const up = new THREE.Vector3();
      enu(pos, east, north, up);
      let a: THREE.Vector3, b: THREE.Vector3;
      if (it.shape === 'aircraft' || it.shape === 'vessel') {
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
    this.buildRunways();
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
    if (FLAT.has(shape)) mesh.renderOrder = 1;
    this.meshes.set(shape, mesh);
    this.scene.add(mesh);
  }

  private setLines(obj: THREE.LineSegments | THREE.Line, pos: number[], col: number[]): void {
    const g = obj.geometry;
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(pos), 3));
    g.setAttribute('color', new THREE.BufferAttribute(new Float32Array(col), 3));
    g.computeBoundingSphere();
  }

  /** A line from each balloon, and from the selected track, down to the ground below
   *  it, so its height reads against the map; faint at the ground, strong at the top.
   *  Only those: on every aircraft the lines bury the picture. */
  private buildDrops(): void {
    const pos: number[] = [], col: number[] = [];
    const ground = new THREE.Vector3();
    for (const p of this.placed) {
      const { shape, altM } = p.item;
      if (shape !== 'balloon' && !(p.item.id === this.selected && (altM ?? 0) > 60)) continue;
      toVec(p.item.lat, p.item.lon, 1.0006, ground);
      const c = this.colour(p.item.colour);
      const top = 1;
      pos.push(ground.x, ground.y, ground.z, p.pos.x, p.pos.y, p.pos.z);
      col.push(c.r * 0.15, c.g * 0.15, c.b * 0.15, c.r * top, c.g * top, c.b * top);
    }
    this.setLines(this.drops, pos, col);
  }

  /** Quads from consecutive path points down to the ground, as one curtain mesh: the
   *  flight path as a wall whose top edge is the route at altitude. Alpha fades to the
   *  ground and, with `fade`, towards the oldest point. */
  private setCurtain(mesh: THREE.Mesh, paths: { points: HeimdallHistoryPoint[]; colour: THREE.Color; alpha: number; fade: boolean }[], drop?: number): void {
    const pos: number[] = [], col: number[] = [];
    const a = new THREE.Vector3(), b = new THREE.Vector3(), ag = new THREE.Vector3(), bg = new THREE.Vector3();
    for (const { points, colour: c, alpha, fade } of paths) {
      const n = points.length;
      if (n < 2) continue;
      // The lower edge: the ground, or `drop` below the path (never below the ground).
      const lower = (pt: HeimdallHistoryPoint, top: THREE.Vector3, out: THREE.Vector3) => {
        const above = top.length() - 1;
        const r = drop === undefined ? 1.0006 : Math.max(1.0006, top.length() - Math.min(drop, above * TRAIL_DROP_SHARE));
        return toVec(pt[1], pt[2], r, out);
      };
      this.pointAt(points[0], a);
      lower(points[0], a, ag);
      for (let i = 1; i < n; i++) {
        this.pointAt(points[i], b);
        lower(points[i], b, bg);
        const fa = fade ? 0.2 + 0.8 * ((i - 1) / (n - 1)) : 1;
        const fb = fade ? 0.2 + 0.8 * (i / (n - 1)) : 1;
        // two triangles: a, ag, b and b, ag, bg
        pos.push(a.x, a.y, a.z, ag.x, ag.y, ag.z, b.x, b.y, b.z, b.x, b.y, b.z, ag.x, ag.y, ag.z, bg.x, bg.y, bg.z);
        const top = (f: number) => [c.r, c.g, c.b, alpha * f];
        const low = (f: number) => [c.r, c.g, c.b, alpha * f * (drop === undefined ? 0.08 : 0)];
        col.push(...top(fa), ...low(fa), ...top(fb), ...top(fb), ...low(fa), ...low(fb));
        a.copy(b);
        ag.copy(bg);
      }
    }
    const g = mesh.geometry;
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(pos), 3));
    g.setAttribute('color', new THREE.BufferAttribute(new Float32Array(col), 4));
    g.computeBoundingSphere();
    mesh.visible = pos.length > 0;
  }

  /** The terrain surface (land cells only; the sea stays the globe) and the coast and
   *  borders draped over it, at the current height stretch. [FR-P3-3] */
  /** Swap in the sharper terrain grid once it has downloaded (bundled with the console,
   *  so this works offline), then rebuild everything that sits on the ground. */
  private loadHiresTerrain(): void {
    const base = (import.meta as { env?: { BASE_URL?: string } }).env?.BASE_URL ?? '/';
    fetch(`${base}imagery/${HIRES.file}`)
      .then((r) => (r.ok ? r.arrayBuffer() : Promise.reject(new Error(`HTTP ${r.status}`))))
      .then((buf) => {
        if (this.disposed || buf.byteLength !== HIRES.nx * HIRES.ny * 2) return;
        GRID = { west: TERRAIN.west, north: TERRAIN.north, step: HIRES.step, nx: HIRES.nx, ny: HIRES.ny, data: new Int16Array(buf) };
        this.buildTerrain();
        this.place();
        this.buildAreas();
        this.buildTrails();
        this.requestRender();
      })
      .catch(() => { /* keep the bundled 5 km grid */ });
  }

  private buildTerrain(): void {
    const { west, north, step, nx, ny, data: terrainGrid } = GRID;
    const ground = (h: number) => 1 + (h > 0 ? drawnHeight(h, this.exaggeration) : 0) + 0.0001;
    const pos = new Float32Array(nx * ny * 3), col = new Float32Array(nx * ny * 3), uv = new Float32Array(nx * ny * 2);
    const v = new THREE.Vector3(), c = new THREE.Color();
    for (let j = 0; j < ny; j++) {
      for (let i = 0; i < nx; i++) {
        const h = terrainGrid[j * nx + i];
        toVec(north - j * step, west + i * step, ground(h), v);
        const k = (j * nx + i) * 3;
        pos[k] = v.x; pos[k + 1] = v.y; pos[k + 2] = v.z;
        reliefColour(h, c);
        col[k] = c.r; col[k + 1] = c.g; col[k + 2] = c.b;
        // The UK image covers exactly the terrain grid's box.
        uv[(j * nx + i) * 2] = i / (nx - 1);
        uv[(j * nx + i) * 2 + 1] = 1 - j / (ny - 1);
      }
    }
    const index: number[] = [];
    for (let j = 0; j + 1 < ny; j++) {
      for (let i = 0; i + 1 < nx; i++) {
        const a = j * nx + i, b = a + 1, d = a + nx, e = d + 1;
        // With imagery the whole box is drawn (the image has the sea in it); without, land only.
        if (!this.imagery && terrainGrid[a] <= 0 && terrainGrid[b] <= 0 && terrainGrid[d] <= 0 && terrainGrid[e] <= 0) continue;
        index.push(a, d, b, b, d, e);
      }
    }
    const g = this.terrain.geometry;
    g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    g.setAttribute('color', new THREE.BufferAttribute(col, 3));
    g.setAttribute('uv', new THREE.BufferAttribute(uv, 2));
    g.setIndex(index);
    g.computeVertexNormals();
    g.computeBoundingSphere();
    // The draped overlays' surface: every cell of the box, a little above the ground.
    const dpos = new Float32Array(nx * ny * 3);
    for (let j = 0; j < ny; j++) {
      for (let i = 0; i < nx; i++) {
        const h = terrainGrid[j * nx + i];
        toVec(north - j * step, west + i * step, ground(h) + 0.00012, v);
        const k = (j * nx + i) * 3;
        dpos[k] = v.x; dpos[k + 1] = v.y; dpos[k + 2] = v.z;
      }
    }
    const full: number[] = [];
    for (let j = 0; j + 1 < ny; j++) {
      for (let i = 0; i + 1 < nx; i++) {
        const a = j * nx + i, b = a + 1, d = a + nx, e = d + 1;
        full.push(a, d, b, b, d, e);
      }
    }
    this.drapeGeometry.setAttribute('position', new THREE.BufferAttribute(dpos, 3));
    this.drapeGeometry.setAttribute('uv', new THREE.BufferAttribute(uv, 2));
    this.drapeGeometry.setIndex(full);
    this.drapeGeometry.computeBoundingSphere();
    const drape = (lat: number, lon: number) => 1 + drawnHeight(Math.max(0, terrainAt(lat, lon)), this.exaggeration) + 0.0006;
    const drapeLow = (lat: number, lon: number) => 1 + drawnHeight(Math.max(0, terrainAt(lat, lon)), this.exaggeration) + 0.00025;
    for (const f of this.fatLines) {
      f.obj.geometry.dispose();
      const g = new LineSegmentsGeometry();
      g.setPositions(stripsToSegments(f.strips, drapeLow));
      f.obj.geometry = g;
      f.obj.computeLineDistances();
    }
    this.draped.forEach((line, n) => {
      line.geometry.setAttribute('position', new THREE.BufferAttribute(stripsToSegments(this.drapedStrips[n], drape), 3));
      line.geometry.computeBoundingSphere();
    });
  }

  private buildRunways(): void {
    const strips: [number, number][][] = [];
    const quads: number[] = [];
    const drape = (lat: number, lon: number) => 1 + drawnHeight(Math.max(0, terrainAt(lat, lon)), this.exaggeration) + 0.0008;
    const a = new THREE.Vector3(), b = new THREE.Vector3(), side = new THREE.Vector3(), up = new THREE.Vector3();
    for (const p of this.placed) {
      for (const [la1, lo1, la2, lo2, w] of p.item.runways ?? []) {
        strips.push([[lo1, la1], [lo2, la2]]);
        toVec(la1, lo1, drape(la1, lo1), a);
        toVec(la2, lo2, drape(la2, lo2), b);
        up.copy(a).add(b).normalize();
        side.subVectors(b, a).cross(up).normalize().multiplyScalar(Math.max(w, RUNWAY_MIN_M) / 2 / 6_371_000);
        // two triangles: a-, a+, b+ and a-, b+, b-
        quads.push(a.x - side.x, a.y - side.y, a.z - side.z, a.x + side.x, a.y + side.y, a.z + side.z, b.x + side.x, b.y + side.y, b.z + side.z,
          a.x - side.x, a.y - side.y, a.z - side.z, b.x + side.x, b.y + side.y, b.z + side.z, b.x - side.x, b.y - side.y, b.z - side.z);
      }
    }
    this.runwayLines.geometry.setAttribute('position', new THREE.BufferAttribute(stripsToSegments(strips, drape), 3));
    this.runwayLines.geometry.computeBoundingSphere();
    const g = this.runwayStrips.geometry;
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(quads), 3));
    g.computeBoundingSphere();
  }

  /** Fly in low over a track (an airport, to see its runways). */
  flyClose(id: string): void {
    const p = this.byId.get(id);
    if (!p) return;
    this.endFollow();
    this.cam.flyTo({ lat: p.item.lat, lon: p.item.lon, dist: 25 / 6371, tilt: 50 }, 1400);
    this.requestRender();
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
    // A narrow ribbon under each trail, fading downward and towards the tail.
    this.setCurtain(this.trailRibbons, this.trails.map((tr) => ({ points: tr.points, colour: this.colour(tr.colour), alpha: 0.12, fade: true })), TRAIL_DROP);
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
    this.setCurtain(this.pathCurtain, this.path && this.path.length > 1 ? [{ points: this.path, colour: c, alpha: 0.4, fade: false }] : []);
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
    for (const f of [...this.fatLines, ...this.trafficFat]) (f.obj.material as LineMaterial).resolution.set(w, h);
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
    if (!animating) this.scheduleDetail();
    let quakes = false;
    const m = new THREE.Matrix4();
    const scale = new THREE.Vector3();
    const camPos = this.camera.position;
    for (const p of this.placed) {
      const s = p.item.shape;
      const d = camPos.distanceTo(p.pos);
      let px = PX[s];
      if (s === 'satellite' && p.item.station) px = 14;
      // Fire hotspots grow with their radiative power, capped.
      if (s === 'fire') px = 4 + Math.min(8, Math.sqrt(Math.max(0, p.item.magnitude ?? 1)) * 1.2);
      if (s === 'quake') {
        quakes = true;
        const pulse = 1 + 0.22 * Math.sin(nowMs / 420 + p.shapeIndex);
        px = (4 + 2.2 * Math.max(0, p.item.magnitude ?? 3)) * pulse;
      }
      const k = px * this.markerScale * this.perPx(d);
      if (s === 'aircraft' || s === 'vessel') scale.set(k, k, k);
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
  baltic(): void {
    this.cb.onInteract?.();
    this.endFollow();
    this.cam.flyTo({ ...BALTIC_VIEW }, 1400);
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
    this.cb.onInteract?.();
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
            if (this.hoverPending) {
              this.cb.onHover(this.pick(this.hoverPending.x, this.hoverPending.y, 10));
              if (this.cb.onPointerGround) {
                const [nx, ny] = this.ndc(this.hoverPending.x, this.hoverPending.y);
                this.cb.onPointerGround(this.cam.surfaceAt(nx, ny));
              }
            }
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
        if (this.pickSurface) {
          const [nx, ny] = this.ndc(p.x, p.y);
          const at = this.cam.surfaceAt(nx, ny);
          if (at) this.cb.onSurface?.(at);
        } else {
          const id = this.pick(p.x, p.y, touch ? 28 : 10);
          if (id) this.cb.onSelect(id);
        }
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
    this.cb.onInteract?.();
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

  /** After the camera settles: if close enough, (re)build the detail for the area in
   *  view; if zoomed out, drop it. */
  private scheduleDetail(): void {
    window.clearTimeout(this.detailTimer);
    this.detailTimer = window.setTimeout(() => this.updateDetail(), 350);
  }

  private updateDetail(): void {
    if (this.disposed) return;
    const s = this.cam.state;
    const km = s.dist * 6371;
    if (km > 700) {
      this.detailWant = '';
      this.detailAbort?.abort();
      if (this.detail) {
        this.scene.remove(this.detail.mesh);
        this.detail.mesh.geometry.dispose();
        const m = this.detail.mesh.material as THREE.MeshBasicMaterial;
        m.map?.dispose();
        m.dispose();
        this.detail = null;
        this.requestRender();
      }
      this.updateTraffic(null);
      return;
    }
    // The area in view, a little generous so a small pan stays covered; snapped so the
    // same view asks for the same box.
    const span = Math.min(8, Math.max(0.2, (km * 1.8) / 111));
    const q = span / 4;
    const lat = Math.round(s.lat / q) * q, lon = Math.round(s.lon / q) * q;
    const cos = Math.max(0.2, Math.cos((lat * Math.PI) / 180));
    const box: Box = { west: lon - span / cos / 2, east: lon + span / cos / 2, south: lat - span / 2, north: lat + span / 2 };
    this.updateTraffic(box);
    const key = `${this.mapStyle}:${box.west.toFixed(3)}:${box.south.toFixed(3)}:${span.toFixed(3)}:${this.exaggeration}`;
    if (key === this.detailWant || key === this.detail?.key) return;
    this.detailWant = key;
    this.detailAbort?.abort();
    const ctrl = new AbortController();
    this.detailAbort = ctrl;
    buildDetail(box, this.mapStyle, ctrl.signal)
      .then((d) => {
        if (ctrl.signal.aborted || this.disposed || this.detailWant !== key) return;
        // A grid over the box on the same terrain surface as the base map, so roads,
        // rivers and overlays keep sitting on it; the detail is in the image.
        const gw = 256, gh = Math.max(32, Math.round(256 * ((box.north - box.south) / (box.east - box.west)) * cos));
        const pos = new Float32Array((gw + 1) * (gh + 1) * 3), uv = new Float32Array((gw + 1) * (gh + 1) * 2);
        const v = new THREE.Vector3();
        for (let j = 0; j <= gh; j++) {
          for (let i = 0; i <= gw; i++) {
            const la = box.north - (j / gh) * (box.north - box.south), lo = box.west + (i / gw) * (box.east - box.west);
            // The surface from the detail's own elevation, so its shape matches its shading.
            toVec(la, lo, 1 + drawnHeight(Math.max(0, d.heightAt(la, lo)), this.exaggeration) + 0.00011, v);
            const k = j * (gw + 1) + i;
            pos[k * 3] = v.x; pos[k * 3 + 1] = v.y; pos[k * 3 + 2] = v.z;
            uv[k * 2] = i / gw; uv[k * 2 + 1] = 1 - j / gh;
          }
        }
        const index: number[] = [];
        for (let j = 0; j < gh; j++) {
          for (let i = 0; i < gw; i++) {
            const a = j * (gw + 1) + i, b = a + 1, c = a + gw + 1, e = c + 1;
            index.push(a, c, b, b, c, e);
          }
        }
        const g = new THREE.BufferGeometry();
        g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
        g.setAttribute('uv', new THREE.BufferAttribute(uv, 2));
        g.setIndex(index);
        const tex = new THREE.CanvasTexture(d.canvas);
        tex.colorSpace = THREE.SRGBColorSpace;
        tex.anisotropy = Math.min(8, this.renderer.capabilities.getMaxAnisotropy());
        const mesh = new THREE.Mesh(g, new THREE.MeshBasicMaterial({ map: tex, polygonOffset: true, polygonOffsetFactor: -1, polygonOffsetUnits: -1 }));
        mesh.renderOrder = 0;
        if (this.detail) {
          this.scene.remove(this.detail.mesh);
          this.detail.mesh.geometry.dispose();
          const m = this.detail.mesh.material as THREE.MeshBasicMaterial;
          m.map?.dispose();
          m.dispose();
        }
        this.scene.add(mesh);
        this.detail = { mesh, key };
        this.requestRender();
      })
      .catch(() => { if (this.detailWant === key) this.detailWant = ''; });
  }

  dispose(): void {
    window.clearTimeout(this.detailTimer);
    this.detailAbort?.abort();
    this.disposed = true;
    cancelAnimationFrame(this.frame);
    cancelAnimationFrame(this.hoverFrame);
    window.clearTimeout(this.pulseTimer);
    this.ro.disconnect();
    this.drapeTexture.dispose();
    this.radarTexture?.dispose();
    this.drapeGeometry.dispose();
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
