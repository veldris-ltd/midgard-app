/* The map tile source, fixed at build time, failing closed. [FR-H-1] [NFR-PRV-1]
 *
 * `VITE_HEIMDALL_TILE_SOURCE` is one of:
 * - `none` (the default, when unset): no tile code and no tile host in the bundle. The
 *   console makes no outbound connection; the signed artefact is built this way.
 * - `mirror`: tiles only from `VITE_HEIMDALL_TILE_MIRROR`, an `https://` origin with an
 *   optional base path, inside the enclosure, in the fixed layout of `mirrorSources`.
 * - `public`: the three public tile hosts. A demo build only, outside the enclosure.
 * Any other value, an empty one included, is an error, and so is a mirror given
 * without `mirror`, or the retired `VITE_HEIMDALL_TILES`. vite.config.ts turns the
 * result into the build-time constants declared in tilesSwitch.d.ts. Pure. */

export type TileSourceMode = 'none' | 'mirror' | 'public';

export interface TileConfig {
  mode: TileSourceMode;
  /** The mirror's origin and base path, with no trailing slash; '' unless `mirror`. */
  mirror: string;
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
  if (mode === 'mirror') return { mode, mirror: parseTileMirror(env.mirror) };
  if (env.mirror !== undefined) {
    throw new TileConfigError(`VITE_HEIMDALL_TILE_MIRROR is set, but VITE_HEIMDALL_TILE_SOURCE is ${mode}, not mirror`);
  }
  return { mode, mirror: '' };
}
