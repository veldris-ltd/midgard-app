import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';
import { initMode } from '@midgard/design-system';
import { AuthGate, gatewayBaseUrl } from '@midgard/ui-shared';
import { MimirHttp, MimirMock, login, type MimirGateway } from '@midgard/sdk';
import { router } from './app/routes';

import '@midgard/design-system/tokens.css';

// Apply persisted colour mode + wire the OS listener.
initMode();

/* The data seam.
 *
 * By DEFAULT the console signs in to the real `http-gateway` edge (127.0.0.1:7710). The
 * Model plane and Session traces are then live: the gateway authorises and audits the
 * user and reads `mimir-server`'s observer port over PQ-mTLS (start the gateway with
 * `--mimir-observe-addr` and `--mimir-identity-dir`, and mimir-server with
 * `--observe-bind`). Agents, Logic, Evals, Guardrails and Feedback are still the
 * in-enclosure simulation (see MimirHttp). [FR-MI-8]
 *
 * Set `VITE_GATEWAY=mock` for an OFFLINE DEMO only (no sign-in, seeded fixtures). */
const mode: 'http' | 'mock' = import.meta.env.VITE_GATEWAY === 'mock' ? 'mock' : 'http';
// The Mac app injects the gateway URL at run time; dev uses VITE_GATEWAY_URL or the local edge.
const baseUrl = gatewayBaseUrl();

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <AuthGate<MimirGateway>
      mode={mode}
      baseUrl={baseUrl}
      mockClient={new MimirMock()}
      makeClient={(token) => new MimirHttp({ baseUrl, token })}
      login={login}
    >
      <RouterProvider router={router} />
    </AuthGate>
  </React.StrictMode>,
);
