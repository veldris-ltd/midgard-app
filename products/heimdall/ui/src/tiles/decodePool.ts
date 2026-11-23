/* A small pool of decode workers with a time limit per tile. [FR-H-1]
 *
 * Each worker decodes one tile at a time. A tile that takes longer than the limit
 * (about 2 s) fails with `TileDecodeTimeout`, and its worker, which may be stuck in a
 * loop, is terminated and replaced; the other workers carry on. A worker that dies
 * fails its tile the same way. The bytes are transferred, not copied. Plain
 * TypeScript with an injected worker factory, so node:test runs it with fake workers. */
import { TileDecodeTimeout } from './loader.ts';

export type DecodeKind = 'terrain' | 'imagery' | 'vector';

export interface DecodeJob {
  id: number;
  kind: DecodeKind;
  bytes: ArrayBuffer;
  /** The tile's zoom, for the vector features drawn at it. */
  z: number;
}

export type DecodeReply = { id: number; ok: true; value: unknown } | { id: number; ok: false; name: string; message: string };

/** What the pool needs of a Worker. */
export interface WorkerLike {
  onmessage: ((e: MessageEvent<DecodeReply>) => void) | null;
  onerror: ((e: unknown) => void) | null;
  postMessage(msg: DecodeJob, transfer: Transferable[]): void;
  terminate(): void;
}

/** The decode time limit per tile. */
export const DECODE_TIMEOUT_MS = 2000;

interface Slot {
  worker: WorkerLike;
  job: { id: number; resolve: (v: unknown) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> } | null;
}

interface Queued {
  job: DecodeJob;
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
}

export class DecodePool {
  /** Workers replaced after a timeout or a crash: a test diagnostic. */
  replaced = 0;
  private readonly make: () => WorkerLike;
  private readonly size: number;
  private readonly timeoutMs: number;
  private readonly slots: Slot[] = [];
  private readonly queue: Queued[] = [];
  private nextId = 1;
  private disposed = false;

  constructor(make: () => WorkerLike, o: { size?: number; timeoutMs?: number } = {}) {
    this.make = make;
    this.size = Math.max(1, o.size ?? 2);
    this.timeoutMs = o.timeoutMs ?? DECODE_TIMEOUT_MS;
  }

  /** Decodes one tile's bytes, which are transferred to the worker. */
  decode<T>(kind: DecodeKind, bytes: ArrayBuffer, z = 0): Promise<T> {
    if (this.disposed) return Promise.reject(new Error('tiles: the decode pool is closed'));
    return new Promise<T>((resolve, reject) => {
      this.queue.push({ job: { id: this.nextId++, kind, bytes, z }, resolve: resolve as (v: unknown) => void, reject });
      this.pump();
    });
  }

  dispose(): void {
    this.disposed = true;
    for (const q of this.queue.splice(0)) q.reject(new Error('tiles: the decode pool is closed'));
    for (const s of this.slots.splice(0)) {
      if (s.job) {
        clearTimeout(s.job.timer);
        s.job.reject(new Error('tiles: the decode pool is closed'));
      }
      s.worker.terminate();
    }
  }

  private pump(): void {
    while (this.queue.length) {
      let slot = this.slots.find((s) => !s.job);
      if (!slot) {
        if (this.slots.length >= this.size) return;
        slot = this.spawn();
      }
      this.start(slot, this.queue.shift() as Queued);
    }
  }

  private spawn(): Slot {
    const slot: Slot = { worker: this.make(), job: null };
    slot.worker.onmessage = (e) => {
      const r = e.data;
      const j = slot.job;
      if (!j || r.id !== j.id) return;
      clearTimeout(j.timer);
      slot.job = null;
      if (r.ok) j.resolve(r.value);
      else {
        const err = new Error(r.message);
        err.name = r.name;
        j.reject(err);
      }
      this.pump();
    };
    slot.worker.onerror = () => this.replace(slot, new Error('tiles: the decode worker failed'));
    this.slots.push(slot);
    return slot;
  }

  private start(slot: Slot, q: Queued): void {
    const timer = setTimeout(() => this.replace(slot, new TileDecodeTimeout()), this.timeoutMs);
    slot.job = { id: q.job.id, resolve: q.resolve, reject: q.reject, timer };
    try {
      slot.worker.postMessage(q.job, [q.job.bytes]);
    } catch (e) {
      this.replace(slot, e instanceof Error ? e : new Error(String(e)));
    }
  }

  /** Fails the slot's tile, terminates its worker and makes way for a new one. */
  private replace(slot: Slot, err: Error): void {
    const j = slot.job;
    if (j) clearTimeout(j.timer);
    slot.job = null;
    slot.worker.onmessage = null;
    slot.worker.onerror = null;
    slot.worker.terminate();
    const i = this.slots.indexOf(slot);
    if (i >= 0) this.slots.splice(i, 1);
    this.replaced++;
    j?.reject(err);
    if (!this.disposed) this.pump();
  }
}

/** The browser's pool: module workers from decodeWorker.ts, a file of the console's own. */
export function browserDecodePool(): DecodePool {
  return new DecodePool(
    () => new Worker(new URL('./decodeWorker.ts', import.meta.url), { type: 'module', name: 'heimdall-tile-decode' }) as unknown as WorkerLike,
  );
}
