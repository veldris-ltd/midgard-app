/* The map tile source, fixed at build time from `VITE_HEIMDALL_TILE_SOURCE` and
 * `VITE_HEIMDALL_TILE_MIRROR` (src/tileSource.ts, vite.config.ts). Code that fetches
 * tiles is reached only behind `if (__HEIMDALL_TILES__)`, so a `none` build contains
 * none of it; the public hosts only behind `__HEIMDALL_TILE_PUBLIC__`. [FR-H-1] [NFR-PRV-1] */
/** Map tiles at all: the source is `mirror` or `public`. */
declare const __HEIMDALL_TILES__: boolean;
/** The source is `public`: the three public hosts (a demo build only). */
declare const __HEIMDALL_TILE_PUBLIC__: boolean;
/** The mirror's origin and base path in a `mirror` build; '' otherwise. */
declare const __HEIMDALL_TILE_MIRROR__: string;
