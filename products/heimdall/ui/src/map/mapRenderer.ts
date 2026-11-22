/* The 2D geographic map, drawn on a canvas. [FR-H-1] [AC-H-1]
 *
 * Under everything, the bundled Natural Earth land (the console's own files), so the
 * map still shows coasts with no network. Over it, VersaTiles vector tiles
 * (OpenStreetMap data, Shortbread schema) for the area in view: water, green and
 * built-up land, buildings, roads, rail, ferries, boundaries and place names, in the
 * design tokens' colours. While a tile loads, its nearest loaded ancestor stands in.
 * Track markers are the view's own buttons; this places them each frame.
 *
 * Reached only through the tiles switch, like every module that fetches tiles. */
import { VectorTile } from '@mapbox/vector-tile';
import { PbfReader } from 'pbf';

import { type Rgba, cssRgba, parseCssColour } from '../globe/colours.ts';
import { type OutlineFile, decodeLine } from '../globe/outlines.ts';
import { tileUrl, TILE_SOURCES } from '../tiles/hosts.ts';
import { type LoaderState, TileLoader } from '../tiles/loader.ts';
import { type Tile, lonToTileX, latToTileY, parent } from '../tiles/tileMath.ts';
import { type FillClass, type LineClass, type MapTileData, FILL_CLASSES, LINE_CLASSES, extractFeatures } from './features.ts';
import { type MapCamera, clampMapCamera, panMap, project, visibleTiles, zoomMapAt } from './mapCamera.ts';

const VECTOR_MAX_ZOOM = TILE_SOURCES.vector.maxZoom;
/** How many zooms up a loading tile looks for a stand-in. */
const ANCESTOR_LEVELS = 5;

export interface MapCallbacks {
  /** The camera moved; `settled` once a drag, zoom or flight has ended. */
  onCamera(c: MapCamera, settled: boolean): void;
  /** The vector tiles' network went away or came back. */
  onState(s: LoaderState): void;
}

interface MapColours {
  sea: string;
  land: string;
  green: string;
  built: string;
  bare: string;
  building: string;
  coast: string;
  road: string;
  roadMajor: string;
  roadCasing: string;
  rail: string;
  boundary: string;
  label: string;
  halo: string;
  font: string;
}

const mix = (a: Rgba, b: Rgba, t: number): Rgba => ({ r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: 1 });

/** Reads the map's colours from the design tokens, as the browser computes them. */
function readColours(scope: HTMLElement): MapColours {
  const probe = document.createElement('span');
  probe.style.display = 'none';
  scope.appendChild(probe);
  const read = (token: string, fallback: Rgba): Rgba => {
    probe.style.color = `var(${token})`;
    return parseCssColour(getComputedStyle(probe).color) ?? fallback;
  };
  try {
    const sea = read('--bg-sunken', { r: 0.03, g: 0.035, b: 0.04, a: 1 });
    const land = read('--surface-raised', { r: 0.16, g: 0.17, b: 0.19, a: 1 });
    const strong = read('--border-strong', { r: 0.3, g: 0.32, b: 0.35, a: 1 });
    const border = read('--border', { r: 0.24, g: 0.25, b: 0.28, a: 1 });
    const muted = read('--text-muted', { r: 0.57, g: 0.6, b: 0.64, a: 1 });
    const faint = read('--text-faint', { r: 0.4, g: 0.42, b: 0.45, a: 1 });
    const page = read('--bg-page', sea);
    const dark = 0.2126 * sea.r + 0.7152 * sea.g + 0.0722 * sea.b < 0.5;
    const green = { r: 0.24, g: 0.55, b: 0.3, a: 1 };
    const water = mix(sea, { r: 0.2, g: 0.4, b: 0.6, a: 1 }, dark ? 0.12 : 0.18);
    return {
      sea: cssRgba(water),
      land: cssRgba({ ...land, a: 1 }),
      green: cssRgba(mix(land, green, dark ? 0.14 : 0.2)),
      built: cssRgba(mix(land, faint, 0.12)),
      bare: cssRgba(mix(land, muted, 0.08)),
      building: cssRgba(mix(land, border, 0.6)),
      coast: cssRgba({ ...strong, a: 1 }),
      road: cssRgba(mix(border, muted, 0.25)),
      roadMajor: cssRgba(mix(strong, muted, 0.45)),
      roadCasing: cssRgba({ ...land, a: 1 }),
      rail: cssRgba(mix(border, faint, 0.5)),
      boundary: cssRgba(mix(strong, muted, 0.3)),
      label: cssRgba({ ...muted, a: 1 }),
      halo: cssRgba({ ...page, a: 0.85 }),
      font: getComputedStyle(scope).getPropertyValue('--font-mono').trim() || 'monospace',
    };
  } finally {
    probe.remove();
  }
}

