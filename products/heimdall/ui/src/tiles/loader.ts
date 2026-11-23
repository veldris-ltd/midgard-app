/* The tile loader: every outbound tile request goes through here. [FR-H-1] [NFR-PRV-1]
 *
 * - Only URLs on the build's allow-list (`TileSet.allowed`), so HTTPS only.
 * - No cookies or other credentials, no referrer, and no redirects (a redirect could
 *   lead off the list).
 * - Each request ends after 15 s (`AbortSignal.timeout`, with the cancel signal); a
 *   timeout is a network failure.
 * - The body is read as a stream and capped (`MAX_TILE_BYTES`: 1 MB for terrain, 2 MB
 *   for imagery and vector tiles); a larger `Content-Length` is refused before reading.
 *   An oversize tile is remembered as "missing", like a tile that will not decode.
 * - A bounded cache of decoded tiles. A 4xx answer (but 429) is "missing"; a 5xx or
 *   429, or a decode that ran out of time (`TileDecodeTimeout`), is retried after the
 *   retry time.
 * - A few requests at a time, most urgent first; `retain` cancels whatever the view
 *   no longer needs, queued or in flight.
 * - Paused (`setPaused`), it sends nothing and cancels what it has: the public build
 *   pauses after a camera move driven by the tracks (src/tilePause.ts).
 * - Offline: after a few network failures in a row, or when nothing has ever loaded
 *   and every request has failed with none left, or when the browser says it is
 *   offline, it stops sending and reports `offline`. After the retry time it calls
 *   `onChange`, so the view asks again, and lets one request through as a probe.
 * Plain TypeScript with an injected `fetch`, so node:test runs it on recorded tiles. */
import { LruCache } from './cache.ts';
import { MAX_TILE_BYTES, type TileSet } from './hosts.ts';

export type FetchLike = (url: string, init: RequestInit) => Promise<Response>;
export type LoaderState = 'online' | 'offline';
export type TileStatus = 'none' | 'pending' | 'ready' | 'missing' | 'failed';

/** A decode that ran out of time: a failure retried later, not a missing tile. */
export class TileDecodeTimeout extends Error {
  constructor(message = 'tiles: decoding took too long') {
    super(message);
    this.name = 'TileDecodeTimeout';
  }
}

/** How long one request, body included, may take. */
export const FETCH_TIMEOUT_MS = 15_000;

export interface LoaderOptions {
  fetch: FetchLike;
  /** The build's tile sources: the allow-list, and each source's byte cap. */
  tiles: TileSet;
  /** One request's time limit, body included. */
  fetchTimeoutMs?: number;
  /** Requests in flight at once. */
  maxInFlight?: number;
  /** Decoded tiles kept. */
  cacheEntries?: number;
  /** The most bytes of decoded tiles kept, by `weigh` (default: no byte limit). */
  cacheBytes?: number;
  /** A decoded tile's size in bytes, for `cacheBytes`. */
  weigh?: (value: unknown) => number;
  /** Network failures in a row that mean offline. */
  offlineAfter?: number;
  /** How long a failed tile, or the offline state, waits before a retry. */
  retryAfterMs?: number;
  now?: () => number;
  /** The browser's own view, e.g. `navigator.onLine`. */
  online?: () => boolean;
  /** Something loaded, failed or changed state. */
  onChange?: () => void;
}

interface Cached {
  value: unknown;
  dispose?: (v: never) => void;
}

interface Pending {
  url: string;
  priority: number;
  decode: (b: ArrayBuffer) => Promise<unknown>;
  dispose?: (v: never) => void;
  ctrl: AbortController | null;
  /** When it was sent. */
  sentAt: number;
}

const TILE_FETCH_INIT: Readonly<RequestInit> = {
  method: 'GET',
  mode: 'cors',
  credentials: 'omit',
  referrerPolicy: 'no-referrer',
  redirect: 'error',
  cache: 'default',
};

