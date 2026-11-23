/* The tile loader, on recorded tiles: allow-list, no cookies, cache, cancel,
 * concurrency, the offline fallback, the byte caps and time limits on untrusted
 * bodies (fuzz-style: a gzip bomb and a slow body, generated here), and the pause.
 * No live network. [FR-H-1] [NFR-PRV-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { gzipSync } from 'node:zlib';

import { MAX_TILE_BYTES, mirrorSources, tileSet } from './hosts.ts';
import { type FetchLike, TileDecodeTimeout, TileLoader } from './loader.ts';
import { publicSources } from './publicHosts.ts';

const TILES = tileSet(publicSources());
const tileUrl = TILES.url;

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
  const l = new TileLoader({ tiles: TILES, fetch: net.fetch });
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
  const l = new TileLoader({ tiles: TILES, fetch: net.fetch });
  for (const u of ['http://tiles.versatiles.org/tiles/osm/1/0/0', 'https://example.com/a.png', 'https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless-2020_3857/default/g/1/0/0.jpg']) {
    assert.throws(() => l.request(u, 0, bytes), /allow-list/);
  }
  assert.equal(net.calls.length, 0);
});

test('a 404 is remembered as missing, not as a failure', async () => {
  const net = recorded();
  const l = new TileLoader({ tiles: TILES, fetch: net.fetch });
  const absent = tileUrl('terrain', { z: 6, x: 0, y: 0 });
  l.request(absent, 0, bytes);
  await settle();
  assert.equal(l.status(absent), 'missing');
  assert.equal(l.get(absent), null);
  assert.equal(l.state, 'online');
});

test('keeps to its concurrency limit and serves the highest priority first', async () => {
  const net = recorded({ hold: true });
  const l = new TileLoader({ tiles: TILES, fetch: net.fetch, maxInFlight: 2 });
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
  const l = new TileLoader({ tiles: TILES, fetch: net.fetch, maxInFlight: 1 });
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
  const l = new TileLoader({ tiles: TILES, fetch, offlineAfter: 3, retryAfterMs: 30_000, now: () => now, onChange: () => changes.push(l.state) });
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
  const l = new TileLoader({ tiles: TILES, fetch, offlineAfter: 3 });
  l.request(TERRAIN, 0, bytes);
  l.request(IMAGERY, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.state, 'offline');
});

test('a browser that says it is offline is offline at once', () => {
  const net = recorded();
  const l = new TileLoader({ tiles: TILES, fetch: net.fetch, online: () => false });
  l.request(TERRAIN, 0, bytes);
  assert.equal(l.state, 'offline');
  assert.equal(net.calls.length, 0);
});

test('the cache is bounded and disposes what it evicts', async () => {
  const net = recorded();
  const gone: string[] = [];
  const l = new TileLoader({ tiles: TILES, fetch: net.fetch, cacheEntries: 2 });
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
    const l = new TileLoader({ tiles: TILES, fetch, retryAfterMs: 30_000, now: () => now });
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
  const l = new TileLoader({ tiles: TILES, fetch });
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
  const l = new TileLoader({ tiles: TILES, fetch, offlineAfter: 3 });
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
  const l = new TileLoader({ tiles: TILES, fetch, retryAfterMs: 20, onChange: () => changes++ });
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
  const l = new TileLoader({ tiles: TILES, fetch, retryAfterMs: 1000, now: () => now, offlineAfter: 1000 });
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

// --- untrusted bodies: byte caps and time limits ------------------------------------

/** A body that yields `chunks` chunks of `size` bytes, counting how many were pulled. */
function streamed(chunks: number, size: number) {
  const seen = { pulled: 0 };
  const body = new ReadableStream<Uint8Array>({
    pull(c) {
      if (seen.pulled >= chunks) return c.close();
      seen.pulled++;
      c.enqueue(new Uint8Array(size));
    },
  });
  return { body, seen };
}

test('a Content-Length over the cap is refused before the body is read: a permanent missing tile', async () => {
  const { body, seen } = streamed(100, 64 * 1024);
  const fetch: FetchLike = () => Promise.resolve(new Response(body, { headers: { 'content-length': String(MAX_TILE_BYTES.terrain + 1) } }));
  let decoded = 0;
  const l = new TileLoader({ tiles: TILES, fetch });
  l.request(TERRAIN, 0, async (b) => { decoded++; return b.byteLength; });
  await settle();
  await settle();
  assert.equal(l.status(TERRAIN), 'missing');
  assert.equal(decoded, 0);
  assert.ok(seen.pulled <= 1, `pulled ${seen.pulled} chunks`);
  assert.equal(l.stats.oversize, 1);
  assert.equal(l.state, 'online');
});

