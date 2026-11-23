/* The globe's map-tile layer: terrain patches for the area in view, in one of three
 * styles. [FR-H-1]
 *
 * Each frame it picks tiles for the camera (`selectGlobeTiles`, within the budget),
 * asks the loader for their data, cancels what the view no longer needs, and swaps the
 * patches in. A patch is built once its data has arrived: terrain heights (Mapzen
 * terrarium tiles, from zoom 6 down; flatter tiles stay flat) raised by HEIGHTS, and
 * the style's colours:
 * - CLEAN: the 1a globe's flat land and sea, from the design tokens;
 * - TOPOGRAPHIC: tints, hillshade and contours drawn from the terrain tiles;
 * - SATELLITE: EOX Sentinel-2 cloudless 2016 imagery (the clean colours where a tile
 *   has none).
 * When the loader goes offline the layer stops drawing and the globe falls back to
 * its own sphere.
 *
 * Tiles are decoded in the decode worker (decodePool.ts), within its limits. In the
 * `public` build the loader is paused after a camera move driven by the tracks
 * (`setPaused`, src/tilePause.ts).
 *
 * Reached only in a build with map tiles: this module is never in a `none` build.
 * [NFR-PRV-1] */
import * as THREE from 'three';

import { CAMERA_FOV_DEG, type CameraState } from '../globe/camera.ts';
import type { GlobeStyle, GlobeTileLayer, TileLayerHost } from '../globe/tileLayer.ts';
import { TILES } from './active.ts';
import { type DecodePool, browserDecodePool } from './decodePool.ts';
import { type LoaderState, TileLoader } from './loader.ts';
import { TOPO_DARK, TOPO_LIGHT, contourIntervalM, shadeZFactor, topoPixels } from './topo.ts';
import { LruCache } from './cache.ts';
import { buildPatch, drawnGroundM } from './mesh.ts';
import { selectGlobeTiles } from './select.ts';
import { type HeightGrid, resampleGrid } from './terrarium.ts';
import { type Tile, ancestorAt, lonToTileX, latToTileY, metresPerPixel, subRect, tileAt, tileKey, tileYToLat } from './tileMath.ts';

/** Terrain heights are fetched to this zoom; finer patches sample their ancestor. */
const TERRAIN_MAX_ZOOM = 13;
/** Patches coarser than this are flat: relief does not show from that far out. */
const TERRAIN_MIN_ZOOM = 6;
const MIN_ZOOM = 2;
const MAX_ZOOM = 14;
/** Most tiles drawn plus wanted at once. */
const TILE_BUDGET = 160;
const SEGMENTS = 32;
/** Time spent building patches per frame, so a burst of arrivals never stalls one. */
const BUILD_MS_PER_FRAME = 10;
/** The topographic style's texture size. */
const TOPO_PX = 256;

const PATCH_VERTEX = /* glsl */ `
  attribute vec2 geoUv;
  varying vec2 vUv;
  varying vec2 vGeo;
  varying vec3 vNormalW;
  void main() {
    vUv = uv;
    vGeo = geoUv;
    vNormalW = normalize(mat3(modelMatrix) * normal);
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  }
`;

const PATCH_FRAGMENT = /* glsl */ `
  uniform sampler2D map;
  uniform sampler2D land;
  uniform float useLand;
  uniform vec3 sunDir;
  uniform float night;
  varying vec2 vUv;
  varying vec2 vGeo;
  varying vec3 vNormalW;
  void main() {
    vec3 base = mix(texture2D(map, vUv).rgb, texture2D(land, vGeo).rgb, useLand);
    float d = dot(normalize(vNormalW), sunDir);
    float light = mix(night, 1.0, smoothstep(-0.12, 0.12, d));
    gl_FragColor = vec4(base * light, 1.0);
    #include <colorspace_fragment>
  }
`;

export interface GlobeTilesOptions {
  fetch?: typeof fetch;
  /** The loader went online or offline. */
  onState?: (state: LoaderState) => void;
}

