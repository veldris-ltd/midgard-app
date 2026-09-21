import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';
import { initMode } from '@midgard/design-system';
import { DataProvider } from '@midgard/ui-shared';
import { ClientMock, type ClientGateway } from '@midgard/sdk';
import { router } from './app/routes';

import '@midgard/design-system/tokens.css';

// Apply persisted colour mode + wire the OS listener.
initMode();

/* The data seam. Today the shells run on the in-device mock — the offline-first
 * client core, modelled in memory. To point them at the real `midgard-client-core`
 * (crypto, storage, CRDT sync over PQ-mTLS), replace this one line with the core
 * bridge once it exposes a browser-reachable seam — no screen changes required. */
const gateway: ClientGateway = new ClientMock();

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <DataProvider client={gateway}>
      <RouterProvider router={router} />
    </DataProvider>
  </React.StrictMode>,
);
