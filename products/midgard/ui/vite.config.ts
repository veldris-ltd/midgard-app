import { defineConfig, loadEnv } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'node:url';

// Air-gap-first: relative base so the built console serves from any static path.
// Workspace packages are aliased to source so the monorepo builds without a
// separate lib build step; a future HTTP gateway swaps in behind @midgard/sdk only.
const r = (p: string) => fileURLToPath(new URL(p, import.meta.url));

// The console sends credentials and the bearer token to VITE_GATEWAY_URL over the
// edge's plaintext HTTP, so a console is not built against a URL that is neither
// https:// nor http:// on loopback. This is the SDK's assertSafeBaseUrl rule
// (sdk/ts/src/http/baseUrl.ts), repeated because the Vite config cannot import the
// SDK source. [RC-24] [RC-37] [VLD-ANB-MIDGARD-0014]
function refusePlaintextGatewayUrl(url: string | undefined): void {
  if (url === undefined || url === '') return;
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    throw new Error(`VITE_GATEWAY_URL does not parse: ${url}`);
  }
  const loopback = ['127.0.0.1', 'localhost', '[::1]', '::1'].includes(parsed.hostname);
  if (parsed.protocol === 'https:' || (parsed.protocol === 'http:' && loopback)) return;
  throw new Error(
    `Refusing to build against VITE_GATEWAY_URL=${url}: a plaintext gateway URL; ` +
      'use https:// unless the edge is on loopback',
  );
}

export default defineConfig(({ mode }) => {
  refusePlaintextGatewayUrl(loadEnv(mode, process.cwd(), 'VITE_').VITE_GATEWAY_URL);
  return {
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
    server: { port: 5173 },
  };
});