export class TileLoader {
  readonly stats = { fetched: 0, cancelled: 0, failed: 0, oversize: 0 };
  private readonly opts: Required<Omit<LoaderOptions, 'onChange' | 'online' | 'cacheBytes' | 'weigh'>> & Pick<LoaderOptions, 'onChange' | 'online' | 'cacheBytes' | 'weigh'>;
  private paused = false;
  private readonly cache: LruCache<Cached>;
  private readonly pending = new Map<string, Pending>();
  private readonly failedAt = new Map<string, number>();
  private inFlight = 0;
  private failuresInARow = 0;
  private offlineSince = 0;
  private scheduled = false;
  private destroyed = false;
  private stateNow: LoaderState = 'online';
  private retryTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(o: LoaderOptions) {
    this.opts = {
      maxInFlight: 6,
      cacheEntries: 256,
      offlineAfter: 3,
      retryAfterMs: 30_000,
      fetchTimeoutMs: FETCH_TIMEOUT_MS,
      now: () => Date.now(),
      ...o,
    };
    const weigh = o.weigh;
    this.cache = new LruCache<Cached>(this.opts.cacheEntries, (_k, c) => c.dispose?.(c.value as never), {
      maxWeight: o.cacheBytes,
      weigh: weigh ? (c) => (c.value === null ? 0 : weigh(c.value)) : undefined,
    });
  }

  get state(): LoaderState {
    return this.stateNow;
  }

  /** Cache entries this returns true for (tiles on screen) are not evicted. */
  set protect(fn: (url: string) => boolean) {
    this.cache.protect = fn;
  }

  status(url: string): TileStatus {
    const c = this.cache.peek(url);
    if (c) return c.value === null ? 'missing' : 'ready';
    if (this.pending.has(url)) return 'pending';
    const f = this.failedAt.get(url);
    if (f !== undefined) {
      if (this.opts.now() - f < this.opts.retryAfterMs) return 'failed';
      this.failedAt.delete(url);
    }
    return 'none';
  }

  get isPaused(): boolean {
    return this.paused;
  }

  /**
   * Paused, nothing is sent and every queued or in-flight request is cancelled; what is
   * cached stays readable. Resuming tells the view to ask again.
   */
  setPaused(paused: boolean): void {
    if (paused === this.paused) return;
    this.paused = paused;
    if (paused) {
      this.retain(new Set());
    } else {
      this.opts.onChange?.();
      this.schedule();
    }
  }

  /** Failures still remembered (a test diagnostic: they are pruned at their retry time). */
  get failureRecords(): number {
    return this.failedAt.size;
  }

  /** The decoded tile, null when the source has none, undefined when not loaded. */
  get<T>(url: string): T | null | undefined {
    const c = this.cache.get(url);
    return c ? (c.value as T | null) : undefined;
  }

  /** Asks for a tile; lower priority numbers go first. Throws for a URL off the list. */
  request<T>(url: string, priority: number, decode: (b: ArrayBuffer) => Promise<T>, dispose?: (v: T) => void): void {
    if (this.opts.tiles.allowed(url) === null) throw new Error(`tiles: not on the allow-list: ${url}`);
    if (this.destroyed || this.paused) return;
    if (this.cache.has(url)) {
      this.cache.get(url);
      return;
    }
    const p = this.pending.get(url);
    if (p) {
      p.priority = Math.min(p.priority, priority);
      this.schedule();
      return;
    }
    if (this.status(url) === 'failed' && this.stateNow === 'online') return;
    if (this.opts.online && !this.opts.online()) this.goOffline();
    this.pending.set(url, { url, priority, decode, dispose: dispose as ((v: never) => void) | undefined, ctrl: null, sentAt: 0 });
    this.schedule();
  }

  /** Cancels every queued or in-flight request not in `keep`. */
  retain(keep: ReadonlySet<string>): void {
    for (const [url, p] of [...this.pending]) {
      if (keep.has(url)) continue;
      this.pending.delete(url);
      if (p.ctrl) {
        p.ctrl.abort();
        this.stats.cancelled++;
      }
    }
  }

