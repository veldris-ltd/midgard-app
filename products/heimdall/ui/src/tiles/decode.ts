/* Decoding untrusted tiles, with limits. [FR-H-1]
 *
 * Runs inside the decode worker (decodeWorker.ts); pure where it can be, with the
 * browser's image decoder injected, so node:test runs it on recorded and generated
 * tiles.
 * - Raster tiles: the PNG or JPEG header's size is read first, and anything that is
 *   not exactly 256 × 256 (or 512 × 512 for terrain) is refused before the image is
 *   decoded; the decoded bitmap is checked again before `getImageData` or a texture.
 * - Vector tiles: the features in each layer and in the tile, and the points of every
 *   geometry, are counted from the encoded bytes before any geometry is built (a
 *   repeated ClosePath can claim millions of points in a few bytes); past a limit the
 *   tile is refused as too complex. */
import { VectorTile } from '@mapbox/vector-tile';
import { PbfReader } from 'pbf';

import { type MapTileData, extractFeatures } from '../map/features.ts';
import { RASTER_TILE_SIZES } from './hosts.ts';
import { type HeightGrid, decodeTerrarium } from './terrarium.ts';

/** A tile refused by a limit: remembered as missing, never retried. */
export class TileRefused extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'TileRefused';
  }
}

// --- raster -----------------------------------------------------------------------

export interface RasterHeader {
  format: 'png' | 'jpeg';
  width: number;
  height: number;
}

const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/** The image's format and size from its header alone, or null when it is neither a PNG nor a baseline or progressive JPEG. */
export function rasterHeader(b: Uint8Array): RasterHeader | null {
  if (b.length >= 24 && PNG_SIGNATURE.every((v, i) => b[i] === v)) {
    // The first chunk must be IHDR: its width and height, big-endian.
    if (String.fromCharCode(b[12], b[13], b[14], b[15]) !== 'IHDR') return null;
    const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
    return { format: 'png', width: v.getUint32(16), height: v.getUint32(20) };
  }
  if (b.length >= 4 && b[0] === 0xff && b[1] === 0xd8) {
    let i = 2;
    while (i + 3 < b.length) {
      if (b[i] !== 0xff) return null;
      const m = b[i + 1];
      if (m === 0xff) {
        i++; // fill byte
        continue;
      }
      if (m === 0x01 || (m >= 0xd0 && m <= 0xd7)) {
        i += 2; // markers without a length
        continue;
      }
      if (m === 0xd9 || m === 0xda) return null; // end of image or scan, and no frame header
      const len = (b[i + 2] << 8) | b[i + 3];
      if (len < 2) return null;
      // SOF0–SOF15 but DHT (C4), JPG (C8) and DAC (CC): precision, height, width.
      if (m >= 0xc0 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc) {
        if (i + 9 > b.length) return null;
        return { format: 'jpeg', height: (b[i + 5] << 8) | b[i + 6], width: (b[i + 7] << 8) | b[i + 8] };
      }
      i += 2 + len;
    }
  }
  return null;
}

/** Refuses a raster tile whose header is not an allowed format and size for its source. */
export function checkRasterHeader(source: 'terrain' | 'imagery', h: RasterHeader | null): RasterHeader {
  if (!h) throw new TileRefused(`${source}: not a PNG or JPEG tile`);
  if (source === 'terrain' && h.format !== 'png') throw new TileRefused('terrain: not a PNG tile');
  checkRasterSize(source, h.width, h.height);
  return h;
}

export function checkRasterSize(source: 'terrain' | 'imagery', width: number, height: number): void {
  if (width !== height || !RASTER_TILE_SIZES[source].includes(width)) {
    throw new TileRefused(`${source}: a ${width} × ${height} tile; only ${RASTER_TILE_SIZES[source].map((n) => `${n} × ${n}`).join(' or ')}`);
  }
}

/** The browser's decoder and a 2D canvas, injected so the tests can stand in for them. */
export interface ImageDeps {
  createImageBitmap(blob: Blob, options?: ImageBitmapOptions): Promise<ImageBitmap>;
  /** The RGBA pixels of a bitmap. */
  pixels(bmp: ImageBitmap): Uint8ClampedArray;
}

async function decodeBitmap(bytes: Uint8Array, source: 'terrain' | 'imagery', deps: ImageDeps, options?: ImageBitmapOptions): Promise<ImageBitmap> {
  const h = checkRasterHeader(source, rasterHeader(bytes));
  const bmp = await deps.createImageBitmap(new Blob([bytes as BlobPart], { type: h.format === 'png' ? 'image/png' : 'image/jpeg' }), options);
  try {
    // The decoder's own idea of the size, too, before anything reads or uploads it.
    checkRasterSize(source, bmp.width, bmp.height);
  } catch (e) {
    bmp.close();
    throw e;
  }
  return bmp;
}

/** An imagery tile as a bitmap, ready for a texture. */
export function decodeImagery(bytes: Uint8Array, deps: ImageDeps): Promise<ImageBitmap> {
  return decodeBitmap(bytes, 'imagery', deps);
}

