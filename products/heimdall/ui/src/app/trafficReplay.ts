/* The road traffic recorded for the offline build (the Mac app): the manifest
 * tools/mac-app/capture-traffic.py writes to <recording>/traffic/manifest.json, served by the
 * app at /traffic/, and which recorded tiles to read for a box. Pure (no DOM, no build
 * constants), so it is unit tested (test/trafficReplay.test.ts).
 *
 * One snapshot: TomTom flow tiles at a few zooms ("levels") over the UK and Ireland box. A
 * level lists the tiles that have roads and those that could not be captured; any other tile
 * in its range has no roads. A read at zoom z uses the finest level at or below z that covers
 * the box in at most `maxTiles` tiles (else the coarsest level). */

export interface TrafficBox { west: number; east: number; south: number; north: number }

export interface TrafficLevel {
  z: number;
  x0: number; x1: number; y0: number; y1: number;
  /** Tiles stored under /traffic/<z>/<x>/<y>.pbf, [x, y]. */
  tiles: [number, number][];
  /** Tiles that could not be captured, [x, y]. */
  missing: [number, number][];
}

export interface TrafficManifest {
  version: 1;
  attribution: string;
  link: string;
  captured: number;
  overviewZoom: number;
  box: TrafficBox;
  levels: TrafficLevel[];
}

const isInt = (v: unknown): v is number => typeof v === 'number' && Number.isInteger(v);
const isPair = (p: unknown): p is [number, number] => Array.isArray(p) && p.length === 2 && p.every(isInt);

/** Checks a fetched manifest against what the overlay reads; throws with the reason. */
export function parseTrafficManifest(raw: unknown, want: { overviewZoom: number; box: TrafficBox }): TrafficManifest {
  const m = raw as Partial<TrafficManifest> | null;
  if (!m || typeof m !== 'object' || m.version !== 1) throw new Error('traffic manifest: unknown version');
  if (m.overviewZoom !== want.overviewZoom) throw new Error('traffic manifest: other overview zoom');
  const b = m.box;
  if (!b || b.west !== want.box.west || b.east !== want.box.east || b.south !== want.box.south || b.north !== want.box.north) {
    throw new Error('traffic manifest: other box');
  }
  const levels = m.levels;
  if (!Array.isArray(levels) || levels.length === 0) throw new Error('traffic manifest: no levels');
  let last = -1;
  for (const l of levels) {
    if (!l || !isInt(l.z) || l.z <= last || l.z > 22) throw new Error('traffic manifest: zooms not increasing');
    if (![l.x0, l.x1, l.y0, l.y1].every(isInt) || l.x1 < l.x0 || l.y1 < l.y0) throw new Error('traffic manifest: bad tile range');
    if (!Array.isArray(l.tiles) || !l.tiles.every(isPair) || !Array.isArray(l.missing) || !l.missing.every(isPair)) {
      throw new Error('traffic manifest: bad tile list');
    }
    last = l.z;
  }
  if (levels[0].z !== want.overviewZoom) throw new Error('traffic manifest: no overview level');
  return {
    version: 1,
    attribution: typeof m.attribution === 'string' && m.attribution.length <= 120 ? m.attribution : '©TomTom',
    link: typeof m.link === 'string' ? m.link : '',
    captured: isInt(m.captured) ? m.captured : 0,
    overviewZoom: m.overviewZoom, box: b, levels,
  };
}

const tileX = (lon: number, z: number) => ((lon + 180) / 360) * 2 ** z;
const tileY = (lat: number, z: number) => {
  const r = (lat * Math.PI) / 180;
  return ((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) * 2 ** z;
};

/** The recorded tiles to read for `box` at (about) zoom `z`: the level used and its stored
 * tiles in the box (none when the box is outside the recording or has no roads). */
export function recordedTiles(m: Pick<TrafficManifest, 'levels'>, box: TrafficBox, z: number, maxTiles: number): { z: number; tiles: [number, number][] } {
  const span = (l: TrafficLevel) => {
    const x0 = Math.floor(tileX(box.west, l.z)), x1 = Math.floor(tileX(box.east, l.z));
    const y0 = Math.floor(tileY(box.north, l.z)), y1 = Math.floor(tileY(box.south, l.z));
    return { x0, x1, y0, y1, n: (x1 - x0 + 1) * (y1 - y0 + 1) };
  };
  let level = m.levels[0];
  for (const l of m.levels) if (l.z <= z && span(l).n <= maxTiles) level = l;
  const s = span(level);
  const tiles = level.tiles.filter(([x, y]) => x >= s.x0 && x <= s.x1 && y >= s.y0 && y <= s.y1);
  return { z: level.z, tiles };
}

/** The tile's URL under the app's /traffic/ (relative to the site root). */
export function recordedTrafficUrl(z: number, x: number, y: number): string {
  return `/traffic/${z}/${x}/${y}.pbf`;
}