const LINE_STYLE: Record<LineClass, { width: number; colour: keyof MapColours; dash?: number[] }> = {
  water: { width: 0.9, colour: 'sea' },
  region: { width: 0.8, colour: 'boundary', dash: [4, 3] },
  country: { width: 1.3, colour: 'boundary', dash: [7, 3, 2, 3] },
  ferry: { width: 0.8, colour: 'boundary', dash: [3, 4] },
  rail: { width: 0.9, colour: 'rail', dash: [5, 3] },
  local: { width: 0.7, colour: 'road' },
  minor: { width: 1.0, colour: 'road' },
  major: { width: 1.6, colour: 'roadMajor' },
  motorway: { width: 2.2, colour: 'roadMajor' },
};

const FILL_COLOUR: Record<FillClass, keyof MapColours> = {
  green: 'green',
  built: 'built',
  bare: 'bare',
  water: 'sea',
  building: 'building',
};

/** Canvas paths for a decoded tile, built once per tile, in tile units. */
const PATHS = new WeakMap<MapTileData, Map<string, Path2D>>();

function pathsFor(d: MapTileData): Map<string, Path2D> {
  let m = PATHS.get(d);
  if (m) return m;
  m = new Map();
  for (const c of FILL_CLASSES) {
    if (!d.fills[c].length) continue;
    const p = new Path2D();
    for (const poly of d.fills[c]) {
      for (const ring of poly) {
        p.moveTo(ring[0], ring[1]);
        for (let i = 2; i < ring.length; i += 2) p.lineTo(ring[i], ring[i + 1]);
        p.closePath();
      }
    }
    m.set(`fill:${c}`, p);
  }
  for (const c of LINE_CLASSES) {
    if (!d.lines[c].length) continue;
    const p = new Path2D();
    for (const l of d.lines[c]) {
      p.moveTo(l[0], l[1]);
      for (let i = 2; i < l.length; i += 2) p.lineTo(l[i], l[i + 1]);
    }
    m.set(`line:${c}`, p);
  }
  PATHS.set(d, m);
  return m;
}

export class MapRenderer {
  private readonly canvas: HTMLCanvasElement;
  private readonly ctx: CanvasRenderingContext2D;
  private readonly loader: TileLoader;
  private readonly resize: ResizeObserver;
  private readonly unlisten: (() => void)[] = [];
  private colours: MapColours;
  private landPath: Path2D | null = null;
  private cam: MapCamera = { lat: 57, lon: -2, zoom: 6 };
  private width = 1;
  private height = 1;
  private dpr = 1;
  private frame = 0;
  private flight: { from: MapCamera; to: MapCamera; start: number; ms: number } | null = null;
  private readonly pointers = new Map<number, { x: number; y: number }>();
  private pinch: number | null = null;
  private settleTimer = 0;
  private lastState: LoaderState = 'online';
  private disposed = false;

  private readonly host: HTMLElement;
  private readonly markerLayer: HTMLElement;
  private readonly cb: MapCallbacks;

  constructor(host: HTMLElement, markerLayer: HTMLElement, land: OutlineFile | null, cb: MapCallbacks) {
    this.host = host;
    this.markerLayer = markerLayer;
    this.cb = cb;
    this.canvas = document.createElement('canvas');
    this.canvas.style.display = 'block';
    this.canvas.style.width = '100%';
    this.canvas.style.height = '100%';
    this.canvas.style.touchAction = 'none';
    this.canvas.setAttribute('aria-hidden', 'true');
    host.appendChild(this.canvas);
    const ctx = this.canvas.getContext('2d');
    if (!ctx) throw new Error('map: no 2D canvas');
    this.ctx = ctx;
    this.colours = readColours(host);
    this.setLand(land);
    this.loader = new TileLoader({
      fetch: (url, init) => fetch(url, init),
      cacheEntries: 192,
      online: () => navigator.onLine !== false,
      onChange: () => {
        if (this.loader.state !== this.lastState) {
          this.lastState = this.loader.state;
          this.cb.onState(this.lastState);
        }
        this.requestRender();
      },
    });
    const online = () => this.requestRender();
    window.addEventListener('online', online);
    this.unlisten.push(() => window.removeEventListener('online', online));
    this.resize = new ResizeObserver(() => this.onResize());
    this.resize.observe(host);
    this.onResize();
    this.listen();
  }

