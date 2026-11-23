/* The tile set this build fetches from, chosen at build time (`VITE_HEIMDALL_TILE_SOURCE`,
 * see src/tileSource.ts): the mirror, or in a `public` demo build the public hosts.
 * The branch not taken, and with it the other set's host names, is left out of the
 * bundle; a `none` build never imports this module. [FR-H-1] [NFR-PRV-1] */
import { type TileSet, mirrorSources, tileSet } from './hosts.ts';
import { publicSources } from './publicHosts.ts';

export const TILES: TileSet = tileSet(__HEIMDALL_TILE_PUBLIC__ ? publicSources() : mirrorSources(__HEIMDALL_TILE_MIRROR__));
