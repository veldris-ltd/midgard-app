/* The gateway base URL guard. The console sends the username, password, TOTP code and
 * bearer token to this URL, and the edge speaks plaintext HTTP: that is safe only over
 * an SSH tunnel to loopback, or behind TLS termination. So a URL is accepted when it
 * is https://, or http:// to 127.0.0.1, localhost or [::1], and refused otherwise,
 * before anything is sent. The same rule is checked when the console is built
 * (products/midgard/ui/vite.config.ts) and when its image is built
 * (deploy/installer/console.Dockerfile). [RC-24] [RC-37] [VLD-ANB-MIDGARD-0014] */

const LOOPBACK_HOSTS = new Set(['127.0.0.1', 'localhost', '[::1]', '::1']);

/**
 * Return `url` unchanged when it is safe to send credentials to; throw otherwise. A
 * URL that does not parse is refused too.
 */
export function assertSafeBaseUrl(url: string): string {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    throw new Error(`Refusing a gateway URL that does not parse: ${url}`);
  }
  if (parsed.protocol === 'https:') return url;
  if (parsed.protocol === 'http:' && LOOPBACK_HOSTS.has(parsed.hostname)) return url;
  throw new Error('Refusing a plaintext gateway URL; use https:// unless the edge is on loopback');
}