  get camera(): MapCamera {
    return this.cam;
  }

  get state(): LoaderState {
    return this.loader.state;
  }

  get viewportHeight(): number {
    return this.height;
  }

  /** The bundled Natural Earth land, as one path in zoom-0 world pixels. */
  setLand(land: OutlineFile | null): void {
    if (!land?.land) {
      this.landPath = null;
      return;
    }
    const p = new Path2D();
    for (const poly of land.land) {
      for (const ring of poly) {
        const pts = decodeLine(ring, land.quantum);
        for (let i = 0; i < pts.length; i += 2) {
          const x = lonToTileX(pts[i], 0) * 256;
          const y = latToTileY(pts[i + 1], 0) * 256;
          if (i === 0) p.moveTo(x, y);
          else p.lineTo(x, y);
        }
        p.closePath();
      }
    }
    this.landPath = p;
    this.requestRender();
  }

  refreshColours(): void {
    this.colours = readColours(this.host);
    this.requestRender();
  }

  /** Where a position is on the map, in CSS pixels from the canvas's corner. */
  project(lat: number, lon: number): { x: number; y: number } {
    return project(lat, lon, this.cam, this.width, this.height);
  }

  flyTo(target: MapCamera, reducedMotion: boolean): void {
    const to = clampMapCamera(target);
    if (reducedMotion) {
      this.flight = null;
      this.cam = to;
      this.moved(true);
      return;
    }
    this.flight = { from: this.cam, to, start: performance.now(), ms: 700 };
    this.requestRender();
  }

  /** Keyboard and button pan and zoom, about the centre. */
  nudge(dxPx: number, dyPx: number, dZoom = 0): void {
    this.flight = null;
    let c = panMap(this.cam, dxPx, dyPx, this.width, this.height);
    if (dZoom) c = zoomMapAt(c, c.zoom + dZoom, this.width / 2, this.height / 2, this.width, this.height);
    this.cam = c;
    this.moved(false);
    this.settleSoon();
  }

  dispose(): void {
    this.disposed = true;
    cancelAnimationFrame(this.frame);
    window.clearTimeout(this.settleTimer);
    this.resize.disconnect();
    for (const u of this.unlisten) u();
    this.loader.destroy();
    this.canvas.remove();
  }

  // --- drawing --------------------------------------------------------------------

  requestRender(): void {
    if (this.frame || this.disposed) return;
    this.frame = requestAnimationFrame((t) => {
      this.frame = 0;
      this.tick(t);
    });
  }

  private onResize(): void {
    this.width = Math.max(1, this.host.clientWidth);
    this.height = Math.max(1, this.host.clientHeight);
    this.dpr = Math.min(window.devicePixelRatio || 1, 2);
    this.canvas.width = Math.round(this.width * this.dpr);
    this.canvas.height = Math.round(this.height * this.dpr);
    this.requestRender();
  }

  private tick(now: number): void {
    if (this.flight) {
      const f = this.flight;
      const t = Math.min(1, (now - f.start) / f.ms);
      const e = t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2;
      this.cam = clampMapCamera({
        lat: f.from.lat + (f.to.lat - f.from.lat) * e,
        lon: f.from.lon + (f.to.lon - f.from.lon) * e,
        zoom: f.from.zoom + (f.to.zoom - f.from.zoom) * e,
      });
      if (t >= 1) {
        this.flight = null;
        this.cam = f.to;
        this.cb.onCamera(this.cam, true);
      } else {
        this.cb.onCamera(this.cam, false);
        this.requestRender();
      }
    }
    this.draw();
  }

