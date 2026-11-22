/* The globe renderer: three.js, drawn on demand. [FR-H-1] [AC-H-1]
 *
 * One sphere carries the clean style (flat land and sea from the design tokens,
 * painted once into an equirectangular texture from the 1:50m land) and the day and
 * night shading from the subsolar point. Over it sit the graticule, the coastlines and
 * the land borders as lines (1:110m far out, 1:50m close in), a soft atmosphere edge,
 * and the track markers. The camera, the markers' placement and the picking are the
 * pure modules beside this file; this one only draws and listens. Nothing here makes
 * a network request: the bundled outlines are handed in by the view, which fetches
 * them from the console's own files. */
import * as THREE from 'three';

import {
  CAMERA_FOV_DEG,
  type CameraState,
  MIN_ALTITUDE_KM,
  clipPlanes,
  flyDurationMs,
  interpolateCamera,
  normaliseCamera,
  panCamera,
  zoomCamera,
} from './camera.ts';
import { type GlobeColours, type Rgba, cssRgba } from './colours.ts';
import { graticule, lineSegments } from './geometry.ts';
import type { GlobeItem } from './items.ts';
import { type OutlineFile, coastFromLand, decodeLine } from './outlines.ts';
import { isOccluded, toXyz } from './projection.ts';
import { subsolarPoint } from './sun.ts';

/** Below this altitude the 1:50m outlines replace the 1:110m ones. */
const DETAIL_BELOW_KM = 3000;
/** Above this altitude only the selected and hovered tracks keep their labels. */
const LABELS_BELOW_KM = 6000;
const MARKER_PX = 13;
const PICK_PX = 14;
const TEXTURE_WIDTH = 4096;

export interface GlobeCallbacks {
  /** The track under the pointer, or null. */
  onHover(id: string | null): void;
  /** A click on a track. */
  onSelect(id: string): void;
  /** The camera moved; `settled` once a drag, zoom or flight has ended. */
  onCamera(camera: CameraState, settled: boolean): void;
  /** WebGL went away after start-up. */
  onContextLost(): void;
}

/** Whether this browser can give the globe a WebGL 2 context, which three.js needs.
 *  The probe's context is released at once. */
export function webglAvailable(): boolean {
  try {
    const gl = document.createElement('canvas').getContext('webgl2');
    if (!gl) return false;
    gl.getExtension('WEBGL_lose_context')?.loseContext();
    return true;
  } catch {
    return false;
  }
}

export interface GlobeOutlines {
  /** 1:110m coastlines and borders. */
  world: OutlineFile;
  /** 1:50m land and borders. */
  close: OutlineFile;
}

const toColor = (c: Rgba) => new THREE.Color().setRGB(c.r, c.g, c.b, THREE.SRGBColorSpace);

const GLOBE_VERTEX = /* glsl */ `
  varying vec2 vUv;
  varying vec3 vNormalW;
  void main() {
    vUv = uv;
    vNormalW = normalize(mat3(modelMatrix) * normal);
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  }
`;

const GLOBE_FRAGMENT = /* glsl */ `
  uniform sampler2D map;
  uniform vec3 sunDir;
  uniform float night;
  varying vec2 vUv;
  varying vec3 vNormalW;
  void main() {
    vec3 base = texture2D(map, vUv).rgb;
    float d = dot(normalize(vNormalW), sunDir);
    float light = mix(night, 1.0, smoothstep(-0.12, 0.12, d));
    gl_FragColor = vec4(base * light, 1.0);
    #include <colorspace_fragment>
  }
`;

const ATMOSPHERE_VERTEX = /* glsl */ `
  varying vec3 vNormalV;
  varying vec3 vPosV;
  void main() {
    vNormalV = normalize(normalMatrix * normal);
    vec4 mv = modelViewMatrix * vec4(position, 1.0);
    vPosV = mv.xyz;
    gl_Position = projectionMatrix * mv;
  }
`;

