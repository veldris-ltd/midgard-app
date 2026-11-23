/* Line geometry for the globe: outlines and the graticule as segment pairs, ready for
 * a three.js LineSegments buffer. Pure, so it is tested without WebGL. [FR-H-1] */
import { toXyz } from './projection.ts';

/**
 * Flat [lon, lat, ...] lines to segment pairs on a sphere of `radius`. A segment
 * longer than `maxStepDeg` is split, so the line follows the curve rather than
 * cutting under the surface.
 */
export function lineSegments(lines: readonly Float64Array[], radius: number, maxStepDeg: number): Float32Array {
  const out: number[] = [];
  const push = (lon: number, lat: number) => {
    const p = toXyz(lat, lon, 0, radius);
    out.push(p.x, p.y, p.z);
  };
  for (const l of lines) {
    for (let i = 0; i + 3 < l.length; i += 2) {
      const lon0 = l[i], lat0 = l[i + 1], lon1 = l[i + 2], lat1 = l[i + 3];
      const steps = Math.max(1, Math.ceil(Math.max(Math.abs(lon1 - lon0), Math.abs(lat1 - lat0)) / maxStepDeg));
      for (let s = 0; s < steps; s++) {
        const a = s / steps, b = (s + 1) / steps;
        push(lon0 + (lon1 - lon0) * a, lat0 + (lat1 - lat0) * a);
        push(lon0 + (lon1 - lon0) * b, lat0 + (lat1 - lat0) * b);
      }
    }
  }
  return Float32Array.from(out);
}

/** Meridians and parallels every `everyDeg`, sampled every `stepDeg`; no line at the poles. */
export function graticule(everyDeg: number, stepDeg: number): Float64Array[] {
  const lines: Float64Array[] = [];
  for (let lon = -180; lon < 180; lon += everyDeg) {
    const pts: number[] = [];
    for (let lat = -90 + everyDeg; lat <= 90 - everyDeg + 1e-9; lat += stepDeg) pts.push(lon, lat);
    pts.push(lon, 90 - everyDeg);
    lines.push(Float64Array.from(pts));
  }
  for (let lat = -90 + everyDeg; lat < 90 - 1e-9; lat += everyDeg) {
    const pts: number[] = [];
    for (let lon = -180; lon <= 180; lon += stepDeg) pts.push(lon, lat);
    lines.push(Float64Array.from(pts));
  }
  return lines;
}
