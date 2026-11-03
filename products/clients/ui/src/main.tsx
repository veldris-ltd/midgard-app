import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router'; // v7, one router package [AC-P4.2]
import { initMode } from '@midgard/design-system';
import { AuthGate, DemonstrationBanner } from '@midgard/ui-shared';
import { login, whoami, type ClientGateway, type Session } from '@midgard/sdk';
import { router } from './app/routes';

import '@midgard/design-system/tokens.css';

// Apply persisted colour mode + wire the OS listener.
initMode();

/* The data seam. Today the shells run on the in-device mock — the offline-first
 * client core, modelled in memory. To point them at the real `midgard-client-core`
 * (crypto, storage, CRDT sync over PQ-mTLS), replace this one line with the core
 * bridge once it exposes a browser-reachable seam — no screen changes required.
 *
 * Until then it runs in mock mode, behind `AuthGate mode="mock"` and the demonstration
 * banner, with `ClientMock` loaded by dynamic import from `@midgard/sdk/mock`; the
 * switch to http is the `mode` line below. [AC-25.2] [VLD-ANB-MIDGARD-0086] */
const mode: 'http' | 'mock' = 'mock';
const baseUrl = import.meta.env.VITE_GATEWAY_URL ?? 'http://127.0.0.1:7710/api/v1';

/** No HTTP ClientGateway exists yet, so http mode has no client to build. */
function noHttpGateway(): never {
  throw new Error('The client shells has no HTTP gateway yet: run it in mock mode.');
}

async function bootstrap() {
  let mockClient: ClientGateway | undefined;
  if (mode === 'mock') {
    const { ClientMock } = await import('@midgard/sdk/mock');
    mockClient = new ClientMock();
  }
  createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      {mode === 'mock' && <DemonstrationBanner />}
      <AuthGate<ClientGateway, Session>
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