test('a streamed body past the cap is stopped there, whatever it claimed: 1 MB terrain, 2 MB imagery and vector', async () => {
  for (const [url, cap] of [[TERRAIN, MAX_TILE_BYTES.terrain], [IMAGERY, MAX_TILE_BYTES.imagery], [VECTOR, MAX_TILE_BYTES.vector]] as const) {
    const { body, seen } = streamed(1000, 64 * 1024);
    // A lying Content-Length, under the cap.
    const fetch: FetchLike = () => Promise.resolve(new Response(body, { headers: { 'content-length': '1000' } }));
    const l = new TileLoader({ tiles: TILES, fetch });
    l.request(url, 0, bytes);
    for (let i = 0; i < 5; i++) await settle();
    assert.equal(l.status(url), 'missing', url);
    assert.ok(seen.pulled * 64 * 1024 <= cap + 2 * 64 * 1024, `read ${seen.pulled} chunks of 64 KB for a ${cap}-byte cap`);
  }
  // At the cap exactly, the tile is read.
  const exact = streamed(16, 64 * 1024);
  const l = new TileLoader({ tiles: TILES, fetch: () => Promise.resolve(new Response(exact.body)) });
  l.request(TERRAIN, 0, bytes);
  for (let i = 0; i < 5; i++) await settle();
  assert.equal(l.get(TERRAIN), MAX_TILE_BYTES.terrain);
});

test('a gzip bomb (a few KB inflating to 16 MB) is stopped at the cap as it inflates', async () => {
  const bomb = gzipSync(Buffer.alloc(16 * 1024 * 1024));
  assert.ok(bomb.length < 64 * 1024, `${bomb.length} bytes compressed`);
  let inflated = 0;
  const fetch: FetchLike = () => {
    // As the browser undoes Content-Encoding: gzip, the body inflates while it is read.
    const body = new Blob([bomb]).stream().pipeThrough(new DecompressionStream('gzip')).pipeThrough(
      new TransformStream<Uint8Array, Uint8Array>({ transform(c, out) { inflated += c.byteLength; out.enqueue(c); } }),
    );
    return Promise.resolve(new Response(body));
  };
  let decoded = 0;
  const l = new TileLoader({ tiles: TILES, fetch });
  l.request(VECTOR, 0, async (b) => { decoded++; return b.byteLength; });
  for (let i = 0; i < 20 && l.status(VECTOR) === 'pending'; i++) await new Promise((r) => setTimeout(r, 5));
  assert.equal(l.status(VECTOR), 'missing');
  assert.equal(decoded, 0);
  assert.ok(inflated < 4 * 1024 * 1024, `inflated ${inflated} bytes before stopping`);
});

test('a slow body runs into the request time limit: a network failure, retried later', async () => {
  const fetch: FetchLike = () => {
    // One chunk, then nothing more, ever.
    let sent = false;
    const body = new ReadableStream<Uint8Array>({
      pull(c) {
        if (!sent) { sent = true; c.enqueue(new Uint8Array(100)); }
        return new Promise(() => {});
      },
    });
    return Promise.resolve(new Response(body));
  };
  let decoded = 0;
  const l = new TileLoader({ tiles: TILES, fetch, fetchTimeoutMs: 40, offlineAfter: 5 });
  l.request(TERRAIN, 0, async (b) => { decoded++; return b.byteLength; });
  await new Promise((r) => setTimeout(r, 20));
  assert.equal(l.status(TERRAIN), 'pending');
  await new Promise((r) => setTimeout(r, 80));
  assert.equal(l.status(TERRAIN), 'failed');
  assert.equal(decoded, 0);
  assert.equal(l.stats.failed, 1);
  l.destroy();
});

