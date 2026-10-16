import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';
import { initMode } from '@midgard/design-system';
import { AuthGate } from '@midgard/ui-shared';
import { HeimdallHttp, HeimdallMock, login, type HeimdallGateway } from '@midgard/sdk';
import { router } from './app/routes';

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
const baseUrl = import.meta.env.VITE_GATEWAY_URL ?? 'http://127.0.0.1:7710/api/v1';

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <AuthGate<HeimdallGateway>
      mode={mode}
      baseUrl={baseUrl}
      mockClient={new HeimdallMock()}
      makeClient={(token) => new HeimdallHttp({ baseUrl, token })}
      login={login}
    >
      <RouterProvider router={router} />
    </AuthGate>
  </React.StrictMode>,
);
