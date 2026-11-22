/* The decode pool, with stand-in workers: replies, the time limit per tile, and a
 * stuck or crashed worker replaced without holding up the rest. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { type DecodeJob, type DecodeReply, DecodePool, type WorkerLike } from './decodePool.ts';

/** A stand-in worker: answers with the byte count, or hangs, or crashes, by kind. */
class FakeWorker implements WorkerLike {
  static made: FakeWorker[] = [];
  onmessage: ((e: MessageEvent<DecodeReply>) => void) | null = null;
  onerror: ((e: unknown) => void) | null = null;
  terminated = false;
  transferred: Transferable[][] = [];
  private readonly behaviour: (job: DecodeJob) => 'answer' | 'hang' | 'crash' | 'refuse';
  constructor(behaviour: (job: DecodeJob) => 'answer' | 'hang' | 'crash' | 'refuse') {
    this.behaviour = behaviour;
    FakeWorker.made.push(this);
  }
  postMessage(job: DecodeJob, transfer: Transferable[]): void {
    this.transferred.push(transfer);
    const b = this.behaviour(job);
    setTimeout(() => {
      if (this.terminated) return;
      if (b === 'answer') this.onmessage?.({ data: { id: job.id, ok: true, value: job.bytes.byteLength } } as MessageEvent<DecodeReply>);
      else if (b === 'refuse') this.onmessage?.({ data: { id: job.id, ok: false, name: 'TileRefused', message: 'vector tile too complex' } } as MessageEvent<DecodeReply>);
      else if (b === 'crash') this.onerror?.(new Error('boom'));
    }, 1);
  }
  terminate(): void {
    this.terminated = true;
  }
}

test('decodes in a worker, transferring the bytes, and passes a refusal through', async () => {
  FakeWorker.made = [];
  const pool = new DecodePool(() => new FakeWorker((j) => (j.z === 99 ? 'refuse' : 'answer')));
  const buf = new ArrayBuffer(10);
  assert.equal(await pool.decode<number>('vector', buf, 8), 10);
  assert.deepEqual(FakeWorker.made[0].transferred[0], [buf]);
  await assert.rejects(pool.decode('vector', new ArrayBuffer(1), 99), (e: unknown) => e instanceof Error && e.name === 'TileRefused');
  pool.dispose();
});

test('a tile over the time limit fails with TileDecodeTimeout; its worker is replaced, the others carry on', async () => {
  FakeWorker.made = [];
  const pool = new DecodePool(() => new FakeWorker((j) => (j.z === 1 ? 'hang' : 'answer')), { size: 2, timeoutMs: 30 });
  const t0 = Date.now();
  const slow = pool.decode('vector', new ArrayBuffer(1), 1);
  const fast = pool.decode<number>('vector', new ArrayBuffer(5), 2);
  assert.equal(await fast, 5, 'the other worker answers meanwhile');
  await assert.rejects(slow, (e: unknown) => e instanceof Error && e.name === 'TileDecodeTimeout');
  assert.ok(Date.now() - t0 >= 25);
  assert.equal(pool.replaced, 1);
  assert.ok(FakeWorker.made[0].terminated, 'the stuck worker is terminated');
  // The pool still works, never on the terminated worker, and can grow back to two.
  const sent = FakeWorker.made[0].transferred.length;
  const again = await Promise.all([2, 3, 4].map((z) => pool.decode<number>('vector', new ArrayBuffer(z), z)));
  assert.deepEqual(again, [2, 3, 4]);
  assert.equal(FakeWorker.made[0].transferred.length, sent);
  assert.equal(FakeWorker.made.length, 3);
  pool.dispose();
});

test('a worker that crashes fails its tile and is replaced', async () => {
  FakeWorker.made = [];
  let first = true;
  const pool = new DecodePool(() => new FakeWorker(() => {
    const b = first ? 'crash' : 'answer';
    first = false;
    return b;
  }), { size: 1 });
  await assert.rejects(pool.decode('terrain', new ArrayBuffer(1)), /worker failed/);
  assert.equal(await pool.decode<number>('terrain', new ArrayBuffer(3)), 3);
  assert.equal(pool.replaced, 1);
  pool.dispose();
});

test('queued tiles wait for a free worker; dispose fails what is left', async () => {
  FakeWorker.made = [];
  const pool = new DecodePool(() => new FakeWorker(() => 'hang'), { size: 1, timeoutMs: 10_000 });
  const a = pool.decode('imagery', new ArrayBuffer(1));
  const b = pool.decode('imagery', new ArrayBuffer(1));
  assert.equal(FakeWorker.made.length, 1);
  pool.dispose();
  await assert.rejects(a, /closed/);
  await assert.rejects(b, /closed/);
  assert.ok(FakeWorker.made[0].terminated);
});
