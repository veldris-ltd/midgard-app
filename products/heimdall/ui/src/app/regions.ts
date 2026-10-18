/* The regions the console can show, each a lat/lon box. Only the regions switched on are
 * read from the gateway (bbox filter), so a laptop is not asked to download and draw the
 * whole world; with FOCUS on, only the region under the pointer (or, on touch, the one the
 * camera looks at) is drawn and the rest show as outlines. Satellites and space weather
 * are global and always shown. [FR-H-1] [FR-P3-7] */
import type { PresetId } from './globe/GlobeView';

export interface Region {
  id: string;
  label: string;
  /** [west, south, east, north], degrees. */
  box: [number, number, number, number];
  preset: PresetId;
}

export const REGION_LIST: Region[] = [
  { id: 'uk', label: 'UK & Ireland', box: [-12, 48, 4, 62], preset: 'uk' },
  { id: 'baltic', label: 'Baltic', box: [9, 53, 32, 66], preset: 'baltic' },
  { id: 'balkans', label: 'Balkans', box: [13, 34, 30, 48.5], preset: 'balkans' },
  { id: 'mideast', label: 'East Mediterranean', box: [25, 27, 45, 42.5], preset: 'mideast' },
  { id: 'gulf', label: 'Gulf', box: [44, 12, 62, 32], preset: 'gulf' },
  { id: 'russia', label: 'Western Russia', box: [19, 43, 60, 71], preset: 'russia' },
  { id: 'fareast', label: 'Siberia & Far East', box: [60, 40, 180, 75], preset: 'fareast' },
  { id: 'taiwan', label: 'Taiwan', box: [117, 20, 124, 27], preset: 'taiwan' },
  { id: 'singapore', label: 'Singapore & Malaysia', box: [99, -1, 106, 7], preset: 'singapore' },
  { id: 'india', label: 'India', box: [66, 6, 92, 36], preset: 'india' },
  { id: 'australia', label: 'Australia', box: [112, -45, 155, -10], preset: 'australia' },
  { id: 'nz', label: 'New Zealand', box: [165, -48, 179.9, -33], preset: 'nz' },
  { id: 'canada', label: 'Canada', box: [-141, 41, -52, 70], preset: 'canada' },
  { id: 'safrica', label: 'South Africa', box: [15, -36, 34, -21], preset: 'safrica' },
];

/** Everything outside the named regions too (no filter at all). */
export const WORLD_ID = 'world';

export const inBox = ([w, s, e, n]: Region['box'], lat: number, lon: number) =>
  lat >= s && lat <= n && (w <= e ? lon >= w && lon <= e : lon >= w || lon <= e);

export const regionAt = (enabled: Region[], lat: number, lon: number): Region | null =>
  enabled.find((r) => inBox(r.box, lat, lon)) ?? null;

const KEY = 'heimdall.regions';
const FOCUS_KEY = 'heimdall.regions.focus';

export function readRegions(): string[] {
  try {
    const v = JSON.parse(window.localStorage.getItem(KEY) ?? 'null');
    if (Array.isArray(v) && v.every((x) => typeof x === 'string')) return v;
  } catch { /* storage blocked or bad value: default */ }
  return ['uk'];
}

export function saveRegions(ids: string[]): void {
  try { window.localStorage.setItem(KEY, JSON.stringify(ids)); } catch { /* still applies */ }
}

export function readFocus(): boolean {
  try { return window.localStorage.getItem(FOCUS_KEY) === '1'; } catch { return false; }
}

export function saveFocus(on: boolean): void {
  try { window.localStorage.setItem(FOCUS_KEY, on ? '1' : '0'); } catch { /* still applies */ }
}
