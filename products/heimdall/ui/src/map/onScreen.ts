/* Which vector tiles the map draws and asks for this frame, within a byte cap on what
 * is on screen. [FR-H-1]
 *
 * The cache's byte budget (64 MB) cannot evict tiles that are on screen, so without a
 * cap of their own those could add up without limit. Going outwards from the centre,
 * each visible tile is drawn from itself or the nearest loaded ancestor (up to
 * `ancestorLevels` up); every distinct decoded tile used is counted once. A tile that
 * would take the total over `maxBytes` is not used: an ancestor already counted, or
 * one that still fits, stands in, or nothing is drawn there. Once the cap is reached
 * no further tiles are requested for the view. At most `maxRequested` tiles are asked
 * for at once, nearest first. Pure. */
import { type Tile, parent } from '../tiles/tileMath.ts';

/** The most bytes of decoded vector tiles on screen at once. */
export const ON_SCREEN_BYTES = 128 * 1024 * 1024;

export interface ViewPlan<T> {
  /** Not loaded yet, and to be asked for, nearest first. */
  request: Tile[];
  /** Per visible tile (by index), the tile drawn in its place and its data, or null. */
  draw: ({ from: Tile; data: T; up: number } | null)[];
  /** Tiles whose cache entries the frame uses (drawn, or known missing). */
  used: Tile[];
  /** Bytes of distinct decoded tiles drawn. */
  bytes: number;
  /** The cap was reached: some tiles were not requested or not drawn from themselves. */
  capped: boolean;
}

/**
 * `get` gives a tile's decoded data, null when the source has none (nothing is drawn
 * there, as before), or undefined when it is not loaded.
 */
export function planView<T extends { bytes: number }>(
  tiles: readonly Tile[],
  get: (t: Tile) => T | null | undefined,
  key: (t: Tile) => string,
  o: { maxRequested: number; maxBytes: number; ancestorLevels: number },
): ViewPlan<T> {
  const counted = new Set<string>();
  const plan: ViewPlan<T> = { request: [], draw: [], used: [], bytes: 0, capped: false };
  tiles.forEach((tile, i) => {
    let drawn: ViewPlan<T>['draw'][number] = null;
    let t: Tile | null = tile;
    for (let up = 0; t && up <= o.ancestorLevels; up++, t = parent(t)) {
      const d = get(t);
      if (d === undefined) {
        if (up === 0 && !plan.capped && i < o.maxRequested) plan.request.push(t);
        continue;
      }
      if (d === null) {
        plan.used.push(t);
        break;
      }
      const k = key(t);
      if (!counted.has(k)) {
        if (plan.bytes + d.bytes > o.maxBytes) {
          // Over the cap: no more requests for this view, and look further up.
          plan.capped = true;
          continue;
        }
        counted.add(k);
        plan.bytes += d.bytes;
        plan.used.push(t);
      }
      drawn = { from: t, data: d, up };
      break;
    }
    plan.draw.push(drawn);
  });
  return plan;
}
