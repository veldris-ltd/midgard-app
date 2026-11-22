/* What the globe renderer needs from a map-tile layer, as types only. [FR-H-1]
 *
 * The renderer draws with or without one. The layer itself (src/tiles/globeTiles.ts)
 * is loaded only behind the tiles switch, so with the switch off the renderer, and
 * the bundle, carry nothing that fetches tiles. */
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
  /** Picks, fetches and swaps tiles for this camera; called once a frame. */
  update(camera: CameraState, viewportPx: number, aspect: number): void;
  /** The ground under a point, km above sea level times HEIGHTS; null when not loaded. */
  groundKm(lat: number, lon: number): number | null;
  setExaggeration(k: number): void;
  setStyle(style: GlobeStyle): void;
  dispose(): void;
}