test('a request that never answers times out as a network failure; the timeout and the cancel share one signal', async () => {
  const signals: AbortSignal[] = [];
  const fetch: FetchLike = (_url, init) => new Promise((_res, reject) => {
    signals.push(init.signal as AbortSignal);
    init.signal?.addEventListener('abort', () => reject(init.signal?.reason));
  });
  const l = new TileLoader({ tiles: TILES, fetch, fetchTimeoutMs: 30, offlineAfter: 1 });
  l.request(TERRAIN, 0, bytes);
  await new Promise((r) => setTimeout(r, 80));
  assert.ok(signals[0].aborted);
  assert.equal(l.status(TERRAIN), 'failed');
  assert.equal(l.state, 'offline', 'a timeout counts towards offline');
  // A cancel by the view, through the same signal, is no failure at all.
  const l2 = new TileLoader({ tiles: TILES, fetch, fetchTimeoutMs: 10_000, offlineAfter: 1 });
  l2.request(IMAGERY, 0, bytes);
  await settle();
  l2.retain(new Set());
  await settle();
  assert.ok(signals[1].aborted);
  assert.equal(l2.stats.failed, 0);
  assert.equal(l2.state, 'online');
  l.destroy();
  l2.destroy();
});

test('a decode that runs out of time is a failure retried later, not a missing tile', async () => {
  let now = 0;
  const net = recorded();
  const l = new TileLoader({ tiles: TILES, fetch: net.fetch, now: () => now, retryAfterMs: 30_000 });
  l.request(TERRAIN, 0, async () => { throw new TileDecodeTimeout(); });
  await settle();
  await settle();
  assert.equal(l.status(TERRAIN), 'failed');
  assert.equal(l.state, 'online');
  now = 31_000;
  assert.equal(l.status(TERRAIN), 'none');
});

test('a mirror build\'s loader fetches the mirror and refuses the public hosts', async () => {
  const M = tileSet(mirrorSources('https://tiles.enclosure.internal/heimdall'));
  const calls: string[] = [];
  const l = new TileLoader({ tiles: M, fetch: (url) => { calls.push(url); return Promise.resolve(new Response(new Uint8Array(4))); } });
  assert.throws(() => l.request(TERRAIN, 0, bytes), /allow-list/);
  const url = M.url('terrain', { z: 6, x: 31, y: 19 });
  l.request(url, 0, bytes);
  await settle();
  assert.deepEqual(calls, [url]);
});

// --- the pause --------------------------------------------------------------------

test('paused, nothing is sent and what was in flight is cancelled; resuming asks the view again', async () => {
  const net = recorded({ hold: true });
  let changes = 0;
  const l = new TileLoader({ tiles: TILES, fetch: net.fetch, onChange: () => changes++ });
  l.request(TERRAIN, 0, bytes);
  await settle();
  assert.equal(net.calls.length, 1);
  l.setPaused(true);
  assert.ok(l.isPaused);
  assert.ok(net.calls[0].init.signal?.aborted, 'the request in flight is cancelled');
  assert.equal(l.status(TERRAIN), 'none');
  l.request(IMAGERY, 0, bytes);
  l.request(VECTOR, 0, bytes);
  await settle();
  assert.equal(net.calls.length, 1, 'nothing sent while paused');
  assert.equal(l.status(IMAGERY), 'none');
  assert.equal(l.state, 'online', 'a pause is not a failure');
  const before = changes;
  l.setPaused(false);
  assert.ok(changes > before);
  l.request(IMAGERY, 0, bytes);
  await settle();
  assert.equal(net.calls.length, 2);
  assert.equal(net.calls[1].url, IMAGERY);
  net.release();
  await settle();
  await settle();
  assert.equal(l.status(IMAGERY), 'ready');
});

test('paused, tiles already loaded are still served from the cache', async () => {
  const net = recorded();
  const l = new TileLoader({ tiles: TILES, fetch: net.fetch });
  l.request(TERRAIN, 0, bytes);
  await settle();
  await settle();
  l.setPaused(true);
  assert.equal(l.status(TERRAIN), 'ready');
  assert.equal(l.get(TERRAIN), RECORDED[TERRAIN].byteLength);
});

