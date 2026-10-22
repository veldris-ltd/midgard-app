/* Detail for where the camera is looking. [FR-P3-3] [FR-P3-7]
 *
 * The bundled map is one image of the UK and Ireland (~280 m a pixel), so close up it is
 * stretched and soft. When the camera is close, this builds one image for just the area in
 * view from map tiles at a matching zoom (Web Mercator, reprojected to plain lat/lon):
 *   - elevation from Terrain Tiles (Mapzen, AWS Open Data; terrarium PNG, keyless, ~30 m
 *     at zoom 12), drawn as the tactical relief (hillshade, height tint, contours), or as
 *     plain relief colours for the dark map;
 *   - for the satellite map, Sentinel-2 cloudless 2016 by EOX IT Services GmbH (contains
 *     modified Copernicus Sentinel data 2016; CC BY 4.0), shaded with the same hillshade.
 * The caller lays it on the terrain surface. Needs the internet; without it the bundled
 * map stays. */
import { OFFLINE } from '../offline';

export interface Box {
  west: number;
  east: number;
  south: number;
  north: number;
}

export type DetailStyle = 'tactical' | 'satellite' | 'dark';

export interface Detail {
  box: Box;
  zoom: number;
  canvas: HTMLCanvasElement;
  /** Height in metres at a point of the box (from the same elevation as the image). */
  heightAt: (lat: number, lon: number) => number;
}

const TILE = 256;
const MAX_TILES = 100;

const tileX = (lon: number, z: number) => ((lon + 180) / 360) * 2 ** z;
const tileY = (lat: number, z: number) => {
  const r = (lat * Math.PI) / 180;
  return ((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) * 2 ** z;
};

function loadImage(src: string, signal?: AbortSignal): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error('tile failed'));
    signal?.addEventListener('abort', () => { img.src = ''; reject(new Error('aborted')); });
    img.src = src;
  });
}

/** The zoom whose tiles give about `pixels` across the box. */
export function zoomFor(box: Box, pixels = 2600): number {
  const z = Math.floor(Math.log2((pixels / TILE) * (360 / (box.east - box.west))));
  return Math.max(7, Math.min(12, z));
}

/** Tiles at `z` covering the box, drawn into one mosaic canvas. */
async function mosaic(box: Box, z: number, url: (z: number, x: number, y: number) => string, signal?: AbortSignal) {
  const x0 = Math.floor(tileX(box.west, z)), x1 = Math.floor(tileX(box.east, z));
  const y0 = Math.floor(tileY(box.north, z)), y1 = Math.floor(tileY(box.south, z));
  if ((x1 - x0 + 1) * (y1 - y0 + 1) > MAX_TILES) throw new Error('too many tiles');
  const c = document.createElement('canvas');
  c.width = (x1 - x0 + 1) * TILE;
  c.height = (y1 - y0 + 1) * TILE;
  const ctx = c.getContext('2d', { willReadFrequently: true })!;
  const jobs: Promise<void>[] = [];
  for (let x = x0; x <= x1; x++) {
    for (let y = y0; y <= y1; y++) {
      jobs.push(loadImage(url(z, x, y), signal).then((img) => { ctx.drawImage(img, (x - x0) * TILE, (y - y0) * TILE); }));
    }
  }
  const done = await Promise.allSettled(jobs);
  if (signal?.aborted) throw new Error('aborted');
  if (done.every((d) => d.status === 'rejected')) throw new Error('tiles unreachable');
  return { c, ctx, x0, y0 };
}