export class GlobeTiles implements GlobeTileLayer {
  readonly object = new THREE.Group();
  private readonly host: TileLayerHost;
  private readonly loader: TileLoader;
  private readonly pool: DecodePool;
  private readonly meshes: LruCache<THREE.Mesh>;
  private readonly blank: THREE.DataTexture;
  private readonly onState?: (state: LoaderState) => void;
  private readonly onOnline = () => this.host.requestRender();
  private style: GlobeStyle = 'clean';
  private exaggeration = 4;
  private drawn = new Set<string>();
  /** The tiles drawn this frame, for the ground under a point. */
  private drawnTiles: Tile[] = [];
  /** Live textures per imagery bitmap, and bitmaps the cache dropped while in use. */
  private readonly bitmapUsers = new Map<ImageBitmap, number>();
  private readonly closeWhenUnused = new Set<ImageBitmap>();
  private lastState: LoaderState = 'online';
  private dark = true;
  private buildUntil = 0;
  private starved = false;

  constructor(host: TileLayerHost, o: GlobeTilesOptions = {}) {
    this.host = host;
    this.onState = o.onState;
    const f = o.fetch ?? ((url: string, init: RequestInit) => fetch(url, init));
    this.pool = browserDecodePool();
    this.loader = new TileLoader({
      fetch: f,
      tiles: TILES,
      online: () => (typeof navigator === 'undefined' ? true : navigator.onLine !== false),
      onChange: () => this.changed(),
    });
    this.meshes = new LruCache<THREE.Mesh>(TILE_BUDGET * 2, (_k, m) => this.disposeMesh(m));
    this.meshes.protect = (k) => this.drawn.has(k);
    this.loader.protect = (url) => this.inUse.has(url);
    this.blank = new THREE.DataTexture(new Uint8Array([0, 0, 0, 255]), 1, 1);
    this.blank.needsUpdate = true;
    window.addEventListener('online', this.onOnline);
  }

  get active(): boolean {
    return this.loader.state === 'online' && this.drawn.size > 0;
  }

  get online(): boolean {
    return this.loader.state === 'online';
  }

  get state(): LoaderState {
    return this.loader.state;
  }

  /** Paused, no tile is requested; tiles already loaded are still drawn. */
  setPaused(paused: boolean): void {
    this.loader.setPaused(paused);
  }

  setStyle(style: GlobeStyle): void {
    if (style === this.style) return;
    this.style = style;
    this.rebuild();
  }

  /** The console's mode, for the topographic palette. */
  setDark(dark: boolean): void {
    if (dark === this.dark) return;
    this.dark = dark;
    if (this.style === 'topographic') this.rebuild();
  }

  setExaggeration(k: number): void {
    if (k === this.exaggeration) return;
    this.exaggeration = k;
    this.rebuild();
  }

  /**
   * The ground under a point as drawn: from the patch actually drawn there, at least
   * the surface it draws between its vertices (`drawnGroundM`). Null where no patch
   * is drawn.
   */
  groundKm(lat: number, lon: number): number | null {
    for (const t of this.drawnTiles) {
      const u = lonToTileX(lon, t.z) - t.x;
      const v = latToTileY(lat, t.z) - t.y;
      if (u < 0 || u > 1 || v < 0 || v > 1) continue;
      if (t.z < TERRAIN_MIN_ZOOM) return 0;
      const tt = this.terrainTile(t);
      const grid = this.terrain(tt);
      if (grid === undefined) return null;
      return (drawnGroundM(grid, subRect(t, tt.z), SEGMENTS, u, v) / 1000) * this.exaggeration;
    }
    return null;
  }

  /** URLs in use by drawn tiles: kept in the cache and never cancelled. */
  private inUse = new Set<string>();

  update(cam: CameraState, viewportPx: number, aspect: number): void {
    this.buildUntil = performance.now() + BUILD_MS_PER_FRAME;
    this.starved = false;
    if (this.loader.state === 'offline') {
      // Keep asking for the tile under the camera, so the loader can probe for the
      // network coming back; draw nothing until it does.
      this.object.visible = false;
      this.drawn = new Set();
      this.drawnTiles = [];
      this.inUse = new Set();
      // Only the probe stays queued; whatever the last online frame wanted is dropped.
      this.loader.retain(new Set(this.requestData(tileAt(cam.lat, cam.lon, TERRAIN_MIN_ZOOM), 0)));
      return;
    }
    const memo = new Map<string, boolean>();
    const ready = (t: Tile) => {
      const k = tileKey(t);
      let r = memo.get(k);
      if (r === undefined) {
        r = this.ensureMesh(t);
        memo.set(k, r);
      }
      return r;
    };
    const sel = selectGlobeTiles(cam, { minZoom: MIN_ZOOM, maxZoom: MAX_ZOOM, budget: TILE_BUDGET, viewportPx, aspect, fovDeg: CAMERA_FOV_DEG }, ready);

    const drawn = new Set(sel.draw.map(tileKey));
    this.object.clear();
    for (const t of sel.draw) {
      const m = this.meshes.get(tileKey(t));
      if (m) this.object.add(m);
    }
    this.object.visible = true;
    this.drawn = drawn;
    this.drawnTiles = sel.draw;

    const keep = new Set<string>();
    this.inUse = new Set();
    for (const t of sel.draw) for (const u of this.urlsFor(t)) this.inUse.add(u);
    sel.want.forEach((t, i) => {
      for (const u of this.requestData(t, i)) keep.add(u);
    });
    for (const u of this.inUse) keep.add(u);
    this.loader.retain(keep);
    // Data waiting to become patches: come back next frame.
    if (this.starved) this.host.requestRender();
  }

