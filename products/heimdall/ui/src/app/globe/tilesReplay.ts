/* The close-up map tiles recorded for the offline build (the Mac app): the manifest
 * tools/mac-app/capture-tiles.py writes to <recording>/tiles/manifest.json, served by the app
 * at /tiles/, and which recorded tiles the close-up detail (detailPatch.ts) reads for a box.
 * Pure (no DOM, no build constants), so it is unit tested (test/tilesReplay.test.ts).
 *
 * Two sources, terrain (terrarium PNG) and imagery (Sentinel-2 cloudless JPEG), each a few
 * zooms ("levels") over one box (the UK and Ireland). A level lists the tiles stored and those
 * that could not be captured; any other tile in its range is sea (not stored: drawn as sea).
 * A read at zoom z uses the finest level at or below z that covers the box with no missing
 * tile, in at most `maxTiles` tiles. Outside the box nothing is recorded: the caller clips the
 * close-up to it and keeps the bundled map elsewhere. */

export interface TileBox { west: number; east: number; south: number; north: number }

export interface TileLevel {
  z: number;
  x0: number; x1: number; y0: number; y1: number;
  /** Tiles stored, [x, y]. */
  tiles: [number, number][];
  /** Tiles that could not be captured, [x, y]. */
  missing: [number, number][];
}

export type TileSourceId = 'terrarium' | 's2cloudless';

export interface TileSource {
  attribution: string;
  levels: TileLevel[];
}

export interface TilesManifest {
  version: 1;
  captured: number;
  box: TileBox;
  sources: Record<TileSourceId, TileSource>;
}

/** The tiles to read at one zoom: the inclusive range, and whether each is stored (else sea). */
export interface TilePick {
  z: number;
  x0: number; x1: number; y0: number; y1: number;
  stored: (x: number, y: number) => boolean;
}

const isInt = (v: unknown): v is number => typeof v === 'number' && Number.isInteger(v);
const isPair = (p: unknown): p is [number, number] => Array.isArray(p) && p.length === 2 && p.every(isInt);
const isNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);

function parseSource(raw: unknown, name: string): TileSource {
  const s = raw as Partial<TileSource> | null;
  if (!s || typeof s !== 'object') throw new Error(`tiles manifest: no ${name}`);
  const levels = s.levels;
  if (!Array.isArray(levels) || levels.length === 0) throw new Error(`tiles manifest: no ${name} levels`);
  let last = -1;
  for (const l of levels) {
    if (!l || !isInt(l.z) || l.z <= last || l.z > 22) throw new Error('tiles manifest: zooms not increasing');
    if (![l.x0, l.x1, l.y0, l.y1].every(isInt) || l.x1 < l.x0 || l.y1 < l.y0) throw new Error('tiles manifest: bad tile range');
    if (!Array.isArray(l.tiles) || !l.tiles.every(isPair) || !Array.isArray(l.missing) || !l.missing.every(isPair)) {
      throw new Error('tiles manifest: bad tile list');
    }
    last = l.z;
  }
  return { attribution: typeof s.attribution === 'string' ? s.attribution.slice(0, 400) : '', levels };
}

/** Checks a fetched manifest; throws with the reason. */
export function parseTilesManifest(raw: unknown): TilesManifest {
  const m = raw as { version?: unknown; captured?: unknown; box?: Partial<TileBox>; sources?: Record<string, unknown> } | null;
  if (!m || typeof m !== 'object' || m.version !== 1) throw new Error('tiles manifest: unknown version');
  const b = m.box;
  if (!b || !isNum(b.west) || !isNum(b.east) || !isNum(b.south) || !isNum(b.north) || b.east <= b.west || b.north <= b.south) {
    throw new Error('tiles manifest: bad box');
  }
  if (!m.sources || typeof m.sources !== 'object') throw new Error('tiles manifest: no sources');
  return {
    version: 1,
    captured: isInt(m.captured) ? m.captured : 0,
    box: { west: b.west, east: b.east, south: b.south, north: b.north },
    sources: { terrarium: parseSource(m.sources.terrarium, 'terrain'), s2cloudless: parseSource(m.sources.s2cloudless, 'imagery') },
  };
}

/** The part of `box` inside the recorded box, or null when too little of it is. */
export function clipToRecorded(box: TileBox, rec: TileBox): TileBox | null {
  const out = { west: Math.max(box.west, rec.west), east: Math.min(box.east, rec.east), south: Math.max(box.south, rec.south), north: Math.min(box.north, rec.north) };
  // Under a tenth of the view across either way: not worth a close-up (the bundled map stays).
  const minW = (box.east - box.west) / 10, minH = (box.north - box.south) / 10;
  return out.east - out.west > minW && out.north - out.south > minH ? out : null;
}

const tileX = (lon: number, z: number) => ((lon + 180) / 360) * 2 ** z;
const tileY = (lat: number, z: number) => {
  const r = (lat * Math.PI) / 180;
  return ((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) * 2 ** z;
};

/** The recorded tiles to read for `box` (inside the recorded box) at about zoom `z`: the
 * finest level at or below z covering it with no missing tile in at most `maxTiles`, or null. */
export function pickRecorded(src: Pick<TileSource, 'levels'>, box: TileBox, z: number, maxTiles: number): TilePick | null {
  for (let i = src.levels.length - 1; i >= 0; i--) {
    const l = src.levels[i];
    if (l.z > z) continue;
    const x0 = Math.max(l.x0, Math.floor(tileX(box.west, l.z))), x1 = Math.min(l.x1, Math.floor(tileX(box.east, l.z)));
    const y0 = Math.max(l.y0, Math.floor(tileY(box.north, l.z))), y1 = Math.min(l.y1, Math.floor(tileY(box.south, l.z)));
    if (x1 < x0 || y1 < y0 || (x1 - x0 + 1) * (y1 - y0 + 1) > maxTiles) continue;
    const inSpan = ([x, y]: [number, number]) => x >= x0 && x <= x1 && y >= y0 && y <= y1;
    if (l.missing.some(inSpan)) continue;
    const stored = new Set(l.tiles.filter(inSpan).map(([x, y]) => x * 4194304 + y));
    return { z: l.z, x0, x1, y0, y1, stored: (x, y) => stored.has(x * 4194304 + y) };
  }
  return null;
}

/** A recorded tile's URL under the app's /tiles/ (relative to the site root), the same path
 * shape as the source's. */
export function recordedTileUrl(src: TileSourceId, z: number, x: number, y: number): string {
  return src === 'terrarium' ? `/tiles/terrarium/${z}/${x}/${y}.png` : `/tiles/s2cloudless/${z}/${y}/${x}.jpg`;
}
