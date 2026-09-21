/* Login: exchange operator credentials for a PQ-signed bearer token at the
 * authenticated HTTP/JSON edge (`services/http-gateway` `/api/v1/login`). The token
 * is then attached to every `HttpGateway` request (`Authorization: Bearer <token>`),
 * so the console renders authenticated enclosure data — not client-side fixtures.
 * [RC-37] [RC-29] */

export interface LoginCredentials {
  username: string;
  password: string;
}

export interface LoginResult {
  /** The hex-encoded PQ-signed IdP token to use as the bearer. */
  token: string;
  /** The authenticated subject. */
  subject: string;
  /** Token expiry (unix seconds, as a string). */
  expiresAt: string;
}

/**
 * POST credentials to the gateway's login endpoint and return the issued token.
 * `baseUrl` is the edge's API base (e.g. `http://127.0.0.1:7710/api/v1`). Throws on
 * invalid credentials or an unreachable edge.
 */
export async function login(baseUrl: string, creds: LoginCredentials): Promise<LoginResult> {
  const base = baseUrl.replace(/\/$/, '');
  const res = await fetch(`${base}/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(creds),
  });
  if (res.status === 401) throw new Error('Invalid credentials');
  if (!res.ok) throw new Error(`login failed: ${res.status}`);
  return (await res.json()) as LoginResult;
}
