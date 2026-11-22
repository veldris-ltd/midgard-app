/* The globe's map-tile layer: terrain patches for the area in view. [FR-H-1]
 *
 * Each frame it picks tiles for the camera (`selectGlobeTiles`, within the budget),
 * asks the loader for their data, cancels what the view no longer needs, and swaps the
 * patches in. A patch is built once its data has arrived: terrain heights (Mapzen
 * terrarium tiles, from zoom 6 down; flatter tiles stay flat) raised by HEIGHTS, and
 * the style's colours. When the loader goes offline the layer stops drawing and the
 * globe falls back to its own sphere.
 *
 * Reached only through the tiles switch: this module is never in a build with
 * `VITE_HEIMDALL_TILES` off. */
import * as THREE from 'three';

import { CAMERA_FOV_DEG, type CameraState } from '../globe/camera.ts';
import type { GlobeStyle, GlobeTileLayer, TileLayerHost } from '../globe/tileLayer.ts';
import { tileUrl } from './hosts.ts';
import { type LoaderState, TileLoader } from './loader.ts';
import { LruCache } from './cache.ts';
import { buildPatch } from './mesh.ts';
import { selectGlobeTiles } from './select.ts';
import { type HeightGrid, decodeTerrarium, heightAt, resampleGrid } from './terrarium.ts';
import { type Tile, ancestorAt, lonToTileX, latToTileY, subRect, tileAt, tileKey } from './tileMath.ts';

/** Terrain heights are fetched to this zoom; finer patches sample their ancestor. */
const TERRAIN_MAX_ZOOM = 12;
/** Patches coarser than this are flat: relief does not show from that far out. */
const TERRAIN_MIN_ZOOM = 6;
const MIN_ZOOM = 2;
const MAX_ZOOM = 14;
/** Most tiles drawn plus wanted at once. */
export const TILE_BUDGET = 160;
const SEGMENTS = 32;
/** Patches built per frame, so a burst of arrivals never stalls a frame. */
const BUILDS_PER_FRAME = 12;

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

/** Decodes a terrarium PNG in the browser, with no colour management. */
async function decodeTerrainPng(bytes: ArrayBuffer): Promise<HeightGrid> {
  const bmp = await createImageBitmap(new Blob([bytes], { type: 'image/png' }), {
    colorSpaceConversion: 'none',
    premultiplyAlpha: 'none',
  });
  try {
    const { width, height } = bmp;
    const canvas: OffscreenCanvas | HTMLCanvasElement =
      typeof OffscreenCanvas !== 'undefined' ? new OffscreenCanvas(width, height) : Object.assign(document.createElement('canvas'), { width, height });
    const ctx = canvas.getContext('2d', { willReadFrequently: true }) as CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null;
    if (!ctx) throw new Error('terrain: no 2D canvas');
    ctx.drawImage(bmp, 0, 0);
    return decodeTerrarium(ctx.getImageData(0, 0, width, height).data, width, height);
  } finally {
    bmp.close();
  }
}

export class GlobeTiles implements GlobeTileLayer {
  readonly object = new THREE.Group();
  private readonly host: TileLayerHost;
  private readonly loader: TileLoader;
  private readonly meshes: LruCache<THREE.Mesh>;
  private readonly blank: THREE.DataTexture;
  private readonly onState?: (state: LoaderState) => void;
  private readonly onOnline = () => this.host.requestRender();
  private style: GlobeStyle = 'clean';
  private exaggeration = 4;
  private drawn = new Set<string>();
  private lastState: LoaderState = 'online';
  private builds = 0;

