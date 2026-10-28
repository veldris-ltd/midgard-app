import type { MarkingLevel, PolicyContext } from '../types';
import { GatewayError } from '../mock/MockGateway';

/* Login: exchange operator credentials for a PQ-signed bearer token at the
 * authenticated HTTP/JSON edge (`services/http-gateway` `/api/v1/login`). The token
 * is then attached to every `HttpGateway` request (`Authorization: Bearer <token>`),
 * so the console renders authenticated enclosure data — not client-side fixtures.
 * A principal enrolled with a TOTP second factor also sends the current code.
 * [RC-37] [RC-29] */

export interface LoginCredentials {
  username: string;
  password: string;
  /** The current authenticator code, for a principal enrolled with a second factor. */
  totp?: string;
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
    body: JSON.stringify(
      creds.totp
        ? { username: creds.username, password: creds.password, totp: creds.totp }
        : { username: creds.username, password: creds.password },
    ),
  });
  if (res.status === 401) throw new Error('Invalid credentials');
  if (res.status === 429) {
    // Too many failed logins for this name: the edge says how long to wait.
    const j = (await res.json().catch(() => ({}))) as { error?: string };
    const wait = res.headers.get('Retry-After');
    throw new Error(j.error ?? `Too many failed logins${wait ? `: try again in ${wait} seconds` : ''}`);
  }
  if (!res.ok) throw new Error(`login failed: ${res.status}`);
  return (await res.json()) as LoginResult;
}

/** The signed-in session as the edge reads it from the token (`GET /whoami`). */
export interface Session {
  subject: string;
  roles: string[];
  /** The clearance band. */
  clearance: MarkingLevel;
  compartments: string[];
  /** The purpose the token authorises. */
  purpose: string;
  /** Token expiry (unix seconds). */
  expiresAt: number;
}

/**
 * Ask the edge who `token` belongs to. Throws a `GatewayError` with status 401 when
 * the token has expired or no longer verifies — the session is over — and a plain
 * `Error` when the edge cannot be reached.
 */
export async function whoami(baseUrl: string, token: string): Promise<Session> {
  const base = baseUrl.replace(/\/$/, '');
  const res = await fetch(`${base}/whoami`, { headers: { Authorization: `Bearer ${token}` } });
  if (!res.ok) {
    const j = (await res.json().catch(() => ({}))) as { error?: string };
    throw new GatewayError(res.status, j.error ?? `whoami failed: ${res.status}`);
  }
  return (await res.json()) as Session;
}

/** The policy context a session acts under. */
export function sessionPolicy(s: Session): PolicyContext {
  return { subject: s.subject, purpose: s.purpose, clearance: s.clearance, roles: s.roles };
}
