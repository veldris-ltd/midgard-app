/* The TOPOGRAPHIC style, drawn from the terrain heights alone: hypsometric tints,
 * a hillshade lit from the north-west, and contour lines, every fifth one stronger.
 * Light and dark palettes follow the console's mode. Pure: heights in, RGBA out.
 * [FR-H-1] */

export type Rgb = readonly [number, number, number];

export interface TopoPalette {
  /** Shallow sea, and the deepest. */
  sea: Rgb;
  seaDeep: Rgb;
  /** Land colour stops by height in metres, ascending. */
  land: readonly (readonly [number, Rgb])[];
  contour: Rgb;
  /** Opacity of a contour line, and of every fifth (index) one. */
  contourAlpha: number;
  indexAlpha: number;
}

export const TOPO_LIGHT: TopoPalette = {
  sea: [196, 216, 228],
  seaDeep: [150, 182, 204],
  land: [
    [0, [214, 226, 200]],
    [200, [205, 218, 178]],
    [500, [222, 212, 168]],
    [1000, [204, 180, 140]],
    [1500, [184, 156, 126]],
    [2500, [214, 206, 198]],
    [4000, [246, 246, 246]],
  ],
  contour: [120, 92, 60],
  contourAlpha: 0.35,
  indexAlpha: 0.65,
};

export const TOPO_DARK: TopoPalette = {
  sea: [16, 28, 38],
  seaDeep: [8, 16, 24],
  land: [
    [0, [34, 42, 34]],
    [200, [40, 48, 34]],
    [500, [52, 52, 38]],
    [1000, [64, 56, 42]],
    [1500, [76, 66, 52]],
    [2500, [98, 94, 88]],
    [4000, [130, 128, 124]],
  ],
  contour: [222, 196, 140],
  contourAlpha: 0.22,
  indexAlpha: 0.45,
};

/** Metres between contours at a tile zoom. */
export function contourIntervalM(z: number): number {
  if (z <= 4) return 1000;
  if (z <= 6) return 500;
  if (z <= 8) return 200;
  if (z <= 10) return 100;
  if (z <= 12) return 50;
  return 20;
}

/** Vertical exaggeration of the hillshade: relief stays visible from far out. */
export function shadeZFactor(z: number): number {
  return Math.min(8, Math.max(1.5, 2 ** ((12 - z) / 3)));
}

const RAD = Math.PI / 180;

/**
 * Horn's hillshade, 0–1, lit from `azimuthDeg` (clockwise from north) at
 * `altitudeDeg`. Rows run north to south; edges repeat their neighbours.
 */
export function hillshade(
  h: Float32Array,
  w: number,
  rows: number,
  metresPerPixel: number,
  zFactor: number,
  azimuthDeg = 315,
  altitudeDeg = 45,
): Float32Array {
  const out = new Float32Array(w * rows);
  const zenith = (90 - altitudeDeg) * RAD;
  const az = ((360 - azimuthDeg + 90) % 360) * RAD;
  const cz = Math.cos(zenith);
  const sz = Math.sin(zenith);
  const at = (x: number, y: number) => h[Math.min(rows - 1, Math.max(0, y)) * w + Math.min(w - 1, Math.max(0, x))];
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < w; x++) {
      const a = at(x - 1, y - 1), b = at(x, y - 1), c = at(x + 1, y - 1);
      const d = at(x - 1, y), f = at(x + 1, y);
      const g = at(x - 1, y + 1), hh = at(x, y + 1), i = at(x + 1, y + 1);
      const dzdx = (c + 2 * f + i - (a + 2 * d + g)) / (8 * metresPerPixel);
      const dzdy = (g + 2 * hh + i - (a + 2 * b + c)) / (8 * metresPerPixel);
      const slope = Math.atan(zFactor * Math.hypot(dzdx, dzdy));
      const aspect = Math.atan2(dzdy, -dzdx);
      const v = cz * Math.cos(slope) + sz * Math.sin(slope) * Math.cos(az - aspect);
      out[y * w + x] = Math.max(0, v);
    }
  }
  return out;
}

const lerp = (a: Rgb, b: Rgb, t: number): Rgb =>
  [Math.round(a[0] + (b[0] - a[0]) * t), Math.round(a[1] + (b[1] - a[1]) * t), Math.round(a[2] + (b[2] - a[2]) * t)];

/** The colour for a height: the sea deepening to 4,000 m, the land by its stops. */
export function tint(p: TopoPalette, h: number): Rgb {
  if (h <= 0) return lerp(p.sea, p.seaDeep, Math.min(1, -h / 4000));
  const s = p.land;
  if (h <= s[0][0]) return s[0][1];
  for (let k = 1; k < s.length; k++) {
    if (h <= s[k][0]) return lerp(s[k - 1][1], s[k][1], (h - s[k - 1][0]) / (s[k][0] - s[k - 1][0]));
  }
  return s[s.length - 1][1];
}

/** The style's pixels for a grid of heights, row-major from the north-west. */
export function topoPixels(
  h: Float32Array,
  w: number,
  rows: number,
  metresPerPixel: number,
  interval: number,
  p: TopoPalette,
  zFactor: number,
): Uint8ClampedArray {
  const shade = hillshade(h, w, rows, metresPerPixel, zFactor);
  const flat = Math.sin(45 * RAD);
  const out = new Uint8ClampedArray(w * rows * 4);
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < w; x++) {
      const k = y * w + x;
      const v = h[k];
      let [r, g, b] = tint(p, v);
      if (v > 0) {
        // Flat ground keeps its tint; slopes lighten or darken around it.
        const f = 1 + 1.1 * (shade[k] - flat);
        r *= f; g *= f; b *= f;
        const band = Math.floor(v / interval);
        const rightBand = x < w - 1 ? Math.floor(Math.max(0, h[k + 1]) / interval) : band;
        const downBand = y < rows - 1 ? Math.floor(Math.max(0, h[k + w]) / interval) : band;
        if (band !== rightBand || band !== downBand) {
          const top = Math.max(band, rightBand, downBand);
          const a = top % 5 === 0 ? p.indexAlpha : p.contourAlpha;
          r += (p.contour[0] - r) * a;
          g += (p.contour[1] - g) * a;
          b += (p.contour[2] - b) * a;
        }
      }
      out[k * 4] = r;
      out[k * 4 + 1] = g;
      out[k * 4 + 2] = b;
      out[k * 4 + 3] = 255;
    }
  }
  return out;
}
