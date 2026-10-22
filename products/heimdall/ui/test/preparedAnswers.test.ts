// Unit tests for the prepared Mimir answers (`node --test`, types stripped by Node).
// Each case imports a fresh copy of the module (a distinct `?case=` query), since the
// shipped store is loaded once per page.
import { test } from 'node:test';
import assert from 'node:assert/strict';

type Fetch = (url: string, init?: unknown) => Promise<{ ok: boolean; json: () => Promise<unknown> }>;

let n = 0;
async function fresh(opts: { prepared?: string; fetch?: Fetch; local?: Record<string, string> } = {}) {
  const local = { ...(opts.local ?? {}) };
  (globalThis as any).window = {
    __MIDGARD_PREPARED__: opts.prepared,
    localStorage: {
      getItem: (k: string) => (k in local ? local[k] : null),
      setItem: (k: string, v: string) => { local[k] = v; },
    },
  };
  (globalThis as any).fetch = opts.fetch ?? (() => Promise.reject(new Error('no fetch expected')));
  return import(`../src/app/preparedAnswers.ts?case=${++n}`);
}

const answer = { answer: 'Shipped.', grounded: true, citations: [], unresolved: [], inferences: [], proposed: [], considered: 3, ms: 15000 };
const file = (entries: Record<string, unknown>) => ({ version: 1, recording: 'hour-1', created: 1, entries, guide: { questions: [], alerts: [] } });
const serve = (body: unknown, calls: string[] = []): Fetch => async (url) => { calls.push(url); return { ok: true, json: async () => body }; };

test('keys normalise the question', async () => {
  const m = await fresh();
  assert.equal(m.preparedKey('ask', 'UK', '  What   has\tchanged?\n'), 'ask|UK|what has changed?');
  assert.equal(m.shippedKey('ask', 'anything', 'What has  CHANGED?'), 'ask|*|what has changed?');
  assert.equal(m.shippedKey('advise', 'power-cut:ukpn-123', ''), 'advise|entity:power-cut:ukpn-123|');
  assert.equal(m.shippedKey('advise', 'e1', ' Why? '), 'advise|entity:e1|why?');
});

test('shipped answers are found once loaded, fetched once', async () => {
  const calls: string[] = [];
  const key = 'ask|*|what has changed in the gulf watch?';
  const m = await fresh({ prepared: 'http://127.0.0.1:1/home/prepared-answers.json', fetch: serve(file({ [key]: { value: answer, at: 5 } }), calls) });
  await Promise.all([m.loadShipped(), m.loadShipped()]);
  const got = await m.findPrepared(m.shippedKey('ask', '*', 'What has changed in the Gulf watch?'), 'ask|UK|other');
  assert.deepEqual(got, { value: answer, at: 5 });
  assert.deepEqual(m.getPrepared(key), { value: answer, at: 5 });
  assert.deepEqual(calls, ['http://127.0.0.1:1/home/prepared-answers.json']);
});

test('shipped answers never expire and come before this browser', async () => {
  const key = 'advise|entity:e1|';
  const local = { [key]: { value: { from: 'local' }, at: Date.now() } };
  const m = await fresh({ prepared: 'u', fetch: serve(file({ [key]: { value: { from: 'shipped' }, at: 1 } })),
    local: { 'heimdall.mimir.prepared': JSON.stringify(local) } });
  assert.deepEqual((await m.findPrepared(key))?.value, { from: 'shipped' });
});

test('this browser is checked after the shipped store, and still expires', async () => {
  const fresh1 = { value: { from: 'local' }, at: Date.now() };
  const stale = { value: { from: 'old' }, at: Date.now() - 3 * 60 * 60 * 1000 };
  const m = await fresh({ prepared: 'u', fetch: serve(file({})),
    local: { 'heimdall.mimir.prepared': JSON.stringify({ 'advise|A-9|': fresh1, 'ask|UK|q': stale }) } });
  assert.deepEqual(await m.findPrepared('advise|entity:e1|', 'advise|A-9|'), fresh1);
  assert.equal(await m.findPrepared('ask|UK|q'), null);
});

test('unset URL: no fetch, nothing shipped', async () => {
  const m = await fresh();
  await m.loadShipped();
  assert.equal(await m.findPrepared('ask|*|q'), null);
});

test('errors and bad files are ignored', async () => {
  for (const f of [
    (() => Promise.reject(new Error('down'))) as Fetch,
    (async () => ({ ok: false, json: async () => file({ 'ask|*|q': { value: answer, at: 1 } }) })) as Fetch,
    (async () => ({ ok: true, json: async () => { throw new SyntaxError('bad json'); } })) as Fetch,
    serve({ version: 2, entries: { 'ask|*|q': { value: answer, at: 1 } } }),
    serve({ version: 1, entries: [] }),
    serve(null),
  ]) {
    const m = await fresh({ prepared: 'u', fetch: f });
    await m.loadShipped();
    assert.equal(await m.findPrepared('ask|*|q'), null);
  }
});

test('parseShipped keeps only well-formed entries', async () => {
  const m = await fresh();
  assert.deepEqual(m.parseShipped(file({
    good: { value: answer, at: 1 },
    noAt: { value: answer },
    noValue: { at: 1 },
    scalar: { value: 'text', at: 1 },
    junk: 3,
  })), { good: { value: answer, at: 1 } });
});
