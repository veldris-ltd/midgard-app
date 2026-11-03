import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';
import { initMode } from '@midgard/design-system';
import { AuthGate } from '@midgard/ui-shared';
import {
  MockGateway,
  assertSafeBaseUrl,
  createHttpGateway,
  login,
  logout,
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
 * (`cargo run -p http-gateway -- --dev`, 127.0.0.1:7710): `AuthGate` presents a login,
 * exchanges credentials for a PQ-signed token, and injects the authenticated
 * `HttpGateway` so screens render live enclosure data — not fixtures. [RC-37]
 *
 * Set `VITE_GATEWAY=mock` for an OFFLINE DEMO only (no auth, seeded fixtures via
 * `MockGateway`). Screens are unchanged either way — they depend only on the
 * GatewayClient interface.
 *
 * The edge speaks plaintext HTTP, so `VITE_GATEWAY_URL` must be https://, or http://
 * on loopback (the SSH tunnel). Any other URL is refused here, before a login form is
 * shown, as it is when the console is built (vite.config.ts) and inside the SDK.
 * [RC-24] [RC-37] [VLD-ANB-MIDGARD-0014] */
const mode: 'http' | 'mock' = import.meta.env.VITE_GATEWAY === 'mock' ? 'mock' : 'http';
const baseUrl = import.meta.env.VITE_GATEWAY_URL ?? 'http://127.0.0.1:7710/api/v1';
let refusal: string | null = null;
if (mode === 'http') {
  try {
    assertSafeBaseUrl(baseUrl);
  } catch (e) {
    refusal = e instanceof Error ? e.message : String(e);
  }
}

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    {refusal !== null ? (
      <main role="alert" style={{ padding: 24, fontSize: 14, color: 'var(--accent-danger, #e5484d)' }}>
        <h1 style={{ fontSize: 16 }}>This console cannot connect to its gateway</h1>
        <p>{refusal}</p>
      </main>
    ) : (
      <AuthGate<GatewayClient, Session>
        mode={mode}
        baseUrl={baseUrl}
        mockClient={new MockGateway()}
        makeClient={(token, session, onUnauthorized) =>
          createHttpGateway({ baseUrl, token, policy: sessionPolicy(session), onUnauthorized })
        }
        login={login}
        whoami={whoami}
        logout={logout}
      >
        <RouterProvider router={router} />
      </AuthGate>
    )}
  </React.StrictMode>,
);