  dispose(): void {
    window.removeEventListener('online', this.onOnline);
    this.loader.destroy();
    this.pool.dispose();
    this.object.clear();
    this.meshes.protect = () => false;
    this.meshes.clear();
    this.blank.dispose();
  }

  // --- data -----------------------------------------------------------------------

  /** Terrain for the relief from zoom 6, and for the topographic colours at any zoom. */
  private needsTerrain(t: Tile): boolean {
    return t.z >= TERRAIN_MIN_ZOOM || this.style === 'topographic';
  }

  private needsImagery(): boolean {
    return this.style === 'satellite';
  }

  private terrainTile(t: Tile): Tile {
    return ancestorAt(t, Math.min(t.z, TERRAIN_MAX_ZOOM));
  }

  private urlsFor(t: Tile): string[] {
    const urls: string[] = [];
    if (this.needsTerrain(t)) urls.push(TILES.url('terrain', this.terrainTile(t)));
    if (this.needsImagery()) urls.push(TILES.url('imagery', t));
    return urls;
  }

  /** The decoded terrain tile: a grid, null when the source has none, or undefined. */
  private terrain(t: Tile): HeightGrid | null | undefined {
    return this.loader.get<HeightGrid>(TILES.url('terrain', t));
  }

  /** Asks for what a tile needs; returns the URLs. */
  private requestData(t: Tile, priority: number): string[] {
    const urls = this.urlsFor(t);
    for (const u of urls) {
      // The loader holds back a failed tile until its retry time, and probes when offline.
      const st = this.loader.status(u);
      if (st !== 'none' && st !== 'failed') continue;
      if (u === TILES.url('terrain', this.terrainTile(t))) this.loader.request(u, priority, (b) => this.pool.decode<HeightGrid>('terrain', b));
      else this.loader.request(u, priority, (b) => this.pool.decode<ImageBitmap>('imagery', b), (b) => this.releaseBitmap(b, true));
    }
    return urls;
  }

  private changed(): void {
    const s = this.loader.state;
    if (s !== this.lastState) {
      this.lastState = s;
      this.onState?.(s);
    }
    this.host.requestRender();
  }

  // --- patches --------------------------------------------------------------------

  /** True when the tile's patch exists or could be built now from loaded data. */
  private ensureMesh(t: Tile): boolean {
    const key = tileKey(t);
    if (this.meshes.has(key)) return true;
    let grid: HeightGrid | null = null;
    if (this.needsTerrain(t)) {
      const g = this.terrain(this.terrainTile(t));
      if (g === undefined) return false;
      grid = g;
    }
    let image: ImageBitmap | null = null;
    if (this.needsImagery()) {
      const b = this.loader.get<ImageBitmap>(TILES.url('imagery', t));
      if (b === undefined) return false;
      image = b;
    }
    if (performance.now() > this.buildUntil) {
      this.starved = true;
      return false;
    }
    const rect = subRect(t, this.terrainTile(t).z);
    const heights = grid && t.z >= TERRAIN_MIN_ZOOM ? resampleGrid(grid, rect, SEGMENTS) : null;
    let map: THREE.Texture | null = null;
    if (this.style === 'topographic' && grid) map = this.topoTexture(t, grid);
    else if (this.style === 'satellite' && image) map = this.imageTexture(image);
    this.meshes.set(key, this.buildMesh(t, heights, map));
    return true;
  }