/** Build the detail image for the box in the given style. */
export async function buildDetail(box: Box, style: DetailStyle, signal?: AbortSignal): Promise<Detail> {
  if (OFFLINE) throw new Error('offline build: the bundled map stays');
  const z = zoomFor(box);
  const W = 2048;
  const midLat = (box.north + box.south) / 2;
  const H = Math.round(W * ((box.north - box.south) / ((box.east - box.west) * Math.cos((midLat * Math.PI) / 180))));

  // Elevation, sampled onto the output's lat/lon pixels.
  const dem = await mosaic(box, z, (z, x, y) => `https://s3.amazonaws.com/elevation-tiles-prod/terrarium/${z}/${x}/${y}.png`, signal);
  const raw = dem.ctx.getImageData(0, 0, dem.c.width, dem.c.height).data;
  const elev = new Float32Array(W * H);
  const colOf = new Float32Array(W);
  for (let i = 0; i < W; i++) colOf[i] = (tileX(box.west + ((i + 0.5) / W) * (box.east - box.west), z) - dem.x0) * TILE;
  for (let j = 0; j < H; j++) {
    const lat = box.north - ((j + 0.5) / H) * (box.north - box.south);
    const sy = Math.min(dem.c.height - 1, Math.max(0, Math.floor((tileY(lat, z) - dem.y0) * TILE)));
    for (let i = 0; i < W; i++) {
      const sx = Math.min(dem.c.width - 1, Math.max(0, Math.floor(colOf[i])));
      const k = (sy * dem.c.width + sx) * 4;
      elev[j * W + i] = raw[k] * 256 + raw[k + 1] + raw[k + 2] / 256 - 32768;
    }
  }

  // Satellite imagery for the satellite map, onto the same pixels.
  let sat: Uint8ClampedArray | null = null;
  if (style === 'satellite') {
    const sz = Math.min(13, z + 1);
    const im = await mosaic(box, sz, (z, x, y) => `https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless_3857/default/g/${z}/${y}/${x}.jpg`, signal).catch(() => null);
    if (im) {
      const out = document.createElement('canvas');
      out.width = W;
      out.height = H;
      const octx = out.getContext('2d', { willReadFrequently: true })!;
      const sx0 = (tileX(box.west, sz) - im.x0) * TILE;
      const sw = (tileX(box.east, sz) - tileX(box.west, sz)) * TILE;
      for (let j = 0; j < H; j++) {
        const lat = box.north - ((j + 0.5) / H) * (box.north - box.south);
        octx.drawImage(im.c, sx0, Math.floor((tileY(lat, sz) - im.y0) * TILE), sw, 1, 0, j, W, 1);
      }
      sat = octx.getImageData(0, 0, W, H).data;
    }
  }

  // Shade: hillshade from the north-west, then the style's colours.
  const mx = ((box.east - box.west) / W) * 111320 * Math.cos((midLat * Math.PI) / 180);
  const my = ((box.north - box.south) / H) * 111320;
  const zf = 2.2;
  const alt = (42 * Math.PI) / 180, az = (315 * Math.PI) / 180;
  const contour = z >= 11 ? 50 : 100;
  const index = contour * 5;
  const out = document.createElement('canvas');
  out.width = W;
  out.height = H;
  const octx = out.getContext('2d')!;
  const img = octx.createImageData(W, H);
  const px = img.data;
  const e = (i: number, j: number) => elev[Math.min(H - 1, Math.max(0, j)) * W + Math.min(W - 1, Math.max(0, i))];
  for (let j = 0; j < H; j++) {
    for (let i = 0; i < W; i++) {
      const h = e(i, j);
      const k = (j * W + i) * 4;
      const land = h > 0;
      const gx = ((e(i + 1, j) - e(i - 1, j)) / (2 * mx)) * zf;
      const gy = ((e(i, j + 1) - e(i, j - 1)) / (2 * my)) * zf;
      const slope = Math.atan(Math.hypot(gx, gy));
      const aspect = Math.atan2(-gx, gy);
      const shade = Math.max(0, (Math.sin(alt) * Math.cos(slope) + Math.cos(alt) * Math.sin(slope) * Math.cos(az - aspect)) / Math.sin(alt));
      let r: number, g: number, b: number;
      if (style === 'satellite' && sat) {
        const s = land ? 0.55 + 0.55 * Math.min(1.6, shade) : 1;
        r = (sat[k] / 255) * s; g = (sat[k + 1] / 255) * s; b = (sat[k + 2] / 255) * s;
      } else if (!land) {
        [r, g, b] = style === 'dark' ? [0.04, 0.06, 0.09] : [0.02, 0.035, 0.06];
      } else {
        const t = Math.sqrt(Math.min(1, h / 1000));
        const kk = Math.min(1.75, Math.max(0.18, 0.25 + 0.95 * shade)) ** 1.15;
        if (style === 'dark') {
          r = (0.16 + 0.26 * t) * kk; g = (0.20 + 0.20 * t) * kk; b = (0.14 + 0.18 * t) * kk;
        } else {
          r = (0.12 + 0.31 * t) * kk; g = (0.15 + 0.32 * t) * kk; b = (0.125 + 0.295 * t) * kk;
        }
        if (style === 'tactical') {
          // contour where the band changes against the right or lower neighbour
          const bnd = Math.floor(h / contour);
          if (bnd !== Math.floor(e(i + 1, j) / contour) || bnd !== Math.floor(e(i, j + 1) / contour)) {
            const major = Math.floor(h / index) !== Math.floor(e(i + 1, j) / index) || Math.floor(h / index) !== Math.floor(e(i, j + 1) / index);
            const w = major ? 0.7 : 0.25;
            r = r * (1 - w) + 0.42 * w; g = g * (1 - w) + 0.82 * w; b = b * (1 - w) + 0.80 * w;
          }
        }
      }
      // the coastline
      if (land !== (e(i + 1, j) > 0) || land !== (e(i, j + 1) > 0)) { r = 0.55; g = 0.7; b = 0.75; }
      px[k] = Math.min(255, r * 255);
      px[k + 1] = Math.min(255, g * 255);
      px[k + 2] = Math.min(255, b * 255);
      px[k + 3] = 255;
    }
  }
  octx.putImageData(img, 0, 0);
  const heightAt = (lat: number, lon: number) => {
    const i = Math.round(((lon - box.west) / (box.east - box.west)) * W - 0.5);
    const j = Math.round(((box.north - lat) / (box.north - box.south)) * H - 0.5);
    return e(i, j);
  };
  return { box, zoom: z, canvas: out, heightAt };
}
