/* The tile sources and the host allow-list. [FR-H-1]
 *
 * The console makes outbound requests to these three places only, and only for
 * tiles: the loader refuses any URL `allowedTileUrl` does not accept. HTTPS only, the
 * exact host (no subdomains, no port, no credentials), the source's path, a tile in
 * range, and no query or fragment.
 *
 * This module, and everything that imports it, is reached only through the tiles
 * switch (`VITE_HEIMDALL_TILES`); with the switch off none of it is in the bundle. */
import { type Tile, validTile } from './tileMath.ts';

export type TileSourceId = 'terrain' | 'imagery' | 'vector';

export interface TileSource {
  id: TileSourceId;
  host: string;
  /** Path up to the tile's own segments, with leading and trailing slashes. */
  pathPrefix: string;
  /** How the tile's segments follow the prefix. */
  order: 'zxy' | 'zyx';
  /** File extension after the last segment, or '' for none. */
  ext: string;
  minZoom: number;
  maxZoom: number;
}

export const TILE_SOURCES: Readonly<Record<TileSourceId, TileSource>> = {
  // Mapzen Terrain Tiles on AWS Open Data, terrarium encoding.
  terrain: {
    id: 'terrain',
    host: 's3.amazonaws.com',
    pathPrefix: '/elevation-tiles-prod/terrarium/',
    order: 'zxy',
    ext: '.png',
    minZoom: 0,
    maxZoom: 15,
  },
  // EOX Sentinel-2 cloudless, the 2016 layer (CC BY 4.0) only, Web Mercator set "g".
  imagery: {
    id: 'imagery',
    host: 'tiles.maps.eox.at',
    pathPrefix: '/wmts/1.0.0/s2cloudless_3857/default/g/',
    order: 'zyx',
    ext: '.jpg',
    minZoom: 0,
    maxZoom: 14,
  },
  // VersaTiles: OpenStreetMap vector tiles in the Shortbread schema.
  vector: {
    id: 'vector',
    host: 'tiles.versatiles.org',
    pathPrefix: '/tiles/osm/',
    order: 'zxy',
    ext: '',
    minZoom: 0,
    maxZoom: 14,
  },
};

export const TILE_HOSTS: ReadonlySet<string> = new Set(Object.values(TILE_SOURCES).map((s) => s.host));

export function tileUrl(id: TileSourceId, t: Tile): string {
  const s = TILE_SOURCES[id];
  if (!validTile(t)) throw new Error(`tiles: not a tile: ${t.z}/${t.x}/${t.y}`);
  if (t.z < s.minZoom || t.z > s.maxZoom) throw new Error(`tiles: ${id} has no zoom ${t.z}`);
  const path = s.order === 'zxy' ? `${t.z}/${t.x}/${t.y}` : `${t.z}/${t.y}/${t.x}`;
  return `https://${s.host}${s.pathPrefix}${path}${s.ext}`;
}

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const PATTERNS = Object.values(TILE_SOURCES).map((s) => ({
  s,
  re: new RegExp(`^${escape(s.pathPrefix)}(\\d{1,2})/(\\d{1,7})/(\\d{1,7})${escape(s.ext)}$`),
}));

/** The source a URL belongs to when it is exactly an allowed tile request, else null. */
export function allowedTileUrl(url: string): TileSourceId | null {
  let u: URL;
  try {
    u = new URL(url);
  } catch {
    return null;
  }
  if (u.protocol !== 'https:' || u.username || u.password || u.port || u.search || u.hash) return null;
  for (const { s, re } of PATTERNS) {
    if (u.hostname !== s.host) continue;
    const m = re.exec(u.pathname);
    if (!m) return null;
    const z = Number(m[1]);
    const [x, y] = s.order === 'zxy' ? [Number(m[2]), Number(m[3])] : [Number(m[3]), Number(m[2])];
    if (!validTile({ z, x, y }) || z < s.minZoom || z > s.maxZoom) return null;
    // Only the one spelling this module builds: nothing a URL parser would normalise
    // (backslashes, dot segments, case, a trailing dot, padded numbers) gets through.
    const canonical = tileUrl(s.id, { z, x, y });
    return url === canonical && u.href === canonical ? s.id : null;
  }
  return null;
}