  private topoTexture(t: Tile, grid: HeightGrid): THREE.Texture {
    const rect = subRect(t, this.terrainTile(t).z);
    const n = TOPO_PX;
    let h: Float32Array;
    if (rect.size === 1 && grid.width === n && grid.height === n) {
      h = grid.data;
    } else {
      // Pixel centres of the finer tile, sampled from its ancestor.
      h = new Float32Array(n * n);
      const inner = resampleGrid(grid, { ox: rect.ox + rect.size / (2 * n), oy: rect.oy + rect.size / (2 * n), size: rect.size * (1 - 1 / n) }, n - 1);
      h.set(inner);
    }
    const lat = tileYToLat(t.y + 0.5, t.z);
    const px = topoPixels(h, n, n, metresPerPixel(lat, t.z, n), contourIntervalM(t.z), this.dark ? TOPO_DARK : TOPO_LIGHT, shadeZFactor(t.z));
    const tex = new THREE.DataTexture(px, n, n, THREE.RGBAFormat);
    this.finishTexture(tex);
    return tex;
  }

  private imageTexture(image: ImageBitmap): THREE.Texture {
    const tex = new THREE.Texture(image);
    this.bitmapUsers.set(image, (this.bitmapUsers.get(image) ?? 0) + 1);
    this.finishTexture(tex);
    return tex;
  }

  private finishTexture(tex: THREE.Texture): void {
    // Row 0 is the tile's north edge, as the patch's uv expects.
    tex.flipY = false;
    tex.colorSpace = THREE.SRGBColorSpace;
    tex.magFilter = THREE.LinearFilter;
    tex.minFilter = THREE.LinearMipmapLinearFilter;
    tex.generateMipmaps = true;
    tex.anisotropy = 4;
    tex.wrapS = THREE.ClampToEdgeWrapping;
    tex.wrapT = THREE.ClampToEdgeWrapping;
    tex.needsUpdate = true;
  }

  private buildMesh(t: Tile, heights: Float32Array | null, map: THREE.Texture | null): THREE.Mesh {
    const p = buildPatch(t, heights, SEGMENTS, this.exaggeration);
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(p.positions, 3));
    geo.setAttribute('normal', new THREE.BufferAttribute(p.normals, 3));
    geo.setAttribute('uv', new THREE.BufferAttribute(p.uv, 2));
    geo.setAttribute('geoUv', new THREE.BufferAttribute(p.geoUv, 2));
    geo.setIndex(new THREE.BufferAttribute(p.indices, 1));
    geo.computeBoundingSphere();
    const mat = new THREE.ShaderMaterial({
      uniforms: {
        map: { value: map ?? this.blank },
        land: { value: this.host.landTexture },
        useLand: { value: map ? 0 : 1 },
        sunDir: { value: this.host.sunDir },
        night: this.host.night,
      },
      vertexShader: PATCH_VERTEX,
      fragmentShader: PATCH_FRAGMENT,
      side: THREE.DoubleSide,
    });
    const mesh = new THREE.Mesh(geo, mat);
    mesh.position.set(p.center.x, p.center.y, p.center.z);
    mesh.matrixAutoUpdate = false;
    mesh.updateMatrix();
    return mesh;
  }

  private disposeMesh(m: THREE.Mesh): void {
    m.removeFromParent();
    m.geometry.dispose();
    const mat = m.material as THREE.ShaderMaterial;
    const map = mat.uniforms.map.value as THREE.Texture;
    if (map !== this.blank) {
      map.dispose();
      if (typeof ImageBitmap !== 'undefined' && map.image instanceof ImageBitmap) this.releaseBitmap(map.image, false);
    }
    mat.dispose();
  }

  /**
   * Closes an imagery bitmap once nothing needs it: the loader's cache has dropped it
   * (`evicted`) and no live texture is built from it.
   */
  private releaseBitmap(b: ImageBitmap, evicted: boolean): void {
    if (!evicted) {
      const n = (this.bitmapUsers.get(b) ?? 1) - 1;
      if (n > 0) {
        this.bitmapUsers.set(b, n);
        return;
      }
      this.bitmapUsers.delete(b);
      if (this.closeWhenUnused.delete(b)) b.close();
      return;
    }
    if (this.bitmapUsers.has(b)) this.closeWhenUnused.add(b);
    else b.close();
  }

  private rebuild(): void {
    this.object.clear();
    this.drawn = new Set();
    this.drawnTiles = [];
    this.meshes.protect = () => false;
    this.meshes.clear();
    this.meshes.protect = (k) => this.drawn.has(k);
    this.host.requestRender();
  }
}
