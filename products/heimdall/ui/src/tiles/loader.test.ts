/* The tile loader, on recorded tiles: allow-list, no cookies, cache, cancel,
 * concurrency and the offline fallback. No live network. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { tileUrl } from './hosts.ts';
import { type FetchLike, TileLoader } from './loader.ts';

const fixture = (name: string) => readFileSync(new URL(`../../fixtures/tiles/${name}`, import.meta.url));
const RECORDED: Record<string, Buffer> = {
  [tileUrl('terrain', { z: 6, x: 31, y: 19 })]: fixture('terrarium-6-31-19.png'),
  [tileUrl('imagery', { z: 6, x: 31, y: 19 })]: fixture('s2cloudless-2016-6-31-19.jpg'),
  [tileUrl('vector', { z: 8, x: 125, y: 78 })]: fixture('versatiles-osm-8-125-78.mvt'),
};
const TERRAIN = tileUrl('terrain', { z: 6, x: 31, y: 19 });
const IMAGERY = tileUrl('imagery', { z: 6, x: 31, y: 19 });
const VECTOR = tileUrl('vector', { z: 8, x: 125, y: 78 });

interface Call { url: string; init: RequestInit }

/** A fetch that serves the recorded tiles, 404s anything else, and can be held. */
function recorded(opts: { hold?: boolean; fail?: boolean } = {}) {
  const calls: Call[] = [];
  const pending: (() => void)[] = [];
  const fetch: FetchLike = (url, init) => {
    calls.push({ url, init });
    const answer = () => {
      if (opts.fail) return Promise.reject(new TypeError('Failed to fetch'));
      const body = RECORDED[url];
      return Promise.resolve(body ? new Response(new Uint8Array(body)) : new Response(null, { status: 404 }));
    };
    if (!opts.hold) return answer();
    return new Promise<Response>((resolve, reject) => {
      const signal = init.signal;
      signal?.addEventListener('abort', () => reject(new DOMException('Aborted', 'AbortError')));
      pending.push(() => answer().then(resolve, reject));
    });
  };
  return { fetch, calls, release: () => pending.splice(0).forEach((f) => f()) };
}

const bytes = async (b: ArrayBuffer) => b.byteLength;
const settle = () => new Promise((r) => setTimeout(r, 0));

test('fetches an allowed tile once, with no cookies, no referrer and no redirects, and caches it', async () => {
  const net = recorded();
  const l = new TileLoader({ fetch: net.fetch });
  assert.equal(l.status(TERRAIN), 'none');
  l.request(TERRAIN, 0, bytes);
  assert.equal(l.status(TERRAIN), 'pending');
  await settle();
  assert.equal(l.status(TERRAIN), 'ready');
  assert.equal(l.get(TERRAIN), RECORDED[TERRAIN].byteLength);
  l.request(TERRAIN, 0, bytes);
  await settle();
  assert.equal(net.calls.length, 1);
  const init = net.calls[0].init;
  assert.equal(init.credentials, 'omit');
  assert.equal(init.referrerPolicy, 'no-referrer');
  assert.equal(init.redirect, 'error');
  assert.equal(init.mode, 'cors');
});

test('refuses a URL off the allow-list without fetching it', async () => {
  const net = recorded();
  const l = new TileLoader({ fetch: net.fetch });
  for (const u of ['http://tiles.versatiles.org/tiles/osm/1/0/0', 'https://example.com/a.png', 'https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless-2020_3857/default/g/1/0/0.jpg']) {
    assert.throws(() => l.request(u, 0, bytes), /allow-list/);
  }
  assert.equal(net.calls.length, 0);
});

test('a 404 is remembered as missing, not as a failure', async () => {
  const net = recorded();
  const l = new TileLoader({ fetch: net.fetch });
  const absent = tileUrl('terrain', { z: 6, x: 0, y: 0 });
  l.request(absent, 0, bytes);
  await settle();
  assert.equal(l.status(absent), 'missing');
  assert.equal(l.get(absent), null);
  assert.equal(l.state, 'online');
});

test('keeps to its concurrency limit and serves the highest priority first', async () => {
  const net = recorded({ hold: true });
  const l = new TileLoader({ fetch: net.fetch, maxInFlight: 2 });
  l.request(VECTOR, 5, bytes);
  l.request(TERRAIN, 1, bytes);
  l.request(IMAGERY, 0, bytes);
  await settle();
  // Requests made together are sent most urgent first, two at a time.
  assert.deepEqual(net.calls.map((c) => c.url), [IMAGERY, TERRAIN]);
  assert.equal(l.status(VECTOR), 'pending');
  net.release();
  await settle();
  await settle();
  assert.deepEqual(net.calls.map((c) => c.url), [IMAGERY, TERRAIN, VECTOR]);
  net.release();
  await settle();
  assert.equal(l.status(VECTOR), 'ready');
});

test('retain cancels queued and in-flight requests the view no longer needs', async () => {
  const net = recorded({ hold: true });
  const l = new TileLoader({ fetch: net.fetch, maxInFlight: 1 });
  l.request(TERRAIN, 0, bytes);
  l.request(IMAGERY, 1, bytes);
  await settle();
  l.retain(new Set([IMAGERY]));
  await settle();
  assert.ok(net.calls[0].init.signal?.aborted, 'the in-flight request is aborted');
  assert.equal(l.status(TERRAIN), 'none');
  net.release();
  await settle();
  await settle();
  assert.equal(net.calls.length, 2);
  assert.equal(net.calls[1].url, IMAGERY);
  assert.equal(l.stats.cancelled, 1);
  // A cancel is not a network failure.
  assert.equal(l.state, 'online');
});