test('a request sent before going offline that fails later does not push the next probe away', async () => {
  let now = 0;
  const hung: (() => void)[] = [];
  let mode: 'hang' | 'fail' | 'ok' = 'hang';
  const calls: string[] = [];
  const fetch: FetchLike = (url) => {
    calls.push(url);
    if (mode === 'hang') return new Promise((_r, reject) => hung.push(() => reject(new TypeError('Failed to fetch'))));
    if (mode === 'fail') return Promise.reject(new TypeError('Failed to fetch'));
    return Promise.resolve(new Response(new Uint8Array(RECORDED[url])));
  };
  const l = new TileLoader({ tiles: TILES, fetch, offlineAfter: 1, retryAfterMs: 30_000, now: () => now });
  l.request(TERRAIN, 0, bytes);
  await settle();
  mode = 'fail';
  now = 1000;
  l.request(IMAGERY, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.state, 'offline');
  // The first request, sent while online, fails 15 s later.
  now = 15_000;
  hung.forEach((f) => f());
  await settle();
  await settle();
  // The probe is still due 30 s after going offline, not 30 s after that late failure.
  mode = 'ok';
  now = 31_000;
  l.request(VECTOR, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.state, 'online');
  l.destroy();
});

test('a retry timer that fires before the clock reaches the retry time re-arms, so the probe still goes', async () => {
  let now = 0;
  let fail = true;
  const calls: string[] = [];
  const fetch: FetchLike = (url) => {
    calls.push(url);
    return fail ? Promise.reject(new TypeError('Failed to fetch')) : Promise.resolve(new Response(new Uint8Array(RECORDED[url])));
  };
  // Like the map: it asks for tiles only when told something changed.
  const l: TileLoader = new TileLoader({ tiles: TILES, fetch, offlineAfter: 1, retryAfterMs: 20, now: () => now, onChange: () => l.request(TERRAIN, 0, bytes) });
  l.request(TERRAIN, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.state, 'offline');
  const before = calls.length;
  fail = false;
  // The timer fires, but the loader's clock says only 10 of the 20 ms have passed.
  now = 10;
  await new Promise((r) => setTimeout(r, 40));
  assert.equal(calls.length, before, 'too early: no probe yet');
  now = 25;
  await new Promise((r) => setTimeout(r, 40));
  assert.ok(calls.length > before, 'the re-armed timer lets the probe through');
  await settle();
  assert.equal(l.state, 'online');
  l.destroy();
});

test('offline with the browser saying so, it keeps looking, so it recovers without an online event', async () => {
  let browserOnline = false;
  const calls: string[] = [];
  const fetch: FetchLike = (url) => {
    calls.push(url);
    return Promise.resolve(new Response(new Uint8Array(RECORDED[url])));
  };
  // Like the map: it asks for tiles only when told something changed.
  const l: TileLoader = new TileLoader({ tiles: TILES, fetch, retryAfterMs: 20, online: () => browserOnline, onChange: () => l.request(TERRAIN, 0, bytes) });
  l.request(TERRAIN, 0, bytes);
  assert.equal(l.state, 'offline');
  // The retry time passes with the browser still offline, and again once it is back.
  await new Promise((r) => setTimeout(r, 30));
  assert.equal(calls.length, 0);
  browserOnline = true;
  await new Promise((r) => setTimeout(r, 60));
  assert.ok(calls.length >= 1, 'probed without any online event');
  await settle();
  assert.equal(l.state, 'online');
  l.destroy();
});

test('a retry time that passes while paused still probes once resumed', async () => {
  let now = 0;
  let fail = true;
  const calls: string[] = [];
  const fetch: FetchLike = (url) => {
    calls.push(url);
    return fail ? Promise.reject(new TypeError('Failed to fetch')) : Promise.resolve(new Response(new Uint8Array(RECORDED[url])));
  };
  const l: TileLoader = new TileLoader({ tiles: TILES, fetch, offlineAfter: 1, retryAfterMs: 20, now: () => now });
  l.request(TERRAIN, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.state, 'offline');
  l.setPaused(true);
  fail = false;
  now = 100;
  await new Promise((r) => setTimeout(r, 40));
  l.setPaused(false);
  l.request(TERRAIN, 0, bytes);
  await settle();
  await settle();
  assert.equal(l.state, 'online');
  l.destroy();
});

test('a byte budget on decoded tiles: past it the oldest go, whatever the entry count', async () => {
  const net = recorded();
  const gone: string[] = [];
  const l = new TileLoader({ tiles: TILES, fetch: net.fetch, cacheEntries: 100, cacheBytes: 100_000, weigh: (v) => (v as { n: number }).n });
  for (const u of [TERRAIN, IMAGERY, VECTOR]) {
    l.request(u, 0, async () => ({ u, n: 40_000 }), (v) => gone.push(v.u));
    await settle();
    await settle();
  }
  assert.deepEqual(gone, [TERRAIN]);
  assert.equal(l.status(VECTOR), 'ready');
});
