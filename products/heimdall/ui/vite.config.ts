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
      '@midgard/sdk': r('../../../sdk/ts/src/index.ts'),
    },
  },
  server: { port: 5176 },
});
