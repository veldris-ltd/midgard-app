import { defineConfig, loadEnv } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'node:url';
import { tileConfig } from './src/tileSource.ts';
import { DATA_MODE } from './src/dataMode.ts';

// Air-gap-first: relative base so the built console serves from any static path.
// Workspace packages are aliased to source so the monorepo builds without a
// separate lib build step; a future HTTP gateway swaps in behind @midgard/sdk only.
const r = (p: string) => fileURLToPath(new URL(p, import.meta.url));

// The map tile source (src/tileSource.ts), fixed at build time and failing closed:
// VITE_HEIMDALL_TILE_SOURCE is none (the default), mirror (with VITE_HEIMDALL_TILE_MIRROR)
// or public (refused unless src/dataMode.ts is mock), and any other value stops the build. It becomes the constants of
// src/tilesSwitch.d.ts, so a `none` build leaves out every tile module and every tile
// host, and a `mirror` build every public host. [FR-H-1] [NFR-PRV-1]

export default defineConfig(({ mode }) => {
  const fileEnv = loadEnv(mode, process.cwd(), 'VITE_');
  const pick = (k: string): string | undefined => process.env[k] ?? fileEnv[k];
  const tiles = tileConfig({
    source: pick('VITE_HEIMDALL_TILE_SOURCE'),
    mirror: pick('VITE_HEIMDALL_TILE_MIRROR'),
    legacy: pick('VITE_HEIMDALL_TILES'),
    dataMode: DATA_MODE,
  });
  for (const w of tiles.warnings) console.warn(`\n  warning: ${w}\n`);
  return {
    base: './',
    plugins: [react()],
    define: {
      __HEIMDALL_TILES__: JSON.stringify(tiles.mode !== 'none'),
      __HEIMDALL_TILE_PUBLIC__: JSON.stringify(tiles.mode === 'public'),
      __HEIMDALL_TILE_MIRROR__: JSON.stringify(tiles.mirror),
    },
    // Tile decoding runs in a same-origin module worker, emitted as its own file (never
    // a blob: URL), so a CSP with `worker-src 'self'` holds. [FR-H-1]
    worker: { format: 'es' as const },
    resolve: {
      alias: {
        '@midgard/design-system/tokens.css': r('../../../ui/design-system/src/tokens/styles.css'),
        '@midgard/design-system': r('../../../ui/design-system/src/index.ts'),
        '@midgard/ui-shared': r('../../../ui/shared/src/index.ts'),
        // The mock entry, ahead of '@midgard/sdk', which would otherwise also match it.
        // Only a mock-mode build imports it. [AC-25.2]
        '@midgard/sdk/mock': r('../../../sdk/ts/src/mock/index.ts'),
        '@midgard/sdk': r('../../../sdk/ts/src/index.ts'),
      },
    },
    server: { port: 5176 },
    build: {
      // The full licence text of every bundled dependency (three.js, React and the rest)
      // ships in dist/, since minifying drops their headers. [FR-H-1]
      license: { fileName: 'THIRD-PARTY-LICENSES.md' },
      // The globe's chunk is three.js (about 0.5 MB minified) and the view. It loads only
      // when 3D is opened, and three cannot be split further, so the warning is raised
      // just above it; the main chunk stays far below. [FR-H-1]
      chunkSizeWarningLimit: 600,
    },
  };
});