test('repeated network failures mean offline; it retries later and recovers', async () => {
  let now = 0;
  let fail = true;
  const calls: string[] = [];
  const fetch: FetchLike = (url) => {
    calls.push(url);
    if (fail) return Promise.reject(new TypeError('Failed to fetch'));
    return Promise.resolve(new Response(new Uint8Array(RECORDED[url] ?? [])));
  };
  const changes: string[] = [];
  const l = new TileLoader({ fetch, offlineAfter: 3, retryAfterMs: 30_000, now: () => now, onChange: () => changes.push(l.state) });
  l.request(TERRAIN, 0, bytes);
  l.request(IMAGERY, 0, bytes);
  l.request(VECTOR, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.state, 'offline');
  assert.ok(changes.includes('offline'));
  // While offline nothing more goes out...
  const before = calls.length;
  l.request(TERRAIN, 0, bytes);
  await settle();
  assert.equal(calls.length, before);
  // ...until the retry time, when one request probes the network.
  fail = false;
  now = 31_000;
  l.request(TERRAIN, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.state, 'online');
  assert.equal(l.status(TERRAIN), 'ready');
});

test('fewer failures than the limit still mean offline when nothing else is left to try', async () => {
  const fetch: FetchLike = () => Promise.reject(new TypeError('Failed to fetch'));
  const l = new TileLoader({ fetch, offlineAfter: 3 });
  l.request(TERRAIN, 0, bytes);
  l.request(IMAGERY, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.state, 'offline');
});

test('a browser that says it is offline is offline at once', () => {
  const net = recorded();
  const l = new TileLoader({ fetch: net.fetch, online: () => false });
  l.request(TERRAIN, 0, bytes);
  assert.equal(l.state, 'offline');
  assert.equal(net.calls.length, 0);
});

test('the cache is bounded and disposes what it evicts', async () => {
  const net = recorded();
  const gone: string[] = [];
  const l = new TileLoader({ fetch: net.fetch, cacheEntries: 2 });
  for (const u of [TERRAIN, IMAGERY, VECTOR]) {
    l.request(u, 0, async (b) => ({ u, n: b.byteLength }), (v) => gone.push(v.u));
    await settle();
  }
  assert.deepEqual(gone, [TERRAIN]);
  assert.equal(l.status(TERRAIN), 'none');
  assert.equal(l.status(VECTOR), 'ready');
});

test('a 503 or a 429 is retried after the retry time, never remembered as missing', async () => {
  for (const status of [503, 429]) {
    let now = 0;
    let answer = status;
    const calls: string[] = [];
    const fetch: FetchLike = (url) => {
      calls.push(url);
      return Promise.resolve(answer === 200 ? new Response(new Uint8Array(RECORDED[url])) : new Response(null, { status: answer }));
    };
    const l = new TileLoader({ fetch, retryAfterMs: 30_000, now: () => now });
    l.request(TERRAIN, 0, bytes);
    await settle();
    await settle();
    assert.equal(l.status(TERRAIN), 'failed', `${status}`);
    assert.equal(l.get(TERRAIN), undefined);
    assert.equal(l.state, 'online', 'the server answered: the network is there');
    // Held back until the retry time...
    l.request(TERRAIN, 0, bytes);
    await settle();
    assert.equal(calls.length, 1);
    // ...then asked again, and this time it comes.
    now = 31_000;
    answer = 200;
    l.request(TERRAIN, 0, bytes);
    await settle();
    await settle();
    assert.equal(calls.length, 2);
    assert.equal(l.status(TERRAIN), 'ready', `${status}`);
  }
});

test('a tile that will not decode, or any other 4xx, is remembered as missing', async () => {
  const fetch: FetchLike = (url) => Promise.resolve(url === TERRAIN ? new Response(new Uint8Array([1, 2, 3])) : new Response(null, { status: 403 }));
  const l = new TileLoader({ fetch });
  l.request(TERRAIN, 0, async () => { throw new Error('not a PNG'); });
  l.request(IMAGERY, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.status(TERRAIN), 'missing');
  assert.equal(l.status(IMAGERY), 'missing');
});

test('after tiles have loaded, one network failure with nothing else pending stays online', async () => {
  let fail = false;
  const fetch: FetchLike = (url) => (fail ? Promise.reject(new TypeError('Failed to fetch')) : Promise.resolve(new Response(new Uint8Array(RECORDED[url]))));
  const l = new TileLoader({ fetch, offlineAfter: 3 });
  l.request(TERRAIN, 0, bytes);
  l.request(IMAGERY, 0, bytes);
  await settle();
  await settle();
  fail = true;
  l.request(VECTOR, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.status(VECTOR), 'failed');
  assert.equal(l.state, 'online');
});

test('going offline arms a timer that tells the view to ask again at the retry time', async () => {
  const fetch: FetchLike = () => Promise.reject(new TypeError('Failed to fetch'));
  let changes = 0;
  const l = new TileLoader({ fetch, retryAfterMs: 20, onChange: () => changes++ });
  l.request(TERRAIN, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.state, 'offline');
  const before = changes;
  await new Promise((r) => setTimeout(r, 40));
  assert.ok(changes > before, 'onChange called after the retry time');
  l.destroy();
});

test('failure records are pruned once their retry time has passed', async () => {
  let now = 0;
  const fetch: FetchLike = () => Promise.reject(new TypeError('Failed to fetch'));
  const l = new TileLoader({ fetch, retryAfterMs: 1000, now: () => now, offlineAfter: 1000 });
  l.request(TERRAIN, 0, bytes);
  l.request(IMAGERY, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.failureRecords, 2);
  now = 5000;
  assert.equal(l.status(TERRAIN), 'none');
  l.request(VECTOR, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.failureRecords, 1);
  l.destroy();
});
