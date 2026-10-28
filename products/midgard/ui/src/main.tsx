import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';
import { initMode } from '@midgard/design-system';
import { AuthGate } from '@midgard/ui-shared';
import {
  MockGateway,
  createHttpGateway,
  login,
  sessionPolicy,
  whoami,
  type GatewayClient,
  type Session,
} from '@midgard/sdk';
import { router } from './app/routes';

import '@midgard/design-system/tokens.css';

// Apply persisted colour mode + wire the OS listener.
initMode();

/* The data seam.
 *
 * By DEFAULT the console authenticates against the real Rust HTTP/JSON edge
 * (`cargo run -p http-gateway`, 127.0.0.1:7710): `AuthGate` presents a login,
 * exchanges credentials for a PQ-signed token, and injects the authenticated
 * `HttpGateway` so screens render live enclosure data — not fixtures. [RC-37]
 *
 * Set `VITE_GATEWAY=mock` for an OFFLINE DEMO only (no auth, seeded fixtures via
 * `MockGateway`). Screens are unchanged either way — they depend only on the
 * GatewayClient interface. */
const mode: 'http' | 'mock' = import.meta.env.VITE_GATEWAY === 'mock' ? 'mock' : 'http';
const baseUrl = import.meta.env.VITE_GATEWAY_URL ?? 'http://127.0.0.1:7710/api/v1';

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <AuthGate<GatewayClient, Session>
      mode={mode}
      baseUrl={baseUrl}
      mockClient={new MockGateway()}
      makeClient={(token, session, onUnauthorized) =>
        createHttpGateway({ baseUrl, token, policy: sessionPolicy(session), onUnauthorized })
      }
      login={login}
      whoami={whoami}
    >
      <RouterProvider router={router} />
    </AuthGate>
  </React.StrictMode>,
);
