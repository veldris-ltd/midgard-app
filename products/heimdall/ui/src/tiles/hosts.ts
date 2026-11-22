/* The tile sources and the allow-list built from them. [FR-H-1] [NFR-PRV-1]
 *
 * The console makes outbound requests for tiles only, and only to URLs the active
 * set's `allowed` accepts (the loader refuses anything else): HTTPS, the exact origin
 * (no other host, port or credentials), the source's path, a tile in range, and no
 * query or fragment. The active set (`active.ts`) is chosen at build time:
 * - `mirror`: one origin inside the enclosure, in the fixed layout of `mirrorSources`;
 * - `public` (a demo build only): the three public hosts of `publicHosts.ts`.
 * This module and everything that imports it are reached only in a build with map
 * tiles; a `none` build holds none of it. Pure. */
import { type Tile, validTile } from './tileMath.ts';

export type TileSourceId = 'terrain' | 'imagery' | 'vector';
export const TILE_SOURCE_IDS: readonly TileSourceId[] = ['terrain', 'imagery', 'vector'];

const MB = 1024 * 1024;

/** The most bytes a tile's body may have; a larger one is a permanent "missing" tile. */
export const MAX_TILE_BYTES: Readonly<Record<TileSourceId, number>> = {
  terrain: 1 * MB,
  imagery: 2 * MB,
  vector: 2 * MB,
};

/** Pixel sizes a raster tile may decode to (square); anything else is refused. */
export const RASTER_TILE_SIZES: Readonly<Record<'terrain' | 'imagery', readonly number[]>> = {
  // Terrarium tiles come at 256 px, and some mirrors build them at 512.
  terrain: [256, 512],
  imagery: [256],
};

export interface TileSource {
  id: TileSourceId;
  /** `https://host` or `https://host:port`, as `URL.origin` writes it. */
  origin: string;
  /** Path up to the tile's own segments, with leading and trailing slashes. */
  pathPrefix: string;
  /** How the tile's segments follow the prefix. */
  order: 'zxy' | 'zyx';
  /** File extension after the last segment, or '' for none. */
  ext: string;
  minZoom: number;
  maxZoom: number;
}

export type TileSources = Readonly<Record<TileSourceId, TileSource>>;

/**
 * A tile mirror's sources, from its origin and base path (as `parseTileMirror` returns
 * it). The layout is fixed:
 *   <mirror>/terrarium/{z}/{x}/{y}.png          terrain heights, terrarium encoding
 *   <mirror>/s2cloudless-2016/{z}/{x}/{y}.jpg   Sentinel-2 cloudless 2016 imagery
 *   <mirror>/osm/{z}/{x}/{y}.pbf                OpenStreetMap vector tiles, Shortbread
 */
export function mirrorSources(mirror: string): TileSources {
  const u = new URL(mirror);
  if (u.protocol !== 'https:') throw new Error('tiles: a mirror must be https');
  const base = u.pathname.replace(/\/$/, '');
  const at = (dir: string) => `${base}/${dir}/`;
  return {
    terrain: { id: 'terrain', origin: u.origin, pathPrefix: at('terrarium'), order: 'zxy', ext: '.png', minZoom: 0, maxZoom: 15 },
    imagery: { id: 'imagery', origin: u.origin, pathPrefix: at('s2cloudless-2016'), order: 'zxy', ext: '.jpg', minZoom: 0, maxZoom: 14 },
    vector: { id: 'vector', origin: u.origin, pathPrefix: at('osm'), order: 'zxy', ext: '.pbf', minZoom: 0, maxZoom: 14 },
  };
}

/** A set of sources with its URL builder and its allow-list. */
export interface TileSet {
  readonly sources: TileSources;
  /** The origins requests may go to: a CSP's `connect-src`. */
  readonly origins: ReadonlySet<string>;
  url(id: TileSourceId, t: Tile): string;
  /** The source a URL belongs to when it is exactly an allowed tile request, else null. */
  allowed(url: string): TileSourceId | null;
}

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

export function tileSet(sources: TileSources): TileSet {
  const list = TILE_SOURCE_IDS.map((id) => sources[id]);
  for (const s of list) {
    if (!s.origin.startsWith('https://') || new URL(s.origin).origin !== s.origin) throw new Error(`tiles: not an https origin: ${s.origin}`);
  }
  const url = (id: TileSourceId, t: Tile): string => {
    const s = sources[id];
    if (!validTile(t)) throw new Error(`tiles: not a tile: ${t.z}/${t.x}/${t.y}`);
    if (t.z < s.minZoom || t.z > s.maxZoom) throw new Error(`tiles: ${id} has no zoom ${t.z}`);
    const path = s.order === 'zxy' ? `${t.z}/${t.x}/${t.y}` : `${t.z}/${t.y}/${t.x}`;
    return `${s.origin}${s.pathPrefix}${path}${s.ext}`;
  };
  const patterns = list.map((s) => ({
    s,
    re: new RegExp(`^${escape(s.pathPrefix)}(\\d{1,2})/(\\d{1,7})/(\\d{1,7})${escape(s.ext)}$`),
  }));
  const allowed = (raw: string): TileSourceId | null => {
    let u: URL;
    try {
      u = new URL(raw);
    } catch {
      return null;
    }
    if (u.protocol !== 'https:' || u.username || u.password || u.search || u.hash) return null;
    for (const { s, re } of patterns) {
      if (u.origin !== s.origin) continue;
      const m = re.exec(u.pathname);
      if (!m) continue;
      const z = Number(m[1]);
      const [x, y] = s.order === 'zxy' ? [Number(m[2]), Number(m[3])] : [Number(m[3]), Number(m[2])];
      if (!validTile({ z, x, y }) || z < s.minZoom || z > s.maxZoom) return null;
      // Only the one spelling `url` builds: nothing a URL parser would normalise
      // (backslashes, dot segments, case, a trailing dot, a default port, padded
      // numbers) gets through.
      const canonical = url(s.id, { z, x, y });
      return raw === canonical && u.href === canonical ? s.id : null;
    }
    return null;
  };
  return { sources, origins: new Set(list.map((s) => s.origin)), url, allowed };
}
