/* The console's data seam, read by main.tsx and, at build time, by vite.config.ts.
 * Today the console runs only on the in-enclosure mock; switching to an HTTP gateway is
 * this one line. vite.config.ts refuses a `public` tile build unless this is `mock`, so
 * a public demo build can never carry real tracks once a gateway exists. [AC-25.2]
 * [VLD-ANB-MIDGARD-0086] [FR-H-1] [NFR-PRV-1] */
export type DataMode = 'http' | 'mock';

export const DATA_MODE = 'mock' as DataMode;
