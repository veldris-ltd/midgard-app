import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';
import { initMode } from '@midgard/design-system';
import { AuthGate, gatewayBaseUrl } from '@midgard/ui-shared';
import { HeimdallHttp, HeimdallMock, login, type HeimdallGateway } from '@midgard/sdk';
import { router } from './app/routes';
import { HeimdallStatusProvider } from './app/status';

import '@midgard/design-system/tokens.css';

// Apply persisted colour mode + wire the OS listener.
initMode();

/* The data seam.
 *
 * By DEFAULT the console signs in to the real `http-gateway` edge (127.0.0.1:7710) and
 * reads the live fused picture through it: the gateway authorises and audits the user,
 * then reads the picture from `heimdall-feed` over PQ-mTLS (start the gateway with
 * `--heimdall-addr` and `--heimdall-identity-dir`). The operating picture, entities and
 * feeds are live; picture sync, workflows, decisions and lineage are still simulated
 * (see HeimdallHttp). [FR-H-1]
 *
 * Set `VITE_GATEWAY=mock` for an OFFLINE DEMO only (no sign-in, seeded fixtures). */
const mode: 'http' | 'mock' = import.meta.env.VITE_GATEWAY === 'mock' ? 'mock' : 'http';
// The Mac app injects the gateway URL at run time; dev uses VITE_GATEWAY_URL or the local edge.
const baseUrl = gatewayBaseUrl();

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <AuthGate<HeimdallGateway>
      mode={mode}
      baseUrl={baseUrl}
      mockClient={new HeimdallMock()}
      makeClient={(token) => new HeimdallHttp({ baseUrl, token })}
      login={login}
    >
      {/* Live header values, alerts, toasts and wall mode for every screen. [FR-H-1] */}
      <HeimdallStatusProvider>
        <RouterProvider router={router} />
      </HeimdallStatusProvider>
    </AuthGate>
  </React.StrictMode>,
);
