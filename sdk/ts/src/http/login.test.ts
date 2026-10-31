/* `logout` ends a session at the edge: one `POST /logout` with the token as the
 * bearer. A 401 (the token was already dead) still resolves, so the console clears
 * its state; any other failure is reported. Run with `pnpm --filter @midgard/sdk test`.
 * [RC-37] [VLD-ANB-MIDGARD-0021] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { logout } from './login.ts';

interface Call {
  url: string;
  method: string;
  authorization: string | null;
}

/** Replace `fetch` with one that answers every call with `status`, recording them. */
function fakeFetch(status: number, body: unknown = {}): Call[] {
  const calls: Call[] = [];
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    calls.push({
      url: String(input),
      method: init?.method ?? 'GET',
      authorization: new Headers(init?.headers).get('Authorization'),
    });
    return new Response(status === 204 ? null : JSON.stringify(body), { status });
  }) as typeof fetch;
  return calls;
}

test('logout posts the bearer to /logout', async () => {
  const calls = fakeFetch(204);
  await logout('http://127.0.0.1:7710/api/v1/', 'abcd');
  assert.deepEqual(calls, [
    { url: 'http://127.0.0.1:7710/api/v1/logout', method: 'POST', authorization: 'Bearer abcd' },
  ]);
});

test('logout tolerates 401: an already-dead token still signs out locally', async () => {
  const calls = fakeFetch(401, { error: 'token revoked' });
  await logout('http://127.0.0.1:7710/api/v1', 'abcd');
  assert.equal(calls.length, 1);
});

test('logout reports any other failure', async () => {
  fakeFetch(500, { error: 'sign-out not recorded: disk full' });
  await assert.rejects(logout('http://127.0.0.1:7710/api/v1', 'abcd'), (e: unknown) => {
    assert.equal((e as { status?: number }).status, 500);
    assert.match(String(e), /sign-out not recorded/);
    return true;
  });
});