/** A terrarium tile's heights, from its PNG with no colour management. */
export async function decodeTerrain(bytes: Uint8Array, deps: ImageDeps): Promise<HeightGrid> {
  const bmp = await decodeBitmap(bytes, 'terrain', deps, { colorSpaceConversion: 'none', premultiplyAlpha: 'none' });
  try {
    return decodeTerrarium(deps.pixels(bmp), bmp.width, bmp.height);
  } finally {
    bmp.close();
  }
}

// --- vector -----------------------------------------------------------------------

export interface VectorLimits {
  featuresPerLayer: number;
  features: number;
  points: number;
}

/**
 * Generous for real OpenStreetMap tiles (the recorded tile has 152 features and about
 * 19,000 points; the densest city tiles at zoom 14 have tens of thousands of features
 * and a few hundred thousand points), far below what would stall the worker.
 */
export const VECTOR_LIMITS: Readonly<VectorLimits> = {
  featuresPerLayer: 50_000,
  features: 100_000,
  points: 1_000_000,
};

/** The private fields of @mapbox/vector-tile 3.0.0's features that locate the geometry. */
interface FeatureInternals {
  _pbf: PbfReader;
  _geometry: number;
}

/**
 * The points a feature's geometry will build, counted from its commands without
 * building any, stopping once past `budget`; a geometry that is not plainly valid is
 * refused. Only geometry that passes is ever given to `loadGeometry`, and for it this
 * count is exactly what `loadGeometry` builds: MoveTo adds a point; LineTo and
 * ClosePath add one once a MoveTo has started a line.
 *
 * Refused, because `loadGeometry` would read them differently (it takes the command
 * integer as signed, `cmdLen >> 3`) or they break the MVT spec: a command integer of
 * 2^31 or more (a "negative" count), a ClosePath with a count other than 1, a MoveTo
 * or LineTo with more repeats than parameter integers left, a parameter read past the
 * geometry's end, a geometry that runs past the tile, and an unknown command.
 */
export function geometryPoints(feature: unknown, budget: number): number {
  const f = feature as FeatureInternals;
  if (!f || typeof f._geometry !== 'number' || !f._pbf) throw new Error('vector tile: unexpected feature shape');
  if (f._geometry < 0) return 0;
  const pbf = f._pbf;
  const bad = (why: string): never => {
    throw new TileRefused(`vector tile malformed: ${why}`);
  };
  pbf.pos = f._geometry;
  const end = pbf.readVarint() + pbf.pos;
  if (end > pbf.length) bad('geometry runs past the tile');
  let points = 0;
  let line = false;
  while (pbf.pos < end) {
    const cmdLen = pbf.readVarint();
    if (cmdLen < 0 || cmdLen > 0x7fffffff) bad(`command integer ${cmdLen}`);
    const cmd = cmdLen % 8;
    const count = Math.floor(cmdLen / 8);
    if (count === 0) continue; // loadGeometry skips it too
    if (cmd === 7) {
      if (count !== 1) bad(`ClosePath count ${count}`);
      if (line) points++;
    } else if (cmd === 1 || cmd === 2) {
      // Every parameter is at least one byte: never more repeats than bytes allow.
      if (2 * count > end - pbf.pos) bad(`${cmd === 1 ? 'MoveTo' : 'LineTo'} count ${count} with ${end - pbf.pos} bytes left`);
      for (let k = 0; k < count; k++) {
        pbf.readSVarint();
        pbf.readSVarint();
        if (pbf.pos > end) bad('a parameter runs past the geometry');
        if (cmd === 1) {
          line = true;
          points++;
        } else if (line) {
          points++;
        }
        if (points > budget) return points;
      }
    } else {
      bad(`unknown geometry command ${cmd}`);
    }
    if (points > budget) return points;
  }
  return points;
}

/** Refuses a vector tile past the limits, before any geometry is built. */
export function checkVectorTile(tile: VectorTile, limits: VectorLimits = VECTOR_LIMITS): void {
  let features = 0;
  for (const layer of Object.values(tile.layers)) {
    if (layer.length > limits.featuresPerLayer) throw new TileRefused(`vector tile too complex: ${layer.length} features in one layer`);
    features += layer.length;
    if (features > limits.features) throw new TileRefused(`vector tile too complex: over ${limits.features} features`);
  }
  let points = 0;
  for (const layer of Object.values(tile.layers)) {
    for (let i = 0; i < layer.length; i++) {
      points += geometryPoints(layer.feature(i), limits.points - points);
      if (points > limits.points) throw new TileRefused(`vector tile too complex: over ${limits.points} points`);
    }
  }
}

/** A vector tile's fills, lines and labels for the map at zoom `z`, within the limits. */
export function decodeVector(bytes: Uint8Array, z: number, limits: VectorLimits = VECTOR_LIMITS): MapTileData {
  const tile = new VectorTile(new PbfReader(bytes));
  checkVectorTile(tile, limits);
  return extractFeatures(tile, z);
}
