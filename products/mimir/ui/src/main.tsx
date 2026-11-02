import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';
import { initMode } from '@midgard/design-system';
import { AuthGate, DemonstrationBanner } from '@midgard/ui-shared';
import { login, whoami, type MimirGateway, type Session } from '@midgard/sdk';
import { router } from './app/routes';

import '@midgard/design-system/tokens.css';

// Apply persisted colour mode + wire the OS listener.
initMode();

/* The data seam. Today the console runs on the in-enclosure mock. To point it
 * at real data, replace this one line with an HTTP-backed `MimirGateway` once a
 * browser-reachable bridge exists (Rust inference/agent plane over PQ-mTLS) —
 * no screen changes required. Screens read this client via `useGateway<MimirGateway>()`.
 *
 * Until then it runs in mock mode, behind `AuthGate mode="mock"` and the demonstration
 * banner, with `MimirMock` loaded by dynamic import from `@midgard/sdk/mock`; the
 * switch to http is the `mode` line below. [AC-25.2] [VLD-ANB-MIDGARD-0086] */
const mode: 'http' | 'mock' = 'mock';
const baseUrl = import.meta.env.VITE_GATEWAY_URL ?? 'http://127.0.0.1:7710/api/v1';

/** No HTTP MimirGateway exists yet, so http mode has no client to build. */
function noHttpGateway(): never {
  throw new Error('The Mimir console has no HTTP gateway yet: run it in mock mode.');
}

async function bootstrap() {
  let mockClient: MimirGateway | undefined;
  if (mode === 'mock') {
    const { MimirMock } = await import('@midgard/sdk/mock');
    mockClient = new MimirMock();
  }
  createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      {mode === 'mock' && <DemonstrationBanner />}
      <AuthGate<MimirGateway, Session>
        mode={mode}
        baseUrl={baseUrl}
        mockClient={mockClient}
        makeClient={noHttpGateway}
        login={login}
        whoami={whoami}
      >
        <RouterProvider router={router} />
      </AuthGate>
    </React.StrictMode>,
  );
}

void bootstrap();
