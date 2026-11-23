/* Terrarium-encoded terrain tiles (Mapzen Terrain Tiles): heights in metres from the
 * pixels of a PNG, height = R × 256 + G + B / 256 − 32768. [FR-H-1]
 *
 * The browser decodes the PNG; these functions take its RGBA pixels. Heights below
 * sea level are kept (the tiles carry bathymetry): the topographic style colours
 * them, and the globe's mesh clamps them to the sea surface. Pure. */

export interface HeightGrid {
  width: number;
  height: number;
  /** Row-major, north row first, metres. */
  data: Float32Array;
}

/** The part of a tile to sample, in fractions of it (see `subRect`). */
export interface Rect {
  ox: number;
  oy: number;
  size: number;
}

export function decodeTerrarium(rgba: ArrayLike<number>, width: number, height: number): HeightGrid {
  const n = width * height;
  if (rgba.length < n * 4) throw new Error(`terrarium: expected ${n * 4} pixels' bytes, got ${rgba.length}`);
  const data = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const o = i * 4;
    data[i] = rgba[o] * 256 + rgba[o + 1] + rgba[o + 2] / 256 - 32768;
  }
  return { width, height, data };
}

/**
 * The height at (u, v), fractions of the tile from its north-west corner. Pixels are
 * areas, so values interpolate between pixel centres and hold flat in the outer half
 * pixel.
 */
export function heightAt(g: HeightGrid, u: number, v: number): number {
  const x = Math.min(g.width - 1, Math.max(0, u * g.width - 0.5));
  const y = Math.min(g.height - 1, Math.max(0, v * g.height - 0.5));
  const x0 = Math.floor(x);
  const y0 = Math.floor(y);
  const x1 = Math.min(g.width - 1, x0 + 1);
  const y1 = Math.min(g.height - 1, y0 + 1);
  const fx = x - x0;
  const fy = y - y0;
  const d = g.data;
  const w = g.width;
  const top = d[y0 * w + x0] * (1 - fx) + d[y0 * w + x1] * fx;
  const bottom = d[y1 * w + x0] * (1 - fx) + d[y1 * w + x1] * fx;
  return top * (1 - fy) + bottom * fy;
}

/** Heights at the (n+1)² vertices of an n × n grid laid over `rect` of the tile. */
export function resampleGrid(g: HeightGrid, rect: Rect, n: number): Float32Array {
  const out = new Float32Array((n + 1) * (n + 1));
  for (let j = 0; j <= n; j++) {
    const v = rect.oy + (rect.size * j) / n;
    for (let i = 0; i <= n; i++) out[j * (n + 1) + i] = heightAt(g, rect.ox + (rect.size * i) / n, v);
  }
  return out;
}

/** The lowest and highest heights in a tile: a test diagnostic. */
export function heightRange(g: HeightGrid): { min: number; max: number } {
  let min = Infinity;
  let max = -Infinity;
  for (const h of g.data) {
    if (h < min) min = h;
    if (h > max) max = h;
  }
  return { min, max };
}
