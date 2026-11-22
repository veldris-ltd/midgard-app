/* Web Mercator tile maths ("slippy map" tiles, EPSG:3857, 256 px), shared by the
 * globe's terrain and imagery tiles and the 2D geographic map. [FR-H-1]
 *
 * x runs west to east and y north to south, each from 0 to 2^z at zoom z; positions
 * are fractional tile coordinates. Pure and dependency-free, so node:test runs it. */

export interface Tile {
  z: number;
  x: number;
  y: number;
}

export interface TileBounds {
  west: number;
  east: number;
  north: number;
  south: number;
}

/** The latitude at which Web Mercator's square world ends. */
export const MAX_MERCATOR_LAT = 85.0511287798066;
/** Equatorial circumference of the WGS 84 ellipsoid, the Mercator scale, in km. */
export const EQUATOR_KM = 40075.016685578485;
/** The deepest zoom any tile source here serves. */
const MAX_TILE_ZOOM = 22;

const RAD = Math.PI / 180;

export function lonToTileX(lon: number, z: number): number {
  return ((lon + 180) / 360) * 2 ** z;
}

export function latToTileY(lat: number, z: number): number {
  const l = Math.max(-MAX_MERCATOR_LAT, Math.min(MAX_MERCATOR_LAT, lat)) * RAD;
  return ((1 - Math.log(Math.tan(l) + 1 / Math.cos(l)) / Math.PI) / 2) * 2 ** z;
}

export function tileXToLon(x: number, z: number): number {
  return (x / 2 ** z) * 360 - 180;
}

export function tileYToLat(y: number, z: number): number {
  const n = Math.PI - (2 * Math.PI * y) / 2 ** z;
  return Math.atan(Math.sinh(n)) / RAD;
}

/** The tile at zoom `z` holding a point; edges and poles land in the last row or column. */
export function tileAt(lat: number, lon: number, z: number): Tile {
  const n = 2 ** z;
  const x = Math.min(n - 1, Math.max(0, Math.floor(lonToTileX(lon, z))));
  const y = Math.min(n - 1, Math.max(0, Math.floor(latToTileY(lat, z))));
  return { z, x, y };
}

export function tileBounds(t: Tile): TileBounds {
  return {
    west: tileXToLon(t.x, t.z),
    east: tileXToLon(t.x + 1, t.z),
    north: tileYToLat(t.y, t.z),
    south: tileYToLat(t.y + 1, t.z),
  };
}

export function tileKey(t: Tile): string {
  return `${t.z}/${t.x}/${t.y}`;
}

export function validTile(t: Tile): boolean {
  if (!Number.isInteger(t.z) || t.z < 0 || t.z > MAX_TILE_ZOOM) return false;
  const n = 2 ** t.z;
  return Number.isInteger(t.x) && Number.isInteger(t.y) && t.x >= 0 && t.y >= 0 && t.x < n && t.y < n;
}

export function parent(t: Tile): Tile | null {
  return t.z === 0 ? null : { z: t.z - 1, x: t.x >> 1, y: t.y >> 1 };
}

/** The four children, north-west, north-east, south-west, south-east. */
export function children(t: Tile): Tile[] {
  const z = t.z + 1;
  const x = t.x * 2;
  const y = t.y * 2;
  return [{ z, x, y }, { z, x: x + 1, y }, { z, x, y: y + 1 }, { z, x: x + 1, y: y + 1 }];
}

/** The tile's ancestor at zoom `z`, or the tile itself when `z` is not above it. */
export function ancestorAt(t: Tile, z: number): Tile {
  if (z >= t.z) return t;
  const d = t.z - z;
  return { z, x: t.x >> d, y: t.y >> d };
}

/** Where `t` sits inside its ancestor at zoom `z`, in fractions of that ancestor. */
export function subRect(t: Tile, z: number): { ox: number; oy: number; size: number } {
  if (z >= t.z) return { ox: 0, oy: 0, size: 1 };
  const n = 2 ** (t.z - z);
  const a = ancestorAt(t, z);
  return { ox: t.x / n - a.x, oy: t.y / n - a.y, size: 1 / n };
}

/** Ground metres per pixel of a 256 px tile at zoom `z` and latitude `lat`. */
export function metresPerPixel(lat: number, z: number, tilePx = 256): number {
  return (EQUATOR_KM * 1000 * Math.cos(lat * RAD)) / (tilePx * 2 ** z);
}

/** The (fractional) zoom at which `spanKm` of ground at `lat` fills `viewportPx` pixels. */
export function zoomForSpan(spanKm: number, lat: number, viewportPx: number, tilePx = 256): number {
  return Math.log2((EQUATOR_KM * Math.cos(lat * RAD) * viewportPx) / (tilePx * spanKm));
}