const ATMOSPHERE_FRAGMENT = /* glsl */ `
  uniform vec3 glow;
  uniform float strength;
  varying vec3 vNormalV;
  varying vec3 vPosV;
  void main() {
    // Seen from inside, the far wall faces away: brightest at the limb of the globe,
    // fading to nothing at the atmosphere's outer edge.
    float facing = -dot(normalize(vNormalV), normalize(-vPosV));
    float rim = pow(clamp(facing / 0.3, 0.0, 1.0), 1.6);
    gl_FragColor = vec4(glow, rim * strength);
    #include <colorspace_fragment>
  }
`;

type Shape = 'circle' | 'square' | 'diamond' | 'ring';

function shapeTexture(shape: Shape): THREE.CanvasTexture {
  const n = 64;
  const c = document.createElement('canvas');
  c.width = n;
  c.height = n;
  const g = c.getContext('2d')!;
  g.fillStyle = '#fff';
  g.strokeStyle = '#fff';
  const m = n / 2;
  g.beginPath();
  if (shape === 'circle') g.arc(m, m, m * 0.62, 0, Math.PI * 2);
  else if (shape === 'square') g.rect(m * 0.42, m * 0.42, m * 1.16, m * 1.16);
  else if (shape === 'diamond') {
    g.moveTo(m, m * 0.24);
    g.lineTo(m * 1.76, m);
    g.lineTo(m, m * 1.76);
    g.lineTo(m * 0.24, m);
    g.closePath();
  }
  if (shape === 'ring') {
    g.lineWidth = 5;
    g.arc(m, m, m * 0.84, 0, Math.PI * 2);
    g.stroke();
  } else {
    g.fill();
  }
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}

const SHAPE_FOR_LAYER: Record<string, Shape> = { air: 'circle', maritime: 'square', ground: 'diamond' };

interface Marker {
  item: GlobeItem;
  sprite: THREE.Sprite;
  drop: THREE.Line | null;
  label: HTMLDivElement;
}

export class GlobeRenderer {
  private readonly renderer: THREE.WebGLRenderer;
  private readonly scene = new THREE.Scene();
  private readonly camera3 = new THREE.PerspectiveCamera(CAMERA_FOV_DEG, 1, 0.01, 10);
  private readonly globeMat: THREE.ShaderMaterial;
  private readonly atmosphereMat: THREE.ShaderMaterial;
  private readonly landTexture: THREE.CanvasTexture;
  private readonly landCanvas: HTMLCanvasElement;
  private readonly lines: { mat: THREE.LineBasicMaterial; obj: THREE.LineSegments; kind: 'graticule' | 'coast' | 'border'; detail: 'far' | 'near' | 'all' }[] = [];
  private readonly textures: Record<Shape, THREE.CanvasTexture>;
  private readonly markerGroup = new THREE.Group();
  private readonly selectRing: THREE.Sprite;
  private markers: Marker[] = [];
  private colours: GlobeColours;
  private cam: CameraState = { lat: 54.6, lon: -4, altKm: 2200 };
  private width = 1;
  private height = 1;
  private frame = 0;
  private flight: { from: CameraState; to: CameraState; start: number; ms: number } | null = null;
  private drag: { id: number; x: number; y: number; moved: boolean } | null = null;
  private readonly pointers = new Map<number, { x: number; y: number }>();
  private pinch: number | null = null;
  private hovered: string | null = null;
  private selected: string | null = null;
  private tooltip: HTMLElement | null = null;
  private settleTimer = 0;
  private floorKm = MIN_ALTITUDE_KM;
  private sunTimer = 0;
  private readonly resize: ResizeObserver;
  private readonly unlisten: (() => void)[] = [];
  private disposed = false;

