import { defineConfig, loadEnv } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'node:url';
import { tilesEnabled } from './src/tilesEnabled.ts';

// Air-gap-first: relative base so the built console serves from any static path.
// Workspace packages are aliased to source so the monorepo builds without a
// separate lib build step; a future HTTP gateway swaps in behind @midgard/sdk only.
const r = (p: string) => fileURLToPath(new URL(p, import.meta.url));

// The map tiles switch, `VITE_HEIMDALL_TILES` (src/tilesEnabled.ts), becomes the
// build-time constant `__HEIMDALL_TILES__`, so with it off every tile module, and
// every tile host name, is left out of the bundle. [FR-H-1]

export default defineConfig(({ mode }) => ({
  base: './',
  plugins: [react()],
  define: {
    __HEIMDALL_TILES__: JSON.stringify(
      tilesEnabled(process.env.VITE_HEIMDALL_TILES ?? loadEnv(mode, process.cwd(), 'VITE_').VITE_HEIMDALL_TILES),
    ),
  },
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
}));
