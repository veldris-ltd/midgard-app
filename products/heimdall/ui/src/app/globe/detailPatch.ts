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
 * The caller lays it on the terrain surface (over `Detail.box`). Online it needs the internet;
 * without it the bundled map stays.
 *
 * The offline build (the Mac app) reads the same tiles recorded by tools/mac-app/
 * capture-tiles.py from the app's /tiles/ instead (tilesReplay.ts): the finest recorded zoom
 * at or below the one wanted, the close-up clipped to the recorded box (the UK and Ireland;
 * the bundled map elsewhere), tiles not recorded drawn as sea. The tile hosts are not in
 * that build.
 * Attribution while the close-up shows (Globe3D): IMAGERY_CREDIT with the satellite imagery
 * (EOX's own wording for this layer, from their WMTS capabilities), TERRAIN_CREDIT always
 * (the Terrain Tiles attribution list, the sources over the UK, Ireland and Europe shown). */
import { OFFLINE } from '../offline';
import { clipToRecorded, parseTilesManifest, pickRecorded, recordedTileUrl, type TilePick, type TilesManifest } from './tilesReplay';

export const IMAGERY_CREDIT = 'EOxCloudless https://cloudless.eox.at by EOX IT Services GmbH (Contains modified Copernicus Sentinel data 2016)';
export const IMAGERY_LICENCE = 'CC BY 4.0';
export const IMAGERY_LINK = 'https://cloudless.eox.at';
export const TERRAIN_CREDIT = 'Terrain: Mapzen Terrain Tiles (AWS Open Data) · United Kingdom terrain data © Environment Agency copyright and/or database right 2015. All rights reserved · Europe terrain data produced using Copernicus data and information funded by the European Union - EU-DEM layers · SRTM, GMTED2010 courtesy of the U.S. Geological Survey · ETOPO1 U.S. National Oceanic and Atmospheric Administration';
/** The whole Terrain Tiles attribution list (github.com/tilezen/joerd docs/attribution.md). */
export const TERRAIN_CREDIT_FULL = 'Mapzen; ArcticDEM terrain data DEM(s) were created from DigitalGlobe, Inc., imagery and funded under National Science Foundation awards 1043681, 1559691, and 1542736; Australia terrain data © Commonwealth of Australia (Geoscience Australia) 2017; Austria terrain data © offene Daten Österreichs – Digitales Geländemodell (DGM) Österreich; Canada terrain data contains information licensed under the Open Government Licence – Canada; Europe terrain data produced using Copernicus data and information funded by the European Union - EU-DEM layers; Global ETOPO1 terrain data U.S. National Oceanic and Atmospheric Administration; Mexico terrain data source: INEGI, Continental relief, 2016; New Zealand terrain data Copyright 2011 Crown copyright (c) Land Information New Zealand and the New Zealand Government (All rights reserved); Norway terrain data © Kartverket; United Kingdom terrain data © Environment Agency copyright and/or database right 2015. All rights reserved; United States 3DEP (formerly NED) and global GMTED2010 and SRTM terrain data courtesy of the U.S. Geological Survey.';

export interface Box {
  west: number;
  east: number;
  south: number;
  north: number;
}

export type DetailStyle = 'tactical' | 'satellite' | 'dark';

export interface Detail {
  /** The area the image covers: the box asked for (offline, clipped to the recording). */
  box: Box;
  zoom: number;
  /** The satellite imagery is in the image (show IMAGERY_CREDIT). */
  imagery: boolean;
  canvas: HTMLCanvasElement;
  /** Height in metres at a point of the box (from the same elevation as the image). */
  heightAt: (lat: number, lon: number) => number;
}

const TILE = 256;
const MAX_TILES = 100;
/** EOX's sea colour (its tile for open sea), under imagery tiles not recorded. */
const SEA = '#182c4e';
/** 0 m in the terrarium encoding (R * 256 + G + B / 256 - 32768), under terrain tiles not
 *  recorded or not loaded, so they read as sea level, not -32768 m (no hillshade specks). */
const SEA_LEVEL = '#800000';

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

/** The tiles of `pick`, drawn into one mosaic canvas (tiles not stored left blank, or
 * `fill`). */
async function mosaic(pick: TilePick, url: (z: number, x: number, y: number) => string, signal?: AbortSignal, fill?: string) {
  const { z, x0, x1, y0, y1 } = pick;
  const c = document.createElement('canvas');
  c.width = (x1 - x0 + 1) * TILE;
  c.height = (y1 - y0 + 1) * TILE;
  const ctx = c.getContext('2d', { willReadFrequently: true })!;
  if (fill) { ctx.fillStyle = fill; ctx.fillRect(0, 0, c.width, c.height); }
  const jobs: Promise<void>[] = [];
  for (let x = x0; x <= x1; x++) {
    for (let y = y0; y <= y1; y++) {
      if (!pick.stored(x, y)) continue;
      jobs.push(loadImage(url(z, x, y), signal).then((img) => { ctx.drawImage(img, (x - x0) * TILE, (y - y0) * TILE); }));
    }
  }
  const done = await Promise.allSettled(jobs);
  if (signal?.aborted) throw new Error('aborted');
  if (jobs.length > 0 && done.every((d) => d.status === 'rejected')) throw new Error('tiles unreachable');
  return { c, ctx, x0, y0 };
}

/** Every tile at `z` covering the box (the live sources have them all), or null if too many. */
function allTiles(box: Box, z: number): TilePick | null {
  const x0 = Math.floor(tileX(box.west, z)), x1 = Math.floor(tileX(box.east, z));
  const y0 = Math.floor(tileY(box.north, z)), y1 = Math.floor(tileY(box.south, z));
  if ((x1 - x0 + 1) * (y1 - y0 + 1) > MAX_TILES) return null;
  return { z, x0, x1, y0, y1, stored: () => true };
}

/** What to read for a close-up: the area, and the terrain and imagery tiles. */
interface Plan {
  box: Box;
  dem: TilePick;
  demUrl: (z: number, x: number, y: number) => string;
  /** Null when there is no imagery to read (then the relief colours are used). */
  sat: TilePick | null;
  satUrl: (z: number, x: number, y: number) => string;
}

/** Online: the tile hosts, at the zoom for the box (imagery one finer). */
async function livePlan(box: Box, style: DetailStyle): Promise<Plan> {
  const z = zoomFor(box);
  const dem = allTiles(box, z);
  if (!dem) throw new Error('too many tiles');
  return {
    box, dem, demUrl: (z, x, y) => `https://s3.amazonaws.com/elevation-tiles-prod/terrarium/${z}/${x}/${y}.png`,
    sat: style === 'satellite' ? allTiles(box, Math.min(13, z + 1)) : null,
    satUrl: (z, x, y) => `https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless_3857/default/g/${z}/${y}/${x}.jpg`,
  };
}

// Offline: the manifest, read once.
let recorded: Promise<TilesManifest> | null = null;

function recordedManifest(): Promise<TilesManifest> {
  if (!recorded) {
    recorded = fetch('/tiles/manifest.json', { cache: 'no-store' })
      .then((res) => {
        if (!res.ok) throw new Error(res.status === 404 ? 'no close-up tiles in this recording' : `HTTP ${res.status}`);
        return res.json();
      })
      .then(parseTilesManifest);
    recorded.catch(() => { recorded = null; });
  }
  return recorded;
}

/** Offline: the recorded tiles in the app's /tiles/, the box clipped to the recording. */
async function recordedPlan(box: Box, style: DetailStyle): Promise<Plan> {
  const m = await recordedManifest();
  const clipped = clipToRecorded(box, m.box);
  if (!clipped) throw new Error('outside the recorded tiles');
  // The zoom for the area drawn (the clipped box), capped by the recorded levels.
  const z = zoomFor(clipped);
  const dem = pickRecorded(m.sources.terrarium, clipped, z, MAX_TILES);
  if (!dem) throw new Error('no recorded terrain here');
  return {
    box: clipped, dem, demUrl: (z, x, y) => recordedTileUrl('terrarium', z, x, y),
    sat: style === 'satellite' ? pickRecorded(m.sources.s2cloudless, clipped, Math.min(13, z + 1), MAX_TILES) : null,
    satUrl: (z, x, y) => recordedTileUrl('s2cloudless', z, x, y),
  };
}

/** A build-time choice, so the offline build has no tile host in it. */
const planDetail: (box: Box, style: DetailStyle) => Promise<Plan> = OFFLINE ? recordedPlan : livePlan;

/** Build the detail image for the box in the given style. */
export async function buildDetail(want: Box, style: DetailStyle, signal?: AbortSignal): Promise<Detail> {
  const plan = await planDetail(want, style);
  if (signal?.aborted) throw new Error('aborted');
  const box = plan.box;
  const z = plan.dem.z;
  // About 2048 pixels on the longer side (square for a whole view).
  const midLat = (box.north + box.south) / 2;
  const aspect = (box.north - box.south) / ((box.east - box.west) * Math.cos((midLat * Math.PI) / 180));
  const W = aspect > 1 ? Math.max(1, Math.round(2048 / aspect)) : 2048;
  const H = aspect > 1 ? 2048 : Math.max(1, Math.round(2048 * aspect));

  // Elevation, sampled onto the output's lat/lon pixels.
  const dem = await mosaic(plan.dem, plan.demUrl, signal, SEA_LEVEL);
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
  if (style === 'satellite' && plan.sat) {
    const sz = plan.sat.z;
    const im = await mosaic(plan.sat, plan.satUrl, signal, SEA).catch(() => null);
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
  return { box, zoom: z, imagery: sat !== null, canvas: out, heightAt };
}