  constructor(
    private readonly host: HTMLElement,
    private readonly labelLayer: HTMLElement,
    colours: GlobeColours,
    private readonly outlines: GlobeOutlines,
    private readonly cb: GlobeCallbacks,
  ) {
    this.colours = colours;
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: 'low-power' });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.setClearColor(0x000000, 0);
    const canvas = this.renderer.domElement;
    canvas.style.display = 'block';
    canvas.style.width = '100%';
    canvas.style.height = '100%';
    canvas.style.touchAction = 'none';
    canvas.setAttribute('aria-hidden', 'true');
    host.appendChild(canvas);

    // The clean style's land and sea, painted from the 1:50m land polygons.
    this.landCanvas = document.createElement('canvas');
    this.landCanvas.width = TEXTURE_WIDTH;
    this.landCanvas.height = TEXTURE_WIDTH / 2;
    this.landTexture = new THREE.CanvasTexture(this.landCanvas);
    this.landTexture.colorSpace = THREE.SRGBColorSpace;
    this.landTexture.anisotropy = Math.min(8, this.renderer.capabilities.getMaxAnisotropy());
    this.paintLand();

    this.globeMat = new THREE.ShaderMaterial({
      uniforms: { map: { value: this.landTexture }, sunDir: { value: new THREE.Vector3(1, 0, 0) }, night: { value: 0.5 } },
      vertexShader: GLOBE_VERTEX,
      fragmentShader: GLOBE_FRAGMENT,
    });
    this.scene.add(new THREE.Mesh(new THREE.SphereGeometry(1, 160, 80), this.globeMat));

    this.atmosphereMat = new THREE.ShaderMaterial({
      uniforms: { glow: { value: toColor(colours.atmosphere) }, strength: { value: 0.5 } },
      vertexShader: ATMOSPHERE_VERTEX,
      fragmentShader: ATMOSPHERE_FRAGMENT,
      side: THREE.BackSide,
      transparent: true,
      depthWrite: false,
    });
    this.scene.add(new THREE.Mesh(new THREE.SphereGeometry(1.035, 96, 48), this.atmosphereMat));

    const w = outlines.world;
    const c = outlines.close;
    this.addLines('graticule', 'all', graticule(15, 2), 1.0004);
    this.addLines('coast', 'far', (w.coast ?? []).map((l) => decodeLine(l, w.quantum)), 1.0008);
    this.addLines('border', 'far', (w.borders ?? []).map((l) => decodeLine(l, w.quantum)), 1.0007);
    this.addLines('coast', 'near', coastFromLand(c.land ?? [], c.quantum), 1.0002);
    this.addLines('border', 'near', (c.borders ?? []).map((l) => decodeLine(l, c.quantum)), 1.0002);
    this.applyLineColours();

    this.textures = { circle: shapeTexture('circle'), square: shapeTexture('square'), diamond: shapeTexture('diamond'), ring: shapeTexture('ring') };
    this.selectRing = new THREE.Sprite(new THREE.SpriteMaterial({ map: this.textures.ring, sizeAttenuation: false, depthTest: false, transparent: true }));
    this.selectRing.visible = false;
    this.selectRing.renderOrder = 3;
    this.markerGroup.add(this.selectRing);
    this.scene.add(this.markerGroup);

    this.updateSun();
    this.sunTimer = window.setInterval(() => { this.updateSun(); this.requestRender(); }, 60_000);

    this.resize = new ResizeObserver(() => this.onResize());
    this.resize.observe(host);
    this.onResize();
    this.listen(canvas);
    this.applyCamera();
  }

  // --- public surface -------------------------------------------------------------

  get cameraState(): CameraState {
    return this.cam;
  }

  setColours(colours: GlobeColours): void {
    this.colours = colours;
    this.paintLand();
    this.atmosphereMat.uniforms.glow.value = toColor(colours.atmosphere);
    this.applyLineColours();
    for (const m of this.markers) this.styleMarker(m);
    this.styleSelection();
    this.requestRender();
  }

  setItems(items: readonly GlobeItem[]): void {
    for (const m of this.markers) {
      this.markerGroup.remove(m.sprite);
      (m.sprite.material as THREE.SpriteMaterial).dispose();
      if (m.drop) {
        this.markerGroup.remove(m.drop);
        m.drop.geometry.dispose();
        (m.drop.material as THREE.LineBasicMaterial).dispose();
      }
      m.label.remove();
    }
    this.markers = items.map((item) => {
      const sprite = new THREE.Sprite(new THREE.SpriteMaterial({ map: this.textures[SHAPE_FOR_LAYER[item.layer] ?? 'circle'], sizeAttenuation: false, depthTest: false, transparent: true }));
      sprite.position.set(item.position.x, item.position.y, item.position.z);
      sprite.renderOrder = 2;
      this.markerGroup.add(sprite);
      let drop: THREE.Line | null = null;
      if (item.altitudeKm > 0) {
        const g = new THREE.BufferGeometry().setFromPoints([
          new THREE.Vector3(item.ground.x, item.ground.y, item.ground.z),
          new THREE.Vector3(item.position.x, item.position.y, item.position.z),
        ]);
        drop = new THREE.Line(g, new THREE.LineBasicMaterial({ transparent: true, opacity: 0.55 }));
        this.markerGroup.add(drop);
      }
      const label = document.createElement('div');
      label.className = 'hg-globe-label';
      label.textContent = item.id;
      this.labelLayer.appendChild(label);
      const m: Marker = { item, sprite, drop, label };
      this.styleMarker(m);
      return m;
    });
    if (this.hovered && !this.markers.some((m) => m.item.id === this.hovered)) this.setHovered(null);
    this.styleSelection();
    this.requestRender();
  }

  /** Extra label text per track, such as " · 78°". */
  setLabelText(text: (id: string) => string): void {
    for (const m of this.markers) m.label.textContent = text(m.item.id);
  }

  setSelected(id: string | null): void {
    this.selected = id;
    this.styleSelection();
    this.requestRender();
  }

  /** The element the hover tooltip lives in; positioned beside the hovered track. */
  setTooltipAnchor(el: HTMLElement | null): void {
    this.tooltip = el;
    this.requestRender();
  }

  /** The lowest the camera may go now, from the highest marker (`cameraFloorKm`). */
  setCameraFloor(km: number): void {
    this.floorKm = Math.max(MIN_ALTITUDE_KM, km);
    if (this.cam.altKm < this.floorKm) this.applyCamera();
  }

  flyTo(target: CameraState, reducedMotion: boolean): void {
    const to = normaliseCamera(target);
    const ms = flyDurationMs(this.cam, to, reducedMotion);
    if (ms === 0) {
      this.flight = null;
      this.cam = to;
      this.applyCamera();
      this.settled();
      return;
    }
    this.flight = { from: this.cam, to, start: performance.now(), ms };
    this.requestRender();
  }

  /** Keyboard orbit and zoom, for the focusable globe surface. */
  nudge(dxPx: number, dyPx: number, zoom = 1): void {
    this.flight = null;
    this.cam = zoomCamera(panCamera(this.cam, dxPx, dyPx, this.height), zoom);
    this.applyCamera();
    this.settleSoon();
  }

  dispose(): void {
    this.disposed = true;
    cancelAnimationFrame(this.frame);
    window.clearInterval(this.sunTimer);
    window.clearTimeout(this.settleTimer);
    this.resize.disconnect();
    for (const u of this.unlisten) u();
    for (const m of this.markers) m.label.remove();
    this.scene.traverse((o) => {
      const mesh = o as THREE.Mesh;
      if (mesh.geometry) mesh.geometry.dispose();
      const mat = mesh.material as THREE.Material | THREE.Material[] | undefined;
      if (Array.isArray(mat)) mat.forEach((x) => x.dispose());
      else mat?.dispose();
    });
    for (const t of Object.values(this.textures)) t.dispose();
    this.landTexture.dispose();
    this.renderer.dispose();
    this.renderer.forceContextLoss();
    this.renderer.domElement.remove();
  }

  // --- drawing --------------------------------------------------------------------

  private paintLand(): void {
    const g = this.landCanvas.getContext('2d')!;
    const W = this.landCanvas.width;
    const H = this.landCanvas.height;
    g.fillStyle = cssRgba({ ...this.colours.sea, a: 1 });
    g.fillRect(0, 0, W, H);
    g.fillStyle = cssRgba({ ...this.colours.land, a: 1 });
    const c = this.outlines.close;
    for (const poly of c.land ?? []) {
      g.beginPath();
      for (const ring of poly) {
        const p = decodeLine(ring, c.quantum);
        for (let i = 0; i < p.length; i += 2) {
          const x = ((p[i] + 180) / 360) * W;
          const y = ((90 - p[i + 1]) / 180) * H;
          if (i === 0) g.moveTo(x, y);
          else g.lineTo(x, y);
        }
        g.closePath();
      }
      g.fill('evenodd');
    }
    this.landTexture.needsUpdate = true;
  }

  private addLines(kind: 'graticule' | 'coast' | 'border', detail: 'far' | 'near' | 'all', lines: Float64Array[], radius: number): void {
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(lineSegments(lines, radius, 1), 3));
    const mat = new THREE.LineBasicMaterial({ transparent: true });
    const obj = new THREE.LineSegments(geo, mat);
    obj.renderOrder = 1;
    this.scene.add(obj);
    this.lines.push({ mat, obj, kind, detail });
  }

  private applyLineColours(): void {
    for (const l of this.lines) {
      const c = l.kind === 'graticule' ? this.colours.graticule : l.kind === 'coast' ? this.colours.coast : this.colours.border;
      l.mat.color = toColor(c);
      // The graticule token is a faint overlay colour; keep it faint on the globe too.
      l.mat.opacity = l.kind === 'graticule' ? Math.max(0.12, Math.min(0.35, c.a * 3)) : l.kind === 'coast' ? 0.95 : 0.7;
    }
  }

  private styleMarker(m: Marker): void {
    const c = toColor(m.item.tone === 'warn' ? this.colours.warn : this.colours.accent);
    (m.sprite.material as THREE.SpriteMaterial).color = c;
    if (m.drop) (m.drop.material as THREE.LineBasicMaterial).color = c;
    m.label.dataset.tone = m.item.tone;
  }

  private styleSelection(): void {
    const m = this.markers.find((x) => x.item.id === this.selected);
    this.selectRing.visible = Boolean(m);
    if (m) {
      this.selectRing.position.copy(m.sprite.position);
      (this.selectRing.material as THREE.SpriteMaterial).color = toColor(m.item.tone === 'warn' ? this.colours.warn : this.colours.accent);
    }
    for (const x of this.markers) x.label.dataset.selected = String(x.item.id === this.selected);
  }

  private updateSun(): void {
    const s = subsolarPoint(new Date());
    const p = toXyz(s.lat, s.lon);
    (this.globeMat.uniforms.sunDir.value as THREE.Vector3).set(p.x, p.y, p.z).normalize();
  }

  private onResize(): void {
    this.width = Math.max(1, this.host.clientWidth);
    this.height = Math.max(1, this.host.clientHeight);
    this.renderer.setSize(this.width, this.height, false);
    this.camera3.aspect = this.width / this.height;
    this.camera3.updateProjectionMatrix();
    this.requestRender();
  }

  private applyCamera(): void {
    if (this.cam.altKm < this.floorKm) this.cam = { ...this.cam, altKm: this.floorKm };
    const { near, far } = clipPlanes(this.cam.altKm);
    const p = toXyz(this.cam.lat, this.cam.lon, this.cam.altKm);
    this.camera3.position.set(p.x, p.y, p.z);
    this.camera3.up.set(0, 1, 0);
    this.camera3.lookAt(0, 0, 0);
    this.camera3.near = near;
    this.camera3.far = far;
    this.camera3.updateProjectionMatrix();
    // Labels and picking project with this camera before the next render updates it.
    this.camera3.updateMatrixWorld();
    const closeIn = this.cam.altKm < DETAIL_BELOW_KM;
    for (const l of this.lines) l.obj.visible = l.detail === 'all' || (l.detail === 'near') === closeIn;
    // The atmosphere fades out as the camera comes down into it.
    this.atmosphereMat.uniforms.strength.value = Math.min(0.55, Math.max(0, (this.cam.altKm - 250) / 1500));
    this.cb.onCamera(this.cam, false);
    this.requestRender();
  }

  private requestRender(): void {
    if (this.frame || this.disposed) return;
    this.frame = requestAnimationFrame((t) => {
      this.frame = 0;
      this.tick(t);
    });
  }

  private tick(now: number): void {
    if (this.flight) {
      const f = this.flight;
      const t = Math.min(1, (now - f.start) / f.ms);
      this.cam = interpolateCamera(f.from, f.to, t);
      if (t >= 1) {
        this.flight = null;
        this.applyCamera();
        this.settled();
      } else {
        this.applyCamera();
      }
    }
    this.placeOverlays();
    this.renderer.render(this.scene, this.camera3);
  }

  /** Marker size, visibility and the HTML labels and tooltip, for this frame. */
  private placeOverlays(): void {
    const scale = (2 * MARKER_PX * Math.tan((CAMERA_FOV_DEG * Math.PI) / 360)) / this.height;
    const camPos = this.camera3.position;
    const v = new THREE.Vector3();
    for (const m of this.markers) {
      const hidden = isOccluded(camPos, m.item.position);
      const big = m.item.id === this.hovered ? 1.35 : 1;
      m.sprite.scale.set(scale * big, scale * big, 1);
      m.sprite.visible = !hidden;
      if (m.drop) m.drop.visible = !hidden;
      v.set(m.item.position.x, m.item.position.y, m.item.position.z).project(this.camera3);
      const x = ((v.x + 1) / 2) * this.width;
      const y = ((1 - v.y) / 2) * this.height;
      const onScreen = !hidden && v.z > -1 && v.z < 1 && x > -40 && x < this.width + 40 && y > -40 && y < this.height + 40;
      const named = this.cam.altKm < LABELS_BELOW_KM || m.item.id === this.selected || m.item.id === this.hovered;
      m.label.style.display = onScreen && named ? '' : 'none';
      m.label.style.transform = `translate(${Math.round(x)}px, ${Math.round(y + MARKER_PX)}px) translateX(-50%)`;
      if (this.tooltip && m.item.id === this.hovered) {
        this.tooltip.style.display = onScreen ? '' : 'none';
        const below = y < 150;
        this.tooltip.style.left = `${Math.round(Math.min(this.width - 110, Math.max(110, x)))}px`;
        this.tooltip.style.top = `${Math.round(y)}px`;
        this.tooltip.style.transform = below ? 'translate(-50%, 22px)' : 'translate(-50%, calc(-100% - 18px))';
      }
    }
    this.selectRing.scale.set(scale * 2, scale * 2, 1);
    const sel = this.markers.find((m) => m.item.id === this.selected);
    this.selectRing.visible = Boolean(sel && sel.sprite.visible);
  }

  // --- input ----------------------------------------------------------------------

  private listen(canvas: HTMLCanvasElement): void {
    const on = <K extends keyof HTMLElementEventMap>(type: K, fn: (e: HTMLElementEventMap[K]) => void, opts?: AddEventListenerOptions) => {
      canvas.addEventListener(type, fn as EventListener, opts);
      this.unlisten.push(() => canvas.removeEventListener(type, fn as EventListener, opts));
    };
    on('pointerdown', (e) => {
      canvas.setPointerCapture(e.pointerId);
      this.pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      this.flight = null;
      if (this.pointers.size === 1) this.drag = { id: e.pointerId, x: e.clientX, y: e.clientY, moved: false };
      else {
        this.drag = null;
        this.pinch = this.pinchDistance();
      }
    });
    on('pointermove', (e) => {
      const prev = this.pointers.get(e.pointerId);
      if (!prev) {
        this.pick(e);
        return;
      }
      this.pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (this.pointers.size >= 2 && this.pinch) {
        const d = this.pinchDistance();
        if (d > 0) this.cam = zoomCamera(this.cam, this.pinch / d);
        this.pinch = d;
        this.applyCamera();
        return;
      }
      if (this.drag && this.drag.id === e.pointerId) {
        const dx = e.clientX - prev.x;
        const dy = e.clientY - prev.y;
        if (Math.abs(e.clientX - this.drag.x) + Math.abs(e.clientY - this.drag.y) > 4) this.drag.moved = true;
        if (this.drag.moved) {
          this.cam = panCamera(this.cam, dx, dy, this.height);
          this.applyCamera();
        }
      }
    });
    const end = (e: PointerEvent) => {
      const wasDrag = this.drag;
      const wasPinch = this.pinch !== null;
      this.pointers.delete(e.pointerId);
      if (this.pointers.size < 2) this.pinch = null;
      if (wasPinch && this.pinch === null) {
        // The pinch is over: settle, and let a finger still down carry on orbiting.
        const rest = [...this.pointers.entries()][0];
        this.drag = rest ? { id: rest[0], x: rest[1].x, y: rest[1].y, moved: true } : null;
        this.settled();
        return;
      }
      if (wasDrag && wasDrag.id === e.pointerId) {
        this.drag = null;
        if (!wasDrag.moved && e.type === 'pointerup') {
          const id = this.pickAt(e);
          if (id) this.cb.onSelect(id);
        } else {
          this.settled();
        }
      }
    };
    on('pointerup', end);
    on('pointercancel', end);
    on('pointerleave', () => { if (!this.drag) this.setHovered(null); });
    on('wheel', (e) => {
      e.preventDefault();
      this.flight = null;
      const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? this.height : 1;
      this.cam = zoomCamera(this.cam, Math.exp(e.deltaY * unit * 0.0015));
      this.applyCamera();
      this.settleSoon();
    }, { passive: false });
    const lost = (e: Event) => {
      e.preventDefault();
      this.cb.onContextLost();
    };
    canvas.addEventListener('webglcontextlost', lost);
    this.unlisten.push(() => canvas.removeEventListener('webglcontextlost', lost));
  }

  private pinchDistance(): number {
    const [a, b] = [...this.pointers.values()];
    return a && b ? Math.hypot(a.x - b.x, a.y - b.y) : 0;
  }

  private pickAt(e: { clientX: number; clientY: number }): string | null {
    const r = this.host.getBoundingClientRect();
    const px = e.clientX - r.left;
    const py = e.clientY - r.top;
    const v = new THREE.Vector3();
    let best: string | null = null;
    let bestD = PICK_PX;
    for (const m of this.markers) {
      if (isOccluded(this.camera3.position, m.item.position)) continue;
      v.set(m.item.position.x, m.item.position.y, m.item.position.z).project(this.camera3);
      if (v.z <= -1 || v.z >= 1) continue;
      const d = Math.hypot(((v.x + 1) / 2) * this.width - px, ((1 - v.y) / 2) * this.height - py);
      if (d <= bestD) {
        bestD = d;
        best = m.item.id;
      }
    }
    return best;
  }

  private pick(e: PointerEvent): void {
    const id = this.pickAt(e);
    this.renderer.domElement.style.cursor = id ? 'pointer' : 'grab';
    this.setHovered(id);
  }

  /** The hovered track; also set from the track list, so keyboard focus shows its tooltip. */
  setHovered(id: string | null): void {
    if (id === this.hovered) return;
    this.hovered = id;
    this.cb.onHover(id);
    this.requestRender();
  }

  private settleSoon(): void {
    window.clearTimeout(this.settleTimer);
    this.settleTimer = window.setTimeout(() => this.settled(), 250);
  }

  private settled(): void {
    window.clearTimeout(this.settleTimer);
    this.cb.onCamera(this.cam, true);
  }
}

