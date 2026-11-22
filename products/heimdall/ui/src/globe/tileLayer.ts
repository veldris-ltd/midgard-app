/* What the globe renderer needs from a map-tile layer, as types only. [FR-H-1]
 *
 * The renderer draws with or without one. The layer itself (src/tiles/globeTiles.ts)
 * is loaded only in a build with map tiles (`VITE_HEIMDALL_TILE_SOURCE` mirror or
 * public), so in a `none` build the renderer, and the bundle, carry nothing that
 * fetches tiles. [NFR-PRV-1] */
import type * as THREE from 'three';

import type { CameraState } from './camera.ts';

export type GlobeStyle = 'clean' | 'topographic' | 'satellite';
export const GLOBE_STYLES: readonly GlobeStyle[] = ['clean', 'topographic', 'satellite'];

/** What the renderer shares with the layer, so tiles shade and colour like the globe. */
export interface TileLayerHost {
  /** Towards the sun, updated in place. */
  sunDir: THREE.Vector3;
  /** The night side's brightness. */
  night: { value: number };
  /** The clean style's land and sea, an equirectangular texture. */
  landTexture: THREE.Texture;
  requestRender(): void;
}

export interface GlobeTileLayer {
  readonly object: THREE.Object3D;
  /** Online and drawing tiles; false falls the globe back to its own sphere. */
  readonly active: boolean;
  /** The tile hosts answer, whether or not any tile is drawn yet. */
  readonly online: boolean;
  /** Picks, fetches and swaps tiles for this camera; called once a frame. */
  update(camera: CameraState, viewportPx: number, aspect: number): void;
  /** The ground under a point, km above sea level times HEIGHTS; null when not loaded. */
  groundKm(lat: number, lon: number): number | null;
  setExaggeration(k: number): void;
  setStyle(style: GlobeStyle): void;
  /** Paused, the layer requests no tile (the public build, after a track-driven move). */
  setPaused(paused: boolean): void;
  /** The console's mode, for palettes drawn by the layer. */
  setDark(dark: boolean): void;
  dispose(): void;
}
