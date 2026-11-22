/* The tile loader: every outbound tile request goes through here. [FR-H-1]
 *
 * - Only URLs on the host allow-list (`allowedTileUrl`), so HTTPS only.
 * - No cookies or other credentials, no referrer, and no redirects (a redirect could
 *   lead off the list).
 * - A bounded cache of decoded tiles; a 4xx answer is remembered as "missing".
 * - A few requests at a time, most urgent first; `retain` cancels whatever the view
 *   no longer needs, queued or in flight.
 * - Offline: after a few network failures in a row, or when every request so far has
 *   failed and none is left, or when the browser says it is offline, it stops
 *   sending, reports `offline`, and lets one request probe again after a while.
 * Plain TypeScript with an injected `fetch`, so node:test runs it on recorded tiles. */
import { LruCache } from './cache.ts';
import { allowedTileUrl } from './hosts.ts';

export type FetchLike = (url: string, init: RequestInit) => Promise<Response>;
export type LoaderState = 'online' | 'offline';
export type TileStatus = 'none' | 'pending' | 'ready' | 'missing' | 'failed';

export interface LoaderOptions {
  fetch: FetchLike;
  /** Requests in flight at once. */
  maxInFlight?: number;
  /** Decoded tiles kept. */
  cacheEntries?: number;
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
}

export const TILE_FETCH_INIT: Readonly<RequestInit> = {
  method: 'GET',
  mode: 'cors',
  credentials: 'omit',
  referrerPolicy: 'no-referrer',
  redirect: 'error',
  cache: 'default',
};

export class TileLoader {
  readonly stats = { fetched: 0, cancelled: 0, failed: 0 };
  private readonly opts: Required<Omit<LoaderOptions, 'onChange' | 'online'>> & Pick<LoaderOptions, 'onChange' | 'online'>;
  private readonly cache: LruCache<Cached>;
  private readonly pending = new Map<string, Pending>();
  private readonly failedAt = new Map<string, number>();
  private inFlight = 0;
  private failuresInARow = 0;
  private offlineSince = 0;
  private scheduled = false;
  private destroyed = false;
  private stateNow: LoaderState = 'online';

  constructor(o: LoaderOptions) {
    this.opts = {
      maxInFlight: 6,
      cacheEntries: 256,
      offlineAfter: 3,
      retryAfterMs: 30_000,
      now: () => Date.now(),
      ...o,
    };
    this.cache = new LruCache<Cached>(this.opts.cacheEntries, (_k, c) => c.dispose?.(c.value as never));
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
    if (f !== undefined && this.opts.now() - f < this.opts.retryAfterMs) return 'failed';
    return 'none';
  }

  /** The decoded tile, null when the source has none, undefined when not loaded. */
  get<T>(url: string): T | null | undefined {
    const c = this.cache.get(url);
    return c ? (c.value as T | null) : undefined;
  }

  /** Asks for a tile; lower priority numbers go first. Throws for a URL off the list. */
  request<T>(url: string, priority: number, decode: (b: ArrayBuffer) => Promise<T>, dispose?: (v: T) => void): void {
    if (allowedTileUrl(url) === null) throw new Error(`tiles: not on the allow-list: ${url}`);
    if (this.destroyed) return;
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
    this.pending.set(url, { url, priority, decode, dispose: dispose as ((v: never) => void) | undefined, ctrl: null });
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
    if (this.destroyed) return;
    let slots = this.opts.maxInFlight - this.inFlight;
    if (this.stateNow === 'offline') {
      if (this.opts.online && !this.opts.online()) return;
      if (this.opts.now() - this.offlineSince < this.opts.retryAfterMs) return;
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
    this.inFlight++;
    const current = () => this.pending.get(p.url) === p;
    let network = true;
    this.opts
      .fetch(p.url, { ...TILE_FETCH_INIT, signal: ctrl.signal })
      .then(async (res) => {
        network = false;
        this.stats.fetched++;
        if (res.ok) {
          const value = await p.decode(await res.arrayBuffer());
          if (current()) this.store(p, value);
          else (p.dispose as ((v: unknown) => void) | undefined)?.(value);
        } else if (res.status >= 400 && res.status < 500 && res.status !== 429) {
          if (current()) this.store(p, null);
        } else {
          throw new Error(`tiles: HTTP ${res.status}`);
        }
        this.answered();
      })
      .catch(() => {
        if (ctrl.signal.aborted) return;
        if (!network) {
          // The server answered but the tile was unusable: treat it as missing.
          if (current()) this.store(p, null);
          this.answered();
          return;
        }
        this.stats.failed++;
        if (current()) this.pending.delete(p.url);
        this.failedAt.set(p.url, this.opts.now());
        this.failuresInARow++;
        if (this.stateNow === 'offline') this.offlineSince = this.opts.now();
        else if (this.failuresInARow >= this.opts.offlineAfter) this.goOffline();
      })
      .finally(() => {
        this.inFlight--;
        // Every request so far has failed and nothing is left to try: that is offline
        // too, however few tiles the view asked for.
        if (this.failuresInARow > 0 && this.inFlight === 0 && this.pending.size === 0) this.goOffline();
        this.opts.onChange?.();
        this.schedule();
      });
  }

  private store(p: Pending, value: unknown): void {
    this.pending.delete(p.url);
    this.failedAt.delete(p.url);
    this.cache.set(p.url, { value, dispose: value === null ? undefined : p.dispose });
  }

  private answered(): void {
    this.failuresInARow = 0;
    if (this.stateNow === 'offline') this.stateNow = 'online';
  }

  private goOffline(): void {
    if (this.stateNow === 'offline') return;
    this.stateNow = 'offline';
    this.offlineSince = this.opts.now();
    this.opts.onChange?.();
  }
}