  private draw(): void {
    const g = this.ctx;
    const c = this.colours;
    const { width: w, height: h, dpr } = this;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.fillStyle = c.sea;
    g.fillRect(0, 0, w, h);

    // The bundled land, everywhere: what shows with no tiles.
    const s = 2 ** this.cam.zoom;
    const cx = lonToTileX(this.cam.lon, 0) * 256;
    const cy = latToTileY(this.cam.lat, 0) * 256;
    if (this.landPath) {
      g.setTransform(dpr * s, 0, 0, dpr * s, dpr * (w / 2 - cx * s), dpr * (h / 2 - cy * s));
      g.fillStyle = c.land;
      g.fill(this.landPath);
      g.strokeStyle = c.coast;
      g.lineWidth = 0.8 / s;
      g.stroke(this.landPath);
    }

    const view = visibleTiles(this.cam, w, h, VECTOR_MAX_ZOOM);
    const keep = new Set<string>();
    const labelled: { data: MapTileData; left: number; top: number; size: number }[] = [];
    const online = this.loader.state === 'online';
    view.tiles.forEach((v, i) => {
      const url = tileUrl('vector', v.tile);
      keep.add(url);
      if (online && this.loader.status(url) === 'none') {
        const z = v.tile.z;
        this.loader.request(url, i, async (b) => extractFeatures(new VectorTile(new PbfReader(new Uint8Array(b))), z));
      }
      // The tile itself, or the nearest loaded ancestor standing in for it.
      let t: Tile | null = v.tile;
      for (let up = 0; t && up <= ANCESTOR_LEVELS; up++, t = parent(t)) {
        const u = tileUrl('vector', t);
        const d = this.loader.get<MapTileData>(u);
        if (d === undefined) continue;
        keep.add(u);
        if (d === null) break;
        const k = 2 ** up;
        const size = view.size * k;
        const left = v.left - (v.tile.x - t.x * k) * view.size;
        const top = v.top - (v.tile.y - t.y * k) * view.size;
        g.save();
        g.setTransform(dpr, 0, 0, dpr, 0, 0);
        g.beginPath();
        g.rect(v.left, v.top, view.size, view.size);
        g.clip();
        this.drawTile(d, left, top, size);
        g.restore();
        if (up === 0) labelled.push({ data: d, left, top, size });
        break;
      }
    });
    this.loader.retain(keep);
    this.drawLabels(labelled);
    this.placeMarkers();
  }

  private drawTile(d: MapTileData, left: number, top: number, size: number): void {
    const g = this.ctx;
    const c = this.colours;
    const k = size / d.extent;
    const dpr = this.dpr;
    g.setTransform(dpr * k, 0, 0, dpr * k, dpr * left, dpr * top);
    g.fillStyle = c.land;
    g.fillRect(0, 0, d.extent, d.extent);
    const paths = pathsFor(d);
    for (const f of FILL_CLASSES) {
      const p = paths.get(`fill:${f}`);
      if (!p) continue;
      g.fillStyle = c[FILL_COLOUR[f]];
      g.fill(p);
    }
    g.lineCap = 'round';
    g.lineJoin = 'round';
    for (const l of LINE_CLASSES) {
      const p = paths.get(`line:${l}`);
      if (!p) continue;
      const st = LINE_STYLE[l];
      if (l === 'major' || l === 'motorway') {
        g.setLineDash([]);
        g.strokeStyle = c.roadCasing;
        g.lineWidth = (st.width + 1.6) / k;
        g.stroke(p);
      }
      g.setLineDash(st.dash ? st.dash.map((x) => x / k) : []);
      g.strokeStyle = c[st.colour];
      g.lineWidth = st.width / k;
      g.stroke(p);
    }
    g.setLineDash([]);
  }

