/* The base URL guard over the four cases of AC-13.4: loopback http:// and any https://
 * pass unchanged, plaintext to another host is refused. The URLs are inert literals.
 * [RC-24] [AC-13.4] [VLD-ANB-MIDGARD-0014] */

import { test } from 'node:test';
import assert from 'node:assert/strict';

import { assertSafeBaseUrl } from './baseUrl.ts';

test('loopback http:// and https:// base URLs are returned unchanged', () => {
  for (const url of [
    'http://127.0.0.1:7710/api/v1',
    'http://localhost:7710/api/v1',
    'http://[::1]:7710/api/v1',
    'https://edge.example/api/v1',
  ]) {
    assert.equal(assertSafeBaseUrl(url), url);
  }
});

test('a plaintext base URL off loopback is refused, naming https', () => {
  for (const url of [
    'http://10.0.0.5:7710/api/v1',
    'http://127.0.0.1.edge.example:7710/api/v1',
    'ftp://127.0.0.1/api/v1',
    'not a url',
  ]) {
    assert.throws(() => assertSafeBaseUrl(url), /https|does not parse/, url);
  }
  assert.throws(() => assertSafeBaseUrl('http://10.0.0.5:7710/api/v1'), /https:\/\//);
});
