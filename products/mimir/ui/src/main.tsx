import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';
import { initMode } from '@midgard/design-system';
import { DataProvider } from '@midgard/ui-shared';
import { MimirMock, type MimirGateway } from '@midgard/sdk';
import { router } from './app/routes';

import '@midgard/design-system/tokens.css';

// Apply persisted colour mode + wire the OS listener.
initMode();

/* The data seam. Today the console runs on the in-enclosure mock. To point it
 * at real data, replace this one line with an HTTP-backed `MimirGateway` once a
 * browser-reachable bridge exists (Rust inference/agent plane over PQ-mTLS) —
 * no screen changes required. Screens read this client via `useGateway<MimirGateway>()`. */
const gateway: MimirGateway = new MimirMock();

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <DataProvider client={gateway}>
      <RouterProvider router={router} />
    </DataProvider>
  </React.StrictMode>,
);
