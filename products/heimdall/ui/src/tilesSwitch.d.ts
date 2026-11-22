/* The map tiles switch, fixed at build time from `VITE_HEIMDALL_TILES` (vite.config.ts).
 * Code that fetches tiles is reached only behind `if (__HEIMDALL_TILES__)`, so a build
 * with the switch off contains none of it. [FR-H-1] */
declare const __HEIMDALL_TILES__: boolean;