  constructor(host: TileLayerHost, o: GlobeTilesOptions = {}) {
    this.host = host;
    this.onState = o.onState;
    const f = o.fetch ?? ((url: string, init: RequestInit) => fetch(url, init));
    this.loader = new TileLoader({
      fetch: f,
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

  get state(): LoaderState {
    return this.loader.state;
  }

  setStyle(style: GlobeStyle): void {
    if (style === this.style) return;
    this.style = style;
    this.rebuild();
  }

  setExaggeration(k: number): void {
    if (k === this.exaggeration) return;
    this.exaggeration = k;
    this.rebuild();
  }

  groundKm(lat: number, lon: number): number | null {
    for (let z = TERRAIN_MAX_ZOOM; z >= TERRAIN_MIN_ZOOM; z--) {
      const t = tileAt(lat, lon, z);
      const grid = this.terrain(t);
      if (grid === undefined) continue;
      if (grid === null) return 0;
      const h = heightAt(grid, lonToTileX(lon, z) - t.x, latToTileY(lat, z) - t.y);
      return (Math.max(0, h) / 1000) * this.exaggeration;
    }
    return null;
  }

  /** URLs in use by drawn tiles: kept in the cache and never cancelled. */
  private inUse = new Set<string>();

  update(cam: CameraState, viewportPx: number, aspect: number): void {
    this.builds = 0;
    if (this.loader.state === 'offline') {
      // Keep asking for the tile under the camera, so the loader can probe for the
      // network coming back; draw nothing until it does.
      this.object.visible = false;
      this.drawn = new Set();
      this.requestData(tileAt(cam.lat, cam.lon, TERRAIN_MIN_ZOOM), 0);
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

    const keep = new Set<string>();
    this.inUse = new Set();
    for (const t of sel.draw) for (const u of this.urlsFor(t)) this.inUse.add(u);
    sel.want.forEach((t, i) => {
      for (const u of this.requestData(t, i)) keep.add(u);
    });
    for (const u of this.inUse) keep.add(u);
    this.loader.retain(keep);
    // Data waiting to become patches: come back next frame.
    if (this.builds >= BUILDS_PER_FRAME) this.host.requestRender();
  }

  dispose(): void {
    window.removeEventListener('online', this.onOnline);
    this.loader.destroy();
    this.object.clear();
    this.meshes.protect = () => false;
    this.meshes.clear();
    this.blank.dispose();
  }

  // --- data -----------------------------------------------------------------------

  private needsTerrain(t: Tile): boolean {
    return t.z >= TERRAIN_MIN_ZOOM;
  }

  private terrainTile(t: Tile): Tile {
    return ancestorAt(t, Math.min(t.z, TERRAIN_MAX_ZOOM));
  }

  private urlsFor(t: Tile): string[] {
    const urls: string[] = [];
    if (this.needsTerrain(t)) urls.push(tileUrl('terrain', this.terrainTile(t)));
    return urls;
  }

  /** The decoded terrain tile: a grid, null when the source has none, or undefined. */
  private terrain(t: Tile): HeightGrid | null | undefined {
    return this.loader.get<HeightGrid>(tileUrl('terrain', t));
  }

  /** Asks for what a tile needs; returns the URLs. */
  private requestData(t: Tile, priority: number): string[] {
    const urls = this.urlsFor(t);
    for (const u of urls) {
      if (this.loader.status(u) !== 'none') continue;
      this.loader.request(u, priority, decodeTerrainPng);
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
    let heights: Float32Array | null = null;
    if (this.needsTerrain(t)) {
      const tt = this.terrainTile(t);
      const grid = this.terrain(tt);
      if (grid === undefined) return false;
      if (grid) heights = resampleGrid(grid, subRect(t, tt.z), SEGMENTS);
    }
    if (this.builds >= BUILDS_PER_FRAME) return false;
    this.builds++;
    this.meshes.set(key, this.buildMesh(t, heights));
    return true;
  }

  private buildMesh(t: Tile, heights: Float32Array | null): THREE.Mesh {
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
        map: { value: this.blank },
        land: { value: this.host.landTexture },
        useLand: { value: 1 },
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
    if (map !== this.blank) map.dispose();
    mat.dispose();
  }

  private rebuild(): void {
    this.object.clear();
    this.drawn = new Set();
    this.meshes.protect = () => false;
    this.meshes.clear();
    this.meshes.protect = (k) => this.drawn.has(k);
    this.host.requestRender();
  }
}
