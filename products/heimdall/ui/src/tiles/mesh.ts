/* The globe's terrain patch for one tile, as plain arrays for a three.js geometry.
 * [FR-H-1]
 *
 * An n × n grid, evenly spaced in Web Mercator so it lines up with the tile's own
 * imagery, raised by the terrain heights times the HEIGHTS setting (below sea level
 * stays at the sea surface). A skirt hangs from the four edges so neighbouring tiles
 * at different zooms never show a crack. Positions are relative to the patch's centre,
 * which the mesh is moved to, so float32 stays precise at a 1.5 km close-up. Pure. */
import { EARTH_RADIUS_KM, type Vec3, toXyz } from '../globe/projection.ts';
import { EQUATOR_KM, type Tile, tileXToLon, tileYToLat } from './tileMath.ts';

export interface Patch {
  center: Vec3;
  positions: Float32Array;
  normals: Float32Array;
  /** Within the tile, (0, 0) at its north-west corner. */
  uv: Float32Array;
  /** Within the world: longitude and latitude scaled to 0–1 (south at 0). */
  geoUv: Float32Array;
  indices: Uint16Array | Uint32Array;
  /** The highest vertex, exaggerated, in km. */
  maxKm: number;
}

/** How far the skirt hangs below the edge: more for bigger tiles. */
export function skirtDepthKm(t: Tile): number {
  return Math.max(0.5, (EQUATOR_KM / 2 ** t.z) * 0.02);
}

export function buildPatch(t: Tile, heights: Float32Array | null, n: number, exaggeration: number): Patch {
  const side = n + 1;
  const grid = side * side;
  if (heights && heights.length !== grid) throw new Error(`mesh: expected ${grid} heights, got ${heights.length}`);
  const skirt = 4 * n;
  const count = grid + skirt;
  const lat = new Float64Array(count);
  const lon = new Float64Array(count);
  const alt = new Float64Array(count);
  const u = new Float32Array(count);
  const v = new Float32Array(count);

  let maxKm = 0;
  for (let j = 0; j <= n; j++) {
    const la = tileYToLat(t.y + j / n, t.z);
    for (let i = 0; i <= n; i++) {
      const k = j * side + i;
      lat[k] = la;
      lon[k] = tileXToLon(t.x + i / n, t.z);
      const h = heights ? Math.max(0, heights[k]) : 0;
      alt[k] = (h / 1000) * exaggeration;
      if (alt[k] > maxKm) maxKm = alt[k];
      u[k] = i / n;
      v[k] = j / n;
    }
  }

  // The edge, walked once around: north (west to east), east (north to south),
  // south (east to west), west (south to north). Each skirt vertex hangs under one.
  const ring: number[] = [];
  for (let i = 0; i < n; i++) ring.push(i);
  for (let j = 0; j < n; j++) ring.push(j * side + n);
  for (let i = n; i > 0; i--) ring.push(n * side + i);
  for (let j = n; j > 0; j--) ring.push(j * side);
  const depth = skirtDepthKm(t);
  ring.forEach((e, s) => {
    const k = grid + s;
    lat[k] = lat[e];
    lon[k] = lon[e];
    alt[k] = alt[e] - depth;
    u[k] = u[e];
    v[k] = v[e];
  });

  const centre = toXyz(tileYToLat(t.y + 0.5, t.z), tileXToLon(t.x + 0.5, t.z));
  const positions = new Float32Array(count * 3);
  const normals = new Float32Array(count * 3);
  const uv = new Float32Array(count * 2);
  const geoUv = new Float32Array(count * 2);
  for (let k = 0; k < count; k++) {
    const p = toXyz(lat[k], lon[k], alt[k]);
    positions[k * 3] = p.x - centre.x;
    positions[k * 3 + 1] = p.y - centre.y;
    positions[k * 3 + 2] = p.z - centre.z;
    const r = 1 + alt[k] / EARTH_RADIUS_KM;
    normals[k * 3] = p.x / r;
    normals[k * 3 + 1] = p.y / r;
    normals[k * 3 + 2] = p.z / r;
    uv[k * 2] = u[k];
    uv[k * 2 + 1] = v[k];
    geoUv[k * 2] = (lon[k] + 180) / 360;
    geoUv[k * 2 + 1] = (lat[k] + 90) / 180;
  }

  // Counter-clockwise seen from outside the globe, so the front faces point out.
  const tris: number[] = [];
  for (let j = 0; j < n; j++) {
    for (let i = 0; i < n; i++) {
      const a = j * side + i;
      const b = a + 1;
      const c = a + side;
      const d = c + 1;
      tris.push(a, c, b, b, c, d);
    }
  }
  for (let s = 0; s < skirt; s++) {
    const e0 = ring[s];
    const e1 = ring[(s + 1) % skirt];
    const s0 = grid + s;
    const s1 = grid + ((s + 1) % skirt);
    tris.push(e0, s0, e1, e1, s0, s1);
  }
  const indices = count > 65535 ? new Uint32Array(tris) : new Uint16Array(tris);
  return { center: centre, positions, normals, uv, geoUv, indices, maxKm };
}
