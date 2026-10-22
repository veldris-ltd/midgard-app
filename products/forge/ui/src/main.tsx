import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';
import { initMode } from '@midgard/design-system';
import { AuthGate, gatewayBaseUrl } from '@midgard/ui-shared';
import { ForgeHttp, ForgeMock, login, type ForgeGateway } from '@midgard/sdk';
import { router } from './app/routes';

import '@midgard/design-system/tokens.css';

// Apply persisted colour mode + wire the OS listener.
initMode();

/* The data seam.
 *
 * By DEFAULT the console signs in to the real `http-gateway` edge (127.0.0.1:7710) and
 * shows the enclosure's live data operations: the Forge worker's jobs and the Heimdall
 * feeds, with each one's last run, schedule, health and projection into the object
 * fabric (start the gateway with `--forge-jobs`, and `--forge-status` and
 * `--heimdall-feeds-status` for live runs). The gateway authorises and audits every
 * read. [FR-F-2]
 *
 * Set `VITE_GATEWAY=mock` for an OFFLINE DEMO only (no sign-in, seeded fixtures). */
const mode: 'http' | 'mock' = import.meta.env.VITE_GATEWAY === 'mock' ? 'mock' : 'http';
// The Mac app injects the gateway URL at run time; dev uses VITE_GATEWAY_URL or the local edge.
const baseUrl = gatewayBaseUrl();

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <AuthGate<ForgeGateway>
      mode={mode}
      baseUrl={baseUrl}
      mockClient={new ForgeMock()}
      makeClient={(token) => new ForgeHttp({ baseUrl, token })}
      login={login}
    >
      <RouterProvider router={router} />
    </AuthGate>
  </React.StrictMode>,
);
