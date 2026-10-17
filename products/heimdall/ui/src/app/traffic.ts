/* Live road traffic for the UK and Ireland from TomTom Traffic Flow (free developer key,
 * VITE_TOMTOM_KEY in products/heimdall/ui/.env.local, which git ignores), as one image in
 * plain latitude/longitude over the same box as the terrain and imagery, so both views
 * lay it straight down. [FR-H-1] [FR-P3-3]
 *
 * The "relative" flow tiles colour each road by current speed against its free-flow
 * speed: green flowing, orange slow, red queuing, dark red stationary. Zoom 7 Web Mercator
 * tiles covering the box are drawn into a mosaic, then reprojected row by row (as for the
 * rain radar). Refreshed every 5 minutes while on (about 60 tiles a read, well inside the
 * free daily allowance). Needs the internet; attribution "TomTom" wherever it shows. */
import * as React from 'react';
import { RADAR_BOX } from './radar';

const ZOOM = 7;
const TILE = 256;
const OUT_W = 2048;
const OUT_H = 1792;
const REFRESH_MS = 5 * 60 * 1000;
const KEY = (import.meta as { env?: Record<string, string | undefined> }).env?.VITE_TOMTOM_KEY;

export interface TrafficFrame {
  canvas: HTMLCanvasElement;
  url: string;
  /** When it was read, unix ms. */
  at: number;
}

export type TrafficState =
  | { status: 'off' }
  | { status: 'loading' }
  | { status: 'ready'; frame: TrafficFrame }
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
    img.onerror = () => reject(new Error('tile failed'));
    img.src = src;
  });
}

export async function loadTraffic(signal?: AbortSignal): Promise<TrafficFrame> {
  if (!KEY) throw new Error('no TomTom key (VITE_TOMTOM_KEY)');
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
      jobs.push(loadImage(`https://api.tomtom.com/traffic/map/4/tile/flow/relative/${ZOOM}/${x}/${y}.png?key=${KEY}&tileSize=${TILE}&thickness=8`)
        .then((img) => { mctx.drawImage(img, (x - x0) * TILE, (y - y0) * TILE); }));
    }
  }
  const done = await Promise.allSettled(jobs);
  if (done.every((d) => d.status === 'rejected')) throw new Error('traffic tiles unreachable');
  if (signal?.aborted) throw new Error('aborted');
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
  return { canvas: out, url: out.toDataURL('image/png'), at: Date.now() };
}

/** Live traffic while `on`, refreshed every 5 minutes. */
export function useTraffic(on: boolean): TrafficState {
  const [state, setState] = React.useState<TrafficState>({ status: 'off' });
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
      loadTraffic(ctrl.signal)
        .then((frame) => { if (live) setState({ status: 'ready', frame }); })
        .catch((err) => { if (live) setState((s) => (s.status === 'ready' ? s : { status: 'unavailable', reason: err instanceof Error ? err.message : String(err) })); });
    };
    setState({ status: 'loading' });
    read();
    const timer = window.setInterval(read, REFRESH_MS);
    return () => {
      live = false;
      ctrl.abort();
      window.clearInterval(timer);
    };
  }, [on]);
  return state;
}

export function trafficLabel(s: TrafficState): string {
  switch (s.status) {
    case 'off': return 'TRAFFIC';
    case 'loading': return 'TRAFFIC · LOADING';
    case 'unavailable': return 'TRAFFIC UNAVAILABLE';
    case 'ready': return `TRAFFIC ${new Date(s.frame.at).toISOString().slice(11, 16)}Z · TOMTOM`;
  }
}
