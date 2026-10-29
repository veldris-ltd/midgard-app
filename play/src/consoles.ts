/* The seven consoles the playground serves, in tour order. `id` is the product
   directory under products/ and the folder each is built into under consoles/.
   `theme` picks the design-system accent scope. Section lists come from the
   consoles' own navigation model, so they cannot drift from the sidebars; the
   client shells have no sidebar and list their three views. [ADR-PLAY-001] */

import { NAV } from '@midgard/ui-shared/nav';

export type ConsoleId = 'midgard' | 'forge' | 'mimir' | 'heimdall' | 'bifrost' | 'admin' | 'clients';

export interface ConsoleInfo {
  id: ConsoleId;
  name: string;
  role: string;
  theme: 'midgard' | 'forge' | 'mimir' | 'heimdall' | 'bifrost';
  summary: string;
  sections: { route: string; label: string }[];
}

const fromNav = (id: Exclude<ConsoleId, 'clients'>) =>
  NAV[id].sections.flatMap((s) => s.items.map((i) => ({ route: i.href, label: i.label })));


export const CONSOLES: ConsoleInfo[] = [
  {
    id: 'midgard',
    name: 'Midgard',
    role: 'Core platform',
    theme: 'midgard',
    summary: 'The enclosure: typed objects with lineage, policies and markings, human-gated actions and a signed audit chain.',
    sections: fromNav('midgard'),
  },
  {
    id: 'forge',
    name: 'Forge',
    role: 'Data operations',
    theme: 'forge',
    summary: 'Pipelines from source to object, versioned datasets, and expectations that stop bad data before it reaches the fabric.',
    sections: fromNav('forge'),
  },
  {
    id: 'mimir',
    name: 'Mimir',
    role: 'Local agentic AI',
    theme: 'mimir',
    summary: 'Agents grounded in the ontology, running on local models, behind guardrails that cannot be switched off.',
    sections: fromNav('mimir'),
  },
  {
    id: 'heimdall',
    name: 'Heimdall',
    role: 'Operational fusion',
    theme: 'heimdall',
    summary: 'A fused operating picture where AI ranks the options, a person decides, and every decision can be replayed.',
    sections: fromNav('heimdall'),
  },
  {
    id: 'bifrost',
    name: 'Bifrost',
    role: 'Continuous delivery',
    theme: 'bifrost',
    summary: 'Ringed, health-gated rollouts of verified builds, including to air-gapped sites, with rollback always armed.',
    sections: fromNav('bifrost'),
  },
  {
    id: 'admin',
    name: 'Admin',
    role: 'Suite control panel',
    theme: 'midgard',
    summary: 'One control panel over every subsystem: dual control, no super-admin, and break-glass that reviews itself.',
    sections: fromNav('admin'),
  },
  {
    id: 'clients',
    name: 'Clients',
    role: 'Web, mobile and desktop',
    theme: 'midgard',
    summary: 'The fabric on the device: sealed local stores, conflict-free sync, and a browser extension that holds no keys.',
    sections: [
      { route: '/', label: 'Web and extension' },
      { route: '/mobile', label: 'Mobile' },
      { route: '/desktop', label: 'Desktop' },
    ],
  },
];

export const byId = (id: string): ConsoleInfo | undefined => CONSOLES.find((c) => c.id === id);

/* Where a console's built index lives, relative to the playground root. */
export const consoleUrl = (id: ConsoleId, route = '/'): string => `consoles/${id}/index.html#${route}`;
