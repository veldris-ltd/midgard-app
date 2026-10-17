/* Live road traffic from TomTom Traffic Flow vector tiles (free developer key,
 * VITE_TOMTOM_KEY in products/heimdall/ui/.env.local, which git ignores), drawn as crisp
 * lines like the rest of the map. [FR-H-1] [FR-P3-3]
 *
 * Each road segment in a flow tile carries `traffic_level`, its current speed as a
 * fraction of free-flow speed, and `road_closure`. Segments are sorted into four classes:
 * light (75 % of free flow or better), moderate (40-75 %), heavy (under 40 %) and closed.
 * The UK and Ireland overview uses zoom 7 tiles (major roads, ~60 tiles a read, every 5
 * minutes while on); the globe reads finer tiles for the area in view when close in
 * (loadTrafficLines with the view's box). Needs the internet; attribution "TomTom". */
import * as React from 'react';
import { VectorTile } from '@mapbox/vector-tile';
import Pbf from 'pbf';
import { RADAR_BOX } from './radar';

export interface TrafficBox {
  west: number;
  east: number;
  south: number;
  north: number;
}

/** Polylines of [lat, lon] by traffic class. */
export interface TrafficLines {
  light: [number, number][][];
  moderate: [number, number][][];
  heavy: [number, number][][];
  closed: [number, number][][];
}

export type TrafficState =
  | { status: 'off' }
  | { status: 'loading' }
  | { status: 'ready'; lines: TrafficLines; at: number }
  | { status: 'unavailable'; reason: string };

const KEY = (import.meta as { env?: Record<string, string | undefined> }).env?.VITE_TOMTOM_KEY;
const OVERVIEW_ZOOM = 7;
const REFRESH_MS = 5 * 60 * 1000;
const MAX_TILES = 80;

const tileX = (lon: number, z: number) => ((lon + 180) / 360) * 2 ** z;
const tileY = (lat: number, z: number) => {
  const r = (lat * Math.PI) / 180;
  return ((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) * 2 ** z;
};
const lonOf = (x: number, z: number) => (x / 2 ** z) * 360 - 180;
const latOf = (y: number, z: number) => (Math.atan(Math.sinh(Math.PI * (1 - (2 * y) / 2 ** z))) * 180) / Math.PI;

/** The traffic for a box from zoom-`z` flow tiles. */
export async function loadTrafficLines(box: TrafficBox, z: number, signal?: AbortSignal): Promise<TrafficLines> {
  if (!KEY) throw new Error('no TomTom key (VITE_TOMTOM_KEY)');
  const x0 = Math.floor(tileX(box.west, z)), x1 = Math.floor(tileX(box.east, z));
  const y0 = Math.floor(tileY(box.north, z)), y1 = Math.floor(tileY(box.south, z));
  if ((x1 - x0 + 1) * (y1 - y0 + 1) > MAX_TILES) throw new Error('too many traffic tiles');
  const out: TrafficLines = { light: [], moderate: [], heavy: [], closed: [] };
  const jobs: Promise<void>[] = [];
  for (let x = x0; x <= x1; x++) {
    for (let y = y0; y <= y1; y++) {
      jobs.push(fetch(`https://api.tomtom.com/traffic/map/4/tile/flow/relative/${z}/${x}/${y}.pbf?key=${KEY}`, { signal })
        .then((r) => (r.ok ? r.arrayBuffer() : Promise.reject(new Error(`HTTP ${r.status}`))))
        .then((buf) => {
          const layer = new VectorTile(new Pbf(buf)).layers['Traffic flow'];
          if (!layer) return;
          for (let i = 0; i < layer.length; i++) {
            const f = layer.feature(i);
            if (f.type !== 2) continue;
            const p = f.properties as { traffic_level?: number; road_closure?: boolean };
            const level = typeof p.traffic_level === 'number' ? p.traffic_level : 1;
            const cls = p.road_closure ? 'closed' : level < 0.4 ? 'heavy' : level < 0.75 ? 'moderate' : 'light';
            for (const ring of f.loadGeometry()) {
              out[cls].push(ring.map((pt: { x: number; y: number }) => [latOf(y + pt.y / layer.extent, z), lonOf(x + pt.x / layer.extent, z)] as [number, number]));
            }
          }
        }));
    }
  }
  const done = await Promise.allSettled(jobs);
  if (signal?.aborted) throw new Error('aborted');
  if (done.every((d) => d.status === 'rejected')) throw new Error('traffic tiles unreachable');
  return out;
}

/** The UK and Ireland overview while `on`, refreshed every 5 minutes. */
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
      loadTrafficLines(RADAR_BOX, OVERVIEW_ZOOM, ctrl.signal)
        .then((lines) => { if (live) setState({ status: 'ready', lines, at: Date.now() }); })
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
    case 'ready': return `TRAFFIC ${new Date(s.at).toISOString().slice(11, 16)}Z · TOMTOM`;
  }
}

/** Line colours by class, shared by the globe and the 2D map. */
export const TRAFFIC_COLOURS: Record<keyof TrafficLines, string> = {
  light: '#2fd16b',
  moderate: '#ffb020',
  heavy: '#ff3b30',
  closed: '#9b1c31',
};
