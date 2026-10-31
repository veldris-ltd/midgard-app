/* AuthGate's sign-out tells the edge before it clears the session, and the token is
 * never put in web storage, so a reload returns to sign-in. Run with
 * `pnpm --filter @midgard/ui-shared test` (node:test, a fake fetch, no browser).
 * [RC-37] [VLD-ANB-MIDGARD-0021] */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { SIGNED_OUT, signOutAtServer, type Logout } from './signOut.ts';

const BASE = 'http://127.0.0.1:7710/api/v1';

/** A `logout` shaped like the SDK's, over `fetch`, logging into `events`. */
function sdkLikeLogout(events: string[], status: number): Logout {
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const bearer = new Headers(init?.headers).get('Authorization');
    events.push(`${init?.method ?? 'GET'} ${String(input)} ${bearer}`);
    return new Response(null, { status });
  }) as typeof fetch;
  return async (baseUrl, token) => {
    const res = await fetch(`${baseUrl}/logout`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}` },
    });
    if (!res.ok && res.status !== 401) throw new Error(`logout failed: ${res.status}`);
  };
}

test('sign-out posts /logout before local state is cleared', async () => {
  const events: string[] = [];
  await signOutAtServer(sdkLikeLogout(events, 204), BASE, 'abcd', (message) =>
    events.push(`end: ${message}`),
  );
  assert.deepEqual(events, [`POST ${BASE}/logout Bearer abcd`, `end: ${SIGNED_OUT}`]);
});

test('a token the edge already refuses still signs out here', async () => {
  const events: string[] = [];
  await signOutAtServer(sdkLikeLogout(events, 401), BASE, 'abcd', (message) =>
    events.push(`end: ${message}`),
  );
  assert.deepEqual(events, [`POST ${BASE}/logout Bearer abcd`, `end: ${SIGNED_OUT}`]);
});

test('an unreachable edge still clears the session, and says the token lives on', async () => {
  const ended: string[] = [];
  const unreachable: Logout = async () => {
    throw new Error('Failed to fetch');
  };
  await signOutAtServer(unreachable, BASE, 'abcd', (message) => ended.push(message));
  assert.equal(ended.length, 1);
  assert.match(ended[0], /could not be told \(Failed to fetch\).*valid there until it expires/);
});

test('without a logout only the local session ends', async () => {
  const ended: string[] = [];
  await signOutAtServer(undefined, BASE, 'abcd', (message) => ended.push(message));
  assert.deepEqual(ended, [SIGNED_OUT]);
});

test('AuthGate keeps the token out of web storage and signs out at the server', () => {
  const source = readFileSync(new URL('./AuthGate.tsx', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /sessionStorage|localStorage|midgard\.session\.token/);
  // The token starts empty on every load, so a reload returns to sign-in.
  assert.match(source, /useState<string \| null>\(null\)/);
  assert.match(source, /signOutAtServer\(logout, baseUrl, token, end\)/);
});
