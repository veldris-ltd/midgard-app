/* The bundled Natural Earth outlines, decoded. [FR-H-1]
 *
 * `tools/heimdall-outlines/build.mjs` writes `data/ne-110m.json` and `data/ne-50m.json`
 * from pinned Natural Earth downloads: rings and lines quantised and delta-encoded as
 * [lon0, lat0, dlon1, dlat1, ...]. Decoded lines are flat [lon, lat, lon, lat, ...]
 * arrays in degrees. */

export type EncodedLine = number[];

export interface OutlineFile {
  format: string;
  quantum: number;
  scale: string;
  source: {
    name: string;
    licence: string;
    url: string;
    version: string;
    commit: string;
    files: { file: string; sha256: string }[];
  };
  coast?: EncodedLine[];
  borders?: EncodedLine[];
  /** Polygons, each a list of rings (outer first). */
  land?: EncodedLine[][];
}

/** Integer steps per degree; `quantum` is a decimal fraction such as 0.01. */
const steps = (quantum: number) => Math.round(1 / quantum);

/** Cumulative integer coordinates of an encoded line. */
function integers(enc: EncodedLine): Int32Array {
  const out = new Int32Array(enc.length);
  let x = 0;
  let y = 0;
  for (let i = 0; i < enc.length; i += 2) {
    x += enc[i];
    y += enc[i + 1];
    out[i] = x;
    out[i + 1] = y;
  }
  return out;
}

export function decodeLine(enc: EncodedLine, quantum: number): Float64Array {
  const n = steps(quantum);
  const ints = integers(enc);
  const out = new Float64Array(ints.length);
  for (let i = 0; i < ints.length; i++) out[i] = ints[i] / n;
  return out;
}

/**
 * Coastlines from land polygons: each ring, less the edges Natural Earth adds where a
 * polygon is cut along the antimeridian or closed along the south pole. Those are
 * not coast and would draw a false line on the globe.
 */
export function coastFromLand(polygons: EncodedLine[][], quantum: number): Float64Array[] {
  const n = steps(quantum);
  const meridian = 180 * n;
  const pole = -90 * n + 1;
  const out: Float64Array[] = [];
  for (const poly of polygons) {
    for (const enc of poly) {
      const p = integers(enc);
      const count = p.length / 2;
      const cut = (i: number) => {
        const ax = p[2 * i], ay = p[2 * i + 1], bx = p[2 * i + 2], by = p[2 * i + 3];
        return (Math.abs(ax) === meridian && bx === ax) || (ay <= pole && by <= pole);
      };
      const runs: number[][] = [];
      let run: number[] = [0];
      for (let i = 0; i < count - 1; i++) {
        if (cut(i)) {
          if (run.length > 1) runs.push(run);
          run = [i + 1];
        } else {
          run.push(i + 1);
        }
      }
      if (run.length > 1) runs.push(run);
      // A closed ring's last run continues into its first, unless a cut sits between.
      const closed = count > 2 && p[0] === p[2 * count - 2] && p[1] === p[2 * count - 1];
      if (closed && runs.length > 1 && runs[0][0] === 0 && runs[runs.length - 1].at(-1) === count - 1) {
        const last = runs.pop()!;
        runs[0] = [...last, ...runs[0].slice(1)];
      }
      for (const r of runs) {
        const line = new Float64Array(r.length * 2);
        r.forEach((idx, k) => {
          line[2 * k] = p[2 * idx] / n;
          line[2 * k + 1] = p[2 * idx + 1] / n;
        });
        out.push(line);
      }
    }
  }
  return out;
}
