/* A least-recently-used cache for decoded tiles and the meshes built from them.
 * [FR-H-1]
 *
 * Bounded by entry count; `protect` keeps entries still on screen from being evicted
 * (the cache may run over its capacity until they are released, and `trim` catches
 * up). Every value that leaves goes through `onEvict`, so GPU textures and geometry
 * are disposed of exactly once. Pure. */

export class LruCache<V> {
  readonly capacity: number;
  /** Entries this returns true for are never evicted. */
  protect: (key: string) => boolean = () => false;
  private readonly map = new Map<string, V>();
  private readonly onEvict: (key: string, value: V) => void;

  constructor(capacity: number, onEvict: (key: string, value: V) => void = () => {}) {
    this.capacity = Math.max(1, Math.floor(capacity));
    this.onEvict = onEvict;
  }

  get size(): number {
    return this.map.size;
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
      if (old !== value) this.onEvict(key, old);
    }
    this.map.set(key, value);
    this.trim();
  }

  delete(key: string): boolean {
    if (!this.map.has(key)) return false;
    const v = this.map.get(key) as V;
    this.map.delete(key);
    this.onEvict(key, v);
    return true;
  }

  /** Evicts, oldest first, until within capacity or only protected entries (and the
   *  most recent one, never evicted by its own arrival) remain. */
  trim(): void {
    if (this.map.size <= this.capacity) return;
    for (const key of [...this.map.keys()].slice(0, -1)) {
      if (this.map.size <= this.capacity) break;
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
