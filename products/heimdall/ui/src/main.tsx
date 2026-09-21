import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';
import { initMode } from '@midgard/design-system';
import { DataProvider } from '@midgard/ui-shared';
import { HeimdallMock, type HeimdallGateway } from '@midgard/sdk';
import { router } from './app/routes';

import '@midgard/design-system/tokens.css';

// Apply persisted colour mode + wire the OS listener.
initMode();

/* The data seam. Today the console runs on the in-enclosure mock. To point it
 * at real fused-picture data, replace this one line with a Heimdall HTTP gateway
 * once a browser-reachable bridge exists — no screen changes required. */
const gateway: HeimdallGateway = new HeimdallMock();

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <DataProvider client={gateway}>
      <RouterProvider router={router} />
    </DataProvider>
  </React.StrictMode>,
);