  destroy(): void {
    this.destroyed = true;
    if (this.retryTimer) clearTimeout(this.retryTimer);
    this.retryTimer = null;
    this.retain(new Set());
    this.cache.clear();
  }

  private schedule(): void {
    if (this.scheduled) return;
    this.scheduled = true;
    queueMicrotask(() => {
      this.scheduled = false;
      this.pump();
    });
  }

  private pump(): void {
    if (this.destroyed || this.paused) return;
    let slots = this.opts.maxInFlight - this.inFlight;
    if (this.stateNow === 'offline') {
      if (this.opts.online && !this.opts.online()) {
        // Look again later: the browser's `online` event is not always sent.
        if (!this.retryTimer) this.armRetry();
        return;
      }
      const wait = this.opts.retryAfterMs - (this.opts.now() - this.offlineSince);
      if (wait > 0) {
        // Too early for a probe. A timer may fire a moment before the clock says the
        // retry time has come: keep one armed for the rest, or a view that renders
        // only on demand (the map) would never probe again.
        if (!this.retryTimer) this.armRetry(wait);
        return;
      }
      // One probe at a time until the network answers.
      slots = Math.min(slots, 1 - this.inFlight);
    }
    if (slots <= 0) return;
    const queued = [...this.pending.values()].filter((p) => !p.ctrl).sort((a, b) => a.priority - b.priority);
    for (const p of queued.slice(0, slots)) this.start(p);
  }

  private start(p: Pending): void {
    const ctrl = new AbortController();
    p.ctrl = ctrl;
    p.sentAt = this.opts.now();
    this.inFlight++;
    const current = () => this.pending.get(p.url) === p;
    // What came back: a tile, nothing usable, a server that cannot serve it now, or no
    // answer at all.
    const settle = (outcome: { value: unknown } | 'missing' | 'server' | 'network') => {
      if (outcome === 'network') {
        this.stats.failed++;
        this.markFailed(p, current());
        this.failuresInARow++;
        if (this.stateNow === 'offline') {
          // A failed probe waits another retry time. A request sent before the loader
          // went offline, failing only now (at its time limit, say), does not push the
          // next probe further away.
          if (p.sentAt >= this.offlineSince) {
            this.offlineSince = this.opts.now();
            this.armRetry();
          }
        } else if (this.failuresInARow >= this.opts.offlineAfter) {
          this.goOffline();
        }
        return;
      }
      this.answered();
      if (outcome === 'server') this.markFailed(p, current());
      else if (outcome === 'missing') { if (current()) this.store(p, null); }
      else if (current()) this.store(p, outcome.value);
      else (p.dispose as ((v: unknown) => void) | undefined)?.(outcome.value);
    };
    const maxBytes = MAX_TILE_BYTES[this.opts.tiles.allowed(p.url) ?? 'vector'];
    const signal = anySignal([ctrl.signal, AbortSignal.timeout(this.opts.fetchTimeoutMs)]);
    // Cancelled by the view: no outcome at all. Anything else that stops the request
    // (the time limit, a dropped connection) is a network failure.
    const failed = () => {
      if (!ctrl.signal.aborted) settle('network');
    };
    this.opts
      .fetch(p.url, { ...TILE_FETCH_INIT, signal })
      .then(
        async (res) => {
          this.stats.fetched++;
          if (res.status >= 500 || res.status === 429) {
            void res.body?.cancel().catch(() => {});
            return settle('server');
          }
          if (!res.ok) {
            void res.body?.cancel().catch(() => {});
            return settle('missing');
          }
          let body: ArrayBuffer | 'oversize';
          try {
            body = await readCapped(res, maxBytes, signal);
          } catch {
            return failed();
          }
          if (body === 'oversize') {
            this.stats.oversize++;
            return settle('missing');
          }
          let value: unknown;
          try {
            value = await p.decode(body);
          } catch (e) {
            if (ctrl.signal.aborted) return;
            return settle(e instanceof Error && e.name === 'TileDecodeTimeout' ? 'server' : 'missing');
          }
          settle({ value });
        },
        failed,
      )
      .finally(() => {
        this.inFlight--;
        // Nothing has ever loaded, every request has failed and none is left to try:
        // that is offline too, however few tiles the view asked for.
        if (this.stats.fetched === 0 && this.failuresInARow > 0 && this.inFlight === 0 && this.pending.size === 0) this.goOffline();
        this.opts.onChange?.();
        this.schedule();
      });
  }

