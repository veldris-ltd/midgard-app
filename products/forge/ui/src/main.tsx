import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';
import { initMode } from '@midgard/design-system';
import { DataProvider } from '@midgard/ui-shared';
import { ForgeMock, type ForgeGateway } from '@midgard/sdk';
import { router } from './app/routes';

import '@midgard/design-system/tokens.css';

// Apply persisted colour mode + wire the OS listener.
initMode();

/* The data seam. Today the console runs on the in-enclosure mock. To point it
 * at real data, replace this one line with `createHttpGateway({ baseUrl })`
 * once a browser-reachable Forge bridge exists — no screen changes required.
 *   import { createHttpGateway } from '@midgard/sdk';
 *   const gateway = createHttpGateway({ baseUrl: '/api/v1' }); */
const gateway: ForgeGateway = new ForgeMock();

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <DataProvider client={gateway}>
      <RouterProvider router={router} />
    </DataProvider>
  </React.StrictMode>,
);