  private drawLabels(tiles: { data: MapTileData; left: number; top: number; size: number }[]): void {
    const g = this.ctx;
    const c = this.colours;
    g.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    g.textAlign = 'center';
    g.textBaseline = 'middle';
    const all = tiles.flatMap((t) => t.data.labels.map((l) => ({ l, x: t.left + (l.x / t.data.extent) * t.size, y: t.top + (l.y / t.data.extent) * t.size })));
    all.sort((a, b) => a.l.rank - b.l.rank || b.l.population - a.l.population);
    const placed: { x0: number; y0: number; x1: number; y1: number }[] = [];
    for (const { l, x, y } of all) {
      if (x < -50 || y < -20 || x > this.width + 50 || y > this.height + 20) continue;
      const size = l.rank === 0 ? 11 : l.rank === 1 ? 10 : 9;
      g.font = `${l.rank === 0 ? 600 : 400} ${size}px ${c.font}`;
      const text = l.rank === 0 ? l.text.toUpperCase() : l.text;
      const wpx = g.measureText(text).width;
      const box = { x0: x - wpx / 2 - 4, y0: y - size, x1: x + wpx / 2 + 4, y1: y + size };
      if (placed.some((b) => b.x0 < box.x1 && b.x1 > box.x0 && b.y0 < box.y1 && b.y1 > box.y0)) continue;
      placed.push(box);
      g.lineWidth = 3;
      g.strokeStyle = c.halo;
      g.strokeText(text, x, y);
      g.fillStyle = c.label;
      g.fillText(text, x, y);
      if (placed.length > 60) break;
    }
  }

  /** Puts each of the view's marker elements (data-lat, data-lon) on its position. */
  private placeMarkers(): void {
    for (const el of Array.from(this.markerLayer.children) as HTMLElement[]) {
      const lat = Number(el.dataset.lat);
      const lon = Number(el.dataset.lon);
      if (!Number.isFinite(lat) || !Number.isFinite(lon)) continue;
      const p = this.project(lat, lon);
      const off = p.x < -40 || p.y < -40 || p.x > this.width + 40 || p.y > this.height + 40;
      el.style.visibility = off ? 'hidden' : '';
      el.style.transform = `translate(${Math.round(p.x)}px, ${Math.round(p.y)}px)`;
    }
  }

  // --- input ----------------------------------------------------------------------

  private moved(settled: boolean): void {
    this.cb.onCamera(this.cam, settled);
    this.requestRender();
  }

  private settleSoon(): void {
    window.clearTimeout(this.settleTimer);
    this.settleTimer = window.setTimeout(() => this.cb.onCamera(this.cam, true), 250);
  }

  private listen(): void {
    const el = this.canvas;
    const on = <K extends keyof HTMLElementEventMap>(type: K, fn: (e: HTMLElementEventMap[K]) => void, opts?: AddEventListenerOptions) => {
      el.addEventListener(type, fn as EventListener, opts);
      this.unlisten.push(() => el.removeEventListener(type, fn as EventListener, opts));
    };
    const local = (e: { clientX: number; clientY: number }) => {
      const r = this.host.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top };
    };
    on('pointerdown', (e) => {
      el.setPointerCapture(e.pointerId);
      this.flight = null;
      this.pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      this.pinch = this.pointers.size === 2 ? this.pinchDistance() : null;
    });
    on('pointermove', (e) => {
      const prev = this.pointers.get(e.pointerId);
      if (!prev) return;
      this.pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (this.pointers.size >= 2 && this.pinch) {
        const d = this.pinchDistance();
        const [a, b] = [...this.pointers.values()];
        const mid = local({ clientX: (a.x + b.x) / 2, clientY: (a.y + b.y) / 2 });
        if (d > 0) this.cam = zoomMapAt(this.cam, this.cam.zoom + Math.log2(d / this.pinch), mid.x, mid.y, this.width, this.height);
        this.pinch = d;
      } else {
        this.cam = panMap(this.cam, e.clientX - prev.x, e.clientY - prev.y, this.width, this.height);
      }
      this.moved(false);
    });
    const end = (e: PointerEvent) => {
      if (!this.pointers.delete(e.pointerId)) return;
      if (this.pointers.size < 2) this.pinch = null;
      this.cb.onCamera(this.cam, true);
    };
    on('pointerup', end);
    on('pointercancel', end);
    on('wheel', (e) => {
      e.preventDefault();
      this.flight = null;
      const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? this.height : 1;
      const p = local(e);
      this.cam = zoomMapAt(this.cam, this.cam.zoom - e.deltaY * unit * 0.002, p.x, p.y, this.width, this.height);
      this.moved(false);
      this.settleSoon();
    }, { passive: false });
    on('dblclick', (e) => {
      const p = local(e);
      this.cam = zoomMapAt(this.cam, this.cam.zoom + 1, p.x, p.y, this.width, this.height);
      this.moved(true);
    });
  }

  private pinchDistance(): number {
    const [a, b] = [...this.pointers.values()];
    return a && b ? Math.hypot(a.x - b.x, a.y - b.y) : 0;
  }
}