  /** Remembers a failure until its retry time, pruning the ones whose time has passed. */
  private markFailed(p: Pending, current: boolean): void {
    if (current) this.pending.delete(p.url);
    const now = this.opts.now();
    for (const [u, t] of this.failedAt) if (now - t >= this.opts.retryAfterMs) this.failedAt.delete(u);
    this.failedAt.set(p.url, now);
  }

  /** After the retry time, tells the view to ask again, which lets a probe through. */
  private armRetry(delayMs = this.opts.retryAfterMs): void {
    if (this.retryTimer) clearTimeout(this.retryTimer);
    if (this.destroyed) return;
    const t = setTimeout(() => {
      this.retryTimer = null;
      if (this.destroyed) return;
      this.opts.onChange?.();
      this.schedule();
    }, Math.max(1, delayMs));
    (t as { unref?: () => void }).unref?.();
    this.retryTimer = t;
  }

  private store(p: Pending, value: unknown): void {
    this.pending.delete(p.url);
    this.failedAt.delete(p.url);
    this.cache.set(p.url, { value, dispose: value === null ? undefined : p.dispose });
  }

  private answered(): void {
    this.failuresInARow = 0;
    if (this.stateNow === 'offline') {
      this.stateNow = 'online';
      if (this.retryTimer) clearTimeout(this.retryTimer);
      this.retryTimer = null;
    }
  }

  private goOffline(): void {
    if (this.stateNow === 'offline') return;
    this.stateNow = 'offline';
    this.offlineSince = this.opts.now();
    this.armRetry();
    this.opts.onChange?.();
  }
}

/** Aborts when any of `signals` does, with its reason (`AbortSignal.any` where it exists). */
function anySignal(signals: AbortSignal[]): AbortSignal {
  const any = (AbortSignal as { any?: (s: AbortSignal[]) => AbortSignal }).any;
  if (any) return any.call(AbortSignal, signals);
  const c = new AbortController();
  for (const s of signals) {
    if (s.aborted) {
      c.abort(s.reason);
      break;
    }
    s.addEventListener('abort', () => c.abort(s.reason), { once: true });
  }
  return c.signal;
}

/**
 * The body, read as a stream and stopped once it passes `max` bytes ('oversize'); a
 * `Content-Length` over `max` is refused before reading. Throws when `signal` aborts.
 */
export async function readCapped(res: Response, max: number, signal: AbortSignal): Promise<ArrayBuffer | 'oversize'> {
  const declared = res.headers.get('content-length');
  if (declared !== null && /^\s*\d+\s*$/.test(declared) && Number(declared) > max) {
    void res.body?.cancel().catch(() => {});
    return 'oversize';
  }
  if (signal.aborted) throw signal.reason;
  if (!res.body) {
    const b = await res.arrayBuffer();
    return b.byteLength > max ? 'oversize' : b;
  }
  const reader = res.body.getReader();
  // A cancel resolves a pending read as done, so an abort ends even a stalled body.
  const stop = () => void reader.cancel(signal.reason).catch(() => {});
  signal.addEventListener('abort', stop, { once: true });
  const chunks: Uint8Array[] = [];
  let total = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (signal.aborted) throw signal.reason;
      if (done) break;
      total += value.byteLength;
      if (total > max) {
        void reader.cancel().catch(() => {});
        return 'oversize';
      }
      chunks.push(value);
    }
  } finally {
    signal.removeEventListener('abort', stop);
  }
  const out = new Uint8Array(total);
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.byteLength;
  }
  return out.buffer;
}
