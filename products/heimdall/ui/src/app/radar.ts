/* Rain radar for the UK and Ireland from RainViewer's public (keyless) API, as one
 * image in plain latitude/longitude so both views can lay it straight down: the 2D map
 * (linear lat/lon) and the globe's terrain box use the same box. [FR-H-1] [FR-P3-3]
 *
 * weather-maps.json names the latest radar frames; the newest past frame's Web Mercator
 * tiles at zoom 6 that cover the box are drawn into a mosaic, then reprojected row by
 * row into an equirectangular canvas (each output row is one latitude, read from the
 * mosaic row at that latitude's Mercator y). Refreshed every 10 minutes while on.
 * Needs the internet: when anything fails the caller shows "RADAR UNAVAILABLE".
 *
 * The offline build (the Mac app) reads the frames recorded by tools/mac-app/
 * capture-radar.py from the app's /radar/ instead (manifest.json and the same tiles), shows
 * the newest recorded frame as "now" and moves on one frame per step, looping
 * (radarReplay.ts). The live code is not in that build.
 * Attribution (RainViewer's terms: mandatory, with a link): "RainViewer" with the frame time
 * on the chip, and RADAR_CREDIT linked to RADAR_LINK wherever the overlay shows. */
import * as React from 'react';
import { OFFLINE } from './offline';
import { frameAt, parseManifest, recordedTileUrl, type RadarManifest } from './radarReplay';

export const RADAR_CREDIT = 'Weather data by RainViewer';
export const RADAR_LINK = 'https://www.rainviewer.com/';

/** The box drawn: the same as the globe's terrain grid and the bundled imagery. */
export const RADAR_BOX = { west: -12, east: 4, south: 48, north: 62 } as const;
const ZOOM = 6;
const TILE = 256;
const OUT_W = 1024;
const OUT_H = 896;
const REFRESH_MS = 10 * 60 * 1000;
/** Offline: how often to check whether the next recorded frame is due. */
const REPLAY_TICK_MS = 30 * 1000;

export interface RadarFrame {
  /** The equirectangular image over RADAR_BOX (transparent where there is no rain). */
  canvas: HTMLCanvasElement;
  /** The same as a data URL, for the 2D map's <image>. */
  url: string;
  /** Frame time, unix s. */
  time: number;
}

export type RadarState =
  | { status: 'off' }
  | { status: 'loading' }
  | { status: 'ready'; frame: RadarFrame }
  | { status: 'unavailable'; reason: string };

const tileX = (lon: number, z: number) => ((lon + 180) / 360) * 2 ** z;
const tileY = (lat: number, z: number) => {
  const r = (lat * Math.PI) / 180;
  return ((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) * 2 ** z;
};

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error(`tile failed: ${src}`));
    img.src = src;
  });
}

/** Draws the zoom-6 tiles over RADAR_BOX (`tileUrl(x, y)`, null to skip one) and
 * reprojects them into the equirectangular image. */
async function buildImage(tileUrl: (x: number, y: number) => string | null, time: number, signal?: AbortSignal): Promise<RadarFrame> {
  const { west, east, south, north } = RADAR_BOX;
  const x0 = Math.floor(tileX(west, ZOOM)), x1 = Math.floor(tileX(east, ZOOM));
  const y0 = Math.floor(tileY(north, ZOOM)), y1 = Math.floor(tileY(south, ZOOM));
  const mosaic = document.createElement('canvas');
  mosaic.width = (x1 - x0 + 1) * TILE;
  mosaic.height = (y1 - y0 + 1) * TILE;
  const mctx = mosaic.getContext('2d')!;
  const jobs: Promise<void>[] = [];
  for (let x = x0; x <= x1; x++) {
    for (let y = y0; y <= y1; y++) {
      const src = tileUrl(x, y);
      if (src === null) continue;
      jobs.push(loadImage(src).then((img) => {
        mctx.drawImage(img, (x - x0) * TILE, (y - y0) * TILE);
      }));
    }
  }
  const done = await Promise.allSettled(jobs);
  if (done.every((d) => d.status === 'rejected')) throw new Error('radar tiles unreachable');
  if (signal?.aborted) throw new Error('aborted');
  // Reproject: column = longitude (linear in both projections), row = latitude.
  const out = document.createElement('canvas');
  out.width = OUT_W;
  out.height = OUT_H;
  const ctx = out.getContext('2d')!;
  const sx0 = (tileX(west, ZOOM) - x0) * TILE;
  const sw = (tileX(east, ZOOM) - tileX(west, ZOOM)) * TILE;
  for (let row = 0; row < OUT_H; row++) {
    const lat = north - ((row + 0.5) / OUT_H) * (north - south);
    const sy = (tileY(lat, ZOOM) - y0) * TILE;
    ctx.drawImage(mosaic, sx0, Math.floor(sy), sw, 1, 0, row, OUT_W, 1);
  }
  return { canvas: out, url: out.toDataURL('image/png'), time };
}

