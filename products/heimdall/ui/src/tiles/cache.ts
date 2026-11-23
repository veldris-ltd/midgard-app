/* A least-recently-used cache for decoded tiles and the meshes built from them.
 * [FR-H-1]
 *
 * Bounded by entry count and, optionally, by total weight (`maxWeight`, with `weigh`
 * giving each value's, such as its bytes); `protect` keeps entries still on screen from being evicted
 * (the cache may run over its capacity until they are released, and `trim` catches
 * up). Every value that leaves goes through `onEvict`, so GPU textures and geometry
 * are disposed of exactly once. Pure. */

export class LruCache<V> {
  readonly capacity: number;
  /** Entries this returns true for are never evicted. */
  protect: (key: string) => boolean = () => false;
  private readonly map = new Map<string, V>();
  private readonly onEvict: (key: string, value: V) => void;
  private readonly maxWeight: number;
  private readonly weigh: (value: V) => number;
  private readonly weights = new Map<string, number>();
  private total = 0;

  constructor(
    capacity: number,
    onEvict: (key: string, value: V) => void = () => {},
    o: { maxWeight?: number; weigh?: (value: V) => number } = {},
  ) {
    this.capacity = Math.max(1, Math.floor(capacity));
    this.onEvict = onEvict;
    this.maxWeight = o.maxWeight ?? Infinity;
    this.weigh = o.weigh ?? (() => 0);
  }

  get size(): number {
    return this.map.size;
  }

  /** The total weight of what is held. */
  get weight(): number {
    return this.total;
  }

  private over(): boolean {
    return this.map.size > this.capacity || this.total > this.maxWeight;
  }

  has(key: string): boolean {
    return this.map.has(key);
  }

  /** The value, marked as most recently used. */
  get(key: string): V | undefined {
    if (!this.map.has(key)) return undefined;
    const v = this.map.get(key) as V;
    this.map.delete(key);
    this.map.set(key, v);
    return v;
  }

  /** The value, without marking it as used. */
  peek(key: string): V | undefined {
    return this.map.get(key);
  }

  set(key: string, value: V): void {
    if (this.map.has(key)) {
      const old = this.map.get(key) as V;
      this.map.delete(key);
      this.total -= this.weights.get(key) ?? 0;
      this.weights.delete(key);
      if (old !== value) this.onEvict(key, old);
    }
    const w = Math.max(0, this.weigh(value) || 0);
    this.map.set(key, value);
    this.weights.set(key, w);
    this.total += w;
    this.trim();
  }

  delete(key: string): boolean {
    if (!this.map.has(key)) return false;
    const v = this.map.get(key) as V;
    this.map.delete(key);
    this.total -= this.weights.get(key) ?? 0;
    this.weights.delete(key);
    this.onEvict(key, v);
    return true;
  }

  /** Evicts, oldest first, until within capacity and weight, or only protected
   *  entries (and the most recent one, never evicted by its own arrival) remain. */
  trim(): void {
    if (!this.over()) return;
    for (const key of [...this.map.keys()].slice(0, -1)) {
      if (!this.over()) break;
      if (this.protect(key)) continue;
      this.delete(key);
    }
  }

  clear(): void {
    for (const key of [...this.map.keys()]) this.delete(key);
  }

  keys(): IterableIterator<string> {
    return this.map.keys();
  }
}
