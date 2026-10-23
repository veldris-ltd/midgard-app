/* The vertex grid the close-up detail (detailPatch.ts) is laid on: cells about square on the
 * ground, the long side `long` cells and the short side at least `minShort`, so a box clipped
 * to a narrow strip at the edge of the recorded tiles stays a small mesh. Pure, so it is unit
 * tested (test/detailGrid.test.ts). */

export interface GridBox { west: number; east: number; south: number; north: number }

/** Cells across (gw) and down (gh) for `box` (degrees). */
export function detailGrid(box: GridBox, long = 256, minShort = 16): { gw: number; gh: number } {
  const cos = Math.max(0.2, Math.cos((((box.north + box.south) / 2) * Math.PI) / 180));
  const w = Math.max(1e-9, (box.east - box.west) * cos), h = Math.max(1e-9, box.north - box.south);
  const short = (r: number) => Math.min(long, Math.max(minShort, Math.round(long * r)));
  return w >= h ? { gw: long, gh: short(h / w) } : { gw: short(w / h), gh: long };
}

/** Triangle indices for a gw x gh cell grid of (gw + 1) x (gh + 1) vertices, rows first. */
export function gridIndex(gw: number, gh: number): Uint32Array {
  const out = new Uint32Array(gw * gh * 6);
  let n = 0;
  for (let j = 0; j < gh; j++) {
    for (let i = 0; i < gw; i++) {
      const a = j * (gw + 1) + i, b = a + 1, c = a + gw + 1, e = c + 1;
      out[n++] = a; out[n++] = c; out[n++] = b; out[n++] = b; out[n++] = c; out[n++] = e;
    }
  }
  return out;
}