/** Read the newest radar frame from RainViewer and build the image. */
async function loadLiveRadar(signal?: AbortSignal): Promise<RadarFrame> {
  const res = await fetch('https://api.rainviewer.com/public/weather-maps.json', { signal, cache: 'no-store' });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const maps = (await res.json()) as { host?: string; radar?: { past?: { time: number; path: string }[] } };
  const past = maps.radar?.past ?? [];
  const frame = past[past.length - 1];
  if (!frame) throw new Error('no radar frame');
  const host = maps.host ?? 'https://tilecache.rainviewer.com';
  // Colour scheme 2 (universal blue), smoothed, with snow shown.
  return buildImage((x, y) => `${host}${frame.path}/${TILE}/${ZOOM}/${x}/${y}/2/1_1.png`, frame.time, signal);
}

// Offline: the manifest, read once, and the replay's anchor (when it was first read, unix s).
let recorded: Promise<{ manifest: RadarManifest; anchor: number }> | null = null;
let lastRecorded: { key: string; frame: RadarFrame } | null = null;

/** The recorded frame due now, from the app's /radar/; the same image while it stays due. */
async function loadRecordedRadar(signal?: AbortSignal): Promise<RadarFrame> {
  if (!recorded) {
    recorded = fetch('/radar/manifest.json', { cache: 'no-store' })
      .then((res) => {
        if (!res.ok) throw new Error(res.status === 404 ? 'no radar in this recording' : `HTTP ${res.status}`);
        return res.json();
      })
      .then((raw) => ({ manifest: parseManifest(raw, { zoom: ZOOM, tileSize: TILE, box: RADAR_BOX }), anchor: Date.now() / 1000 }));
    recorded.catch(() => { recorded = null; });
  }
  const { manifest, anchor } = await recorded;
  const { frame, time } = frameAt(manifest, anchor, Date.now() / 1000);
  const key = `${frame.path}@${time}`;
  if (lastRecorded?.key === key) return lastRecorded.frame;
  const missing = new Set(frame.missing.map(([x, y]) => `${x}/${y}`));
  const built = await buildImage((x, y) => (missing.has(`${x}/${y}`) ? null : recordedTileUrl(frame, x, y)), time, signal);
  lastRecorded = { key, frame: built };
  return built;
}

/** Read the radar frame due now and build the image: RainViewer's newest, or offline the
 * recorded one. A build-time choice, so the offline build has no RainViewer endpoint. */
export const loadRadar: (signal?: AbortSignal) => Promise<RadarFrame> = OFFLINE ? loadRecordedRadar : loadLiveRadar;

/** The radar while `on`, refreshed every 10 minutes (offline: the next recorded frame when due). */
export function useRadar(on: boolean): RadarState {
  const [state, setState] = React.useState<RadarState>({ status: 'off' });
  React.useEffect(() => {
    if (!on) {
      setState({ status: 'off' });
      return;
    }
    let live = true;
    let ctrl = new AbortController();
    const read = () => {
      ctrl.abort();
      ctrl = new AbortController();
      loadRadar(ctrl.signal)
        .then((frame) => { if (live) setState((s) => (s.status === 'ready' && s.frame === frame ? s : { status: 'ready', frame })); })
        .catch((err) => { if (live) setState((s) => (s.status === 'ready' ? s : { status: 'unavailable', reason: err instanceof Error ? err.message : String(err) })); });
    };
    setState({ status: 'loading' });
    read();
    const timer = window.setInterval(read, OFFLINE ? REPLAY_TICK_MS : REFRESH_MS);
    return () => {
      live = false;
      ctrl.abort();
      window.clearInterval(timer);
    };
  }, [on]);
  return state;
}

/** "RADAR 12:40Z · RAINVIEWER" and friends, for the chip. */
export function radarLabel(s: RadarState): string {
  switch (s.status) {
    case 'off': return 'RADAR';
    case 'loading': return 'RADAR · LOADING';
    case 'unavailable': return 'RADAR UNAVAILABLE';
    case 'ready': return `RADAR ${new Date(s.frame.time * 1000).toISOString().slice(11, 16)}Z · RAINVIEWER`;
  }
}
