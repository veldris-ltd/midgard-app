import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'node:url';

// Air-gap-first: relative base so the built console serves from any static path.
// Workspace packages are aliased to source so the monorepo builds without a
// separate lib build step; a future HTTP gateway swaps in behind @midgard/sdk only.
const r = (p: string) => fileURLToPath(new URL(p, import.meta.url));

export default defineConfig({
  base: './',
  plugins: [react()],
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
});
