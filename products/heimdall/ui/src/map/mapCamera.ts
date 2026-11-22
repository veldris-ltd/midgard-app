/* The 2D geographic map's camera, as data: Web Mercator at a fractional zoom, pan,
 * zoom about a point, and the vector tiles in view. Pure. [FR-H-1] */
import { CAMERA_FOV_DEG } from '../globe/camera.ts';
import { wrapLongitude } from '../globe/projection.ts';
import { MAX_MERCATOR_LAT, type Tile, latToTileY, lonToTileX, tileXToLon, tileYToLat, zoomForSpan } from '../tiles/tileMath.ts';

export interface MapCamera {
  lat: number;
  lon: number;
  zoom: number;
}

export const MAP_MIN_ZOOM = 3;
/** Two levels past the vector tiles' deepest, drawn larger. */
export const MAP_MAX_ZOOM = 16;

const TILE_PX = 256;
const RAD = Math.PI / 180;

export function clampMapCamera(c: MapCamera): MapCamera {
  const zoom = Number.isFinite(c.zoom) ? Math.min(MAP_MAX_ZOOM, Math.max(MAP_MIN_ZOOM, c.zoom)) : MAP_MIN_ZOOM;
  const lat = Number.isFinite(c.lat) ? Math.max(-MAX_MERCATOR_LAT, Math.min(MAX_MERCATOR_LAT, c.lat)) : 0;
  const lon = Number.isFinite(c.lon) ? wrapLongitude(c.lon) : 0;
  return { lat, lon, zoom };
}

/** World pixels at the camera's zoom. */
function world(lat: number, lon: number, zoom: number): { x: number; y: number } {
  return { x: lonToTileX(lon, zoom) * TILE_PX, y: latToTileY(lat, zoom) * TILE_PX };
}

export function project(lat: number, lon: number, c: MapCamera, w: number, h: number): { x: number; y: number } {
  const p = world(lat, lon, c.zoom);
  const o = world(c.lat, c.lon, c.zoom);
  return { x: w / 2 + (p.x - o.x), y: h / 2 + (p.y - o.y) };
}

export function unproject(x: number, y: number, c: MapCamera, w: number, h: number): { lat: number; lon: number } {
  const o = world(c.lat, c.lon, c.zoom);
  return {
    lat: tileYToLat((o.y + y - h / 2) / TILE_PX, c.zoom),
    lon: tileXToLon((o.x + x - w / 2) / TILE_PX, c.zoom),
  };
}

/** Drag by (dx, dy) pixels: the ground follows the pointer. */
export function panMap(c: MapCamera, dx: number, dy: number, w: number, h: number): MapCamera {
  const centre = unproject(w / 2 - dx, h / 2 - dy, c, w, h);
  return clampMapCamera({ ...centre, zoom: c.zoom });
}

/** Zoom to `zoom` keeping the ground under (px, py) where it is. */
export function zoomMapAt(c: MapCamera, zoom: number, px: number, py: number, w: number, h: number): MapCamera {
  const z = clampMapCamera({ ...c, zoom }).zoom;
  const at = unproject(px, py, c, w, h);
  const p = world(at.lat, at.lon, z);
  const centre = { x: p.x - (px - w / 2), y: p.y - (py - h / 2) };
  return clampMapCamera({ lat: tileYToLat(centre.y / TILE_PX, z), lon: tileXToLon(centre.x / TILE_PX, z), zoom: z });
}

export interface VisibleTile {
  tile: Tile;
  /** Screen position of the tile's north-west corner. */
  left: number;
  top: number;
}

/**
 * The tiles covering the viewport, at the zoom below the camera's (at most the
 * source's `maxZoom`), each `size` pixels across, nearest the centre first.
 */
export function visibleTiles(c: MapCamera, w: number, h: number, maxZoom: number): { z: number; size: number; tiles: VisibleTile[] } {
  const z = Math.max(0, Math.min(maxZoom, Math.floor(c.zoom)));
  const size = TILE_PX * 2 ** (c.zoom - z);
  const n = 2 ** z;
  const cx = lonToTileX(c.lon, z);
  const cy = latToTileY(c.lat, z);
  const x0 = Math.floor(cx - w / 2 / size);
  const x1 = Math.floor(cx + w / 2 / size);
  const y0 = Math.max(0, Math.floor(cy - h / 2 / size));
  const y1 = Math.min(n - 1, Math.floor(cy + h / 2 / size));
  const tiles: (VisibleTile & { d: number })[] = [];
  for (let y = y0; y <= y1; y++) {
    for (let x = x0; x <= x1; x++) {
      if (x < 0 || x >= n) continue;
      const left = w / 2 + (x - cx) * size;
      const top = h / 2 + (y - cy) * size;
      const d = Math.hypot(left + size / 2 - w / 2, top + size / 2 - h / 2);
      tiles.push({ tile: { z, x, y }, left, top, d });
    }
  }
  tiles.sort((a, b) => a.d - b.d);
  return { z, size, tiles: tiles.map(({ tile, left, top }) => ({ tile, left, top })) };
}

/** The map zoom that shows what the globe camera at `altKm` shows, `viewportPx` tall. */
export function zoomForAltitude(altKm: number, lat: number, viewportPx: number): number {
  const spanKm = 2 * altKm * Math.tan((CAMERA_FOV_DEG * RAD) / 2);
  return zoomForSpan(spanKm, lat, viewportPx);
}
