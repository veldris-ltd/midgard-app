/* The map tile source, fixed at build time, failing closed. [FR-H-1] [NFR-PRV-1]
 *
 * `VITE_HEIMDALL_TILE_SOURCE` is one of:
 * - `none` (the default, when unset): no tile code and no tile host in the bundle. The
 *   console makes no outbound connection; the signed artefact is built this way.
 * - `mirror`: tiles only from `VITE_HEIMDALL_TILE_MIRROR`, an `https://` origin with an
 *   optional base path, inside the enclosure, in the fixed layout of `mirrorSources`;
 *   a mirror naming a public tile host or its domain stops the build, and one on a
 *   public IP address draws a warning.
 * - `public`: the three public tile hosts. A demo build only, outside the enclosure.
 * Any other value, an empty one included, is an error, and so is a mirror given
 * without `mirror`, or the retired `VITE_HEIMDALL_TILES`. vite.config.ts turns the
 * result into the build-time constants declared in tilesSwitch.d.ts. Pure. */

export type TileSourceMode = 'none' | 'mirror' | 'public';

export interface TileConfig {
  mode: TileSourceMode;
  /** The mirror's origin and base path, with no trailing slash; '' unless `mirror`. */
  mirror: string;
  /** Things the build should print but not stop for (a mirror on a public IP address). */
  warnings: string[];
}

/** The public tile hosts' domains: a mirror on one of them, or under one, is not inside an enclosure. */
export const PUBLIC_TILE_DOMAINS: readonly string[] = [
  's3.amazonaws.com', 'amazonaws.com',
  'tiles.maps.eox.at', 'maps.eox.at', 'eox.at',
  'tiles.versatiles.org', 'versatiles.org',
];

/** The hostname is one of the public tile hosts or their domains, or under one. */
export function isPublicTileHost(hostname: string): boolean {
  const h = hostname.toLowerCase().replace(/\.$/, '');
  return PUBLIC_TILE_DOMAINS.some((d) => h === d || h.endsWith(`.${d}`));
}

/** An IP literal (as `URL.hostname` writes it) that is a public address, not a private, loopback or link-local one. */
export function isPublicIpLiteral(hostname: string): boolean {
  const v4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(hostname);
  if (v4) {
    const [a, b] = [Number(v4[1]), Number(v4[2])];
    const internal = a === 10 || a === 127 || a === 0 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 169 && b === 254) || (a === 100 && b >= 64 && b <= 127);
    return !internal;
  }
  if (hostname.startsWith('[') && hostname.endsWith(']')) {
    const v6 = hostname.slice(1, -1).toLowerCase();
    const internal = v6 === '::1' || /^f[cd][0-9a-f]{0,2}:/.test(v6) || /^fe[89ab][0-9a-f]?:/.test(v6) || v6.startsWith('::ffff:10.') || v6.startsWith('::ffff:192.168.') || v6.startsWith('::ffff:127.');
    return !internal;
  }
  return false;
}

export class TileConfigError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'TileConfigError';
  }
}

const MODES: readonly TileSourceMode[] = ['none', 'mirror', 'public'];

/** The mode from `VITE_HEIMDALL_TILE_SOURCE`: unset is `none`; anything else must be exact. */
export function parseTileSource(value: string | undefined): TileSourceMode {
  if (value === undefined) return 'none';
  if ((MODES as readonly string[]).includes(value)) return value as TileSourceMode;
  throw new TileConfigError(
    `VITE_HEIMDALL_TILE_SOURCE must be exactly none, mirror or public (got ${JSON.stringify(value)}); ` +
      'leave it unset for none, the build with no map tiles',
  );
}

/** One path segment of a mirror's base path: no dot segments, nothing a URL parser rewrites. */
const SEGMENT = /^[A-Za-z0-9_~-][A-Za-z0-9._~-]*$/;

/**
 * The mirror from `VITE_HEIMDALL_TILE_MIRROR`: `https://host[:port][/base/path]`, with no
 * credentials, query or fragment, written exactly as a URL parser writes it back (a
 * trailing slash is allowed). Returns it without the trailing slash.
 */
export function parseTileMirror(value: string | undefined): string {
  const fail = (why: string): never => {
    throw new TileConfigError(
      `VITE_HEIMDALL_TILE_MIRROR ${why} (got ${JSON.stringify(value ?? null)}); ` +
        'it must be an https:// origin with an optional base path, such as https://tiles.enclosure.internal/heimdall',
    );
  };
  if (value === undefined || value === '') return fail('is required when VITE_HEIMDALL_TILE_SOURCE=mirror');
  let u: URL;
  try {
    u = new URL(value);
  } catch {
    return fail('is not a URL');
  }
  if (u.protocol !== 'https:') fail('must use https://');
  if (u.username || u.password) fail('may not carry credentials');
  if (u.search || u.hash || value.includes('?') || value.includes('#')) fail('may not have a query or fragment');
  const path = u.pathname.replace(/\/$/, '');
  if (path !== '' && !path.slice(1).split('/').every((s) => SEGMENT.test(s))) fail('has a base path segment that is not plain');
  const mirror = `${u.origin}${path}`;
  if (value !== mirror && value !== `${mirror}/`) fail(`is not in its canonical form, ${mirror}`);
  // A mirror is the enclosure's own copy: never one of the public tile hosts.
  if (isPublicTileHost(u.hostname)) fail(`names a public tile host (${u.hostname}); a mirror must be inside the enclosure`);
  return mirror;
}

export interface TileEnv {
  source?: string;
  mirror?: string;
  /** The retired switch, VITE_HEIMDALL_TILES: an error if set at all. */
  legacy?: string;
}

/** The whole setting, checked together. Throws `TileConfigError` with a clear message. */
export function tileConfig(env: TileEnv): TileConfig {
  if (env.legacy !== undefined) {
    throw new TileConfigError(
      'VITE_HEIMDALL_TILES is retired: set VITE_HEIMDALL_TILE_SOURCE to none (the default), mirror or public instead',
    );
  }
  const mode = parseTileSource(env.source);
  if (mode === 'mirror') {
    const mirror = parseTileMirror(env.mirror);
    const host = new URL(mirror).hostname;
    const warnings = isPublicIpLiteral(host)
      ? [`VITE_HEIMDALL_TILE_MIRROR is on a public IP address (${host}); a mirror must be inside the enclosure`]
      : [];
    return { mode, mirror, warnings };
  }
  if (env.mirror !== undefined) {
    throw new TileConfigError(`VITE_HEIMDALL_TILE_MIRROR is set, but VITE_HEIMDALL_TILE_SOURCE is ${mode}, not mirror`);
  }
  return { mode, mirror: '', warnings: [] };
}
