/* The gateway API base URL, decided at run time.
 *
 * The Mac app serves the consoles' static builds from a loopback server on a port it
 * picks at launch, and sets `window.__MIDGARD_GATEWAY__` (e.g.
 * `http://127.0.0.1:61234/api/v1`) before each page loads. Without it (the Vite dev
 * servers) the build-time `VITE_GATEWAY_URL`, then the default local edge, apply. */

declare global {
  interface Window {
    /** Set by the Mac app shell: the gateway API base URL. */
    __MIDGARD_GATEWAY__?: string;
    /** Set by the Mac app shell: the Midgard console's URL. */
    __MIDGARD_CONSOLE__?: string;
  }
}

const DEFAULT_GATEWAY = 'http://127.0.0.1:7710/api/v1';

/** The gateway API base URL: the Mac app's, else VITE_GATEWAY_URL, else the local edge. */
export function gatewayBaseUrl(): string {
  // An empty value counts as unset.
  const injected = typeof window !== 'undefined' ? window.__MIDGARD_GATEWAY__ : undefined;
  return injected || import.meta.env.VITE_GATEWAY_URL || DEFAULT_GATEWAY;
}

/** The Midgard console's URL when the Mac app set one, else undefined. */
export function midgardConsoleUrl(): string | undefined {
  return typeof window !== 'undefined' ? window.__MIDGARD_CONSOLE__ : undefined;
}
