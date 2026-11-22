/* Camera presets, the REGION list, and the camera summary for screen readers.
 * [FR-H-1]
 *
 * WORLD, UK & IRELAND, NORTH SEA and CHANNEL are fixed. SECTOR NORTH is fitted to the
 * positioned tracks of the picture, so it frames whatever the console is showing. */
import { CAMERA_FOV_DEG, type CameraState, clampAltitude } from './camera.ts';
import { EARTH_RADIUS_KM } from './projection.ts';

export interface Bounds {
  south: number;
  north: number;
  west: number;
  east: number;
}

export interface Region {
  id: string;
  label: string;
  camera: CameraState;
  /** The area the region names; WORLD has none and never counts as "over" it. */
  bounds?: Bounds;
}

const RAD = Math.PI / 180;
const KM_PER_DEG = (Math.PI * EARTH_RADIUS_KM) / 180;

export const PRESETS: readonly Region[] = [
  { id: 'world', label: 'WORLD', camera: { lat: 30, lon: -10, altKm: 20000 } },
  {
    id: 'uk-ireland',
    label: 'UK & IRELAND',
    camera: { lat: 54.6, lon: -4.0, altKm: 2200 },
    bounds: { south: 49.5, north: 61.0, west: -11.0, east: 2.5 },
  },
  {
    id: 'north-sea',
    label: 'NORTH SEA',
    camera: { lat: 56.5, lon: 3.0, altKm: 2000 },
    bounds: { south: 51.0, north: 61.5, west: -3.5, east: 9.0 },
  },
  {
    id: 'channel',
    label: 'CHANNEL',
    camera: { lat: 50.1, lon: -2.0, altKm: 900 },
    bounds: { south: 48.5, north: 51.2, west: -6.0, east: 2.0 },
  },
];

const SECTOR_NORTH_FALLBACK: Region = {
  id: 'sector-north',
  label: 'SECTOR NORTH',
  camera: { lat: 57.0, lon: -1.5, altKm: 900 },
  bounds: { south: 55.0, north: 59.0, west: -5.5, east: 2.5 },
};

/** Fits SECTOR NORTH to the tracks that carry a position; a fixed view when none do. */
export function sectorNorth(tracks: readonly { lat?: number; lon?: number }[]): Region {
  const pts = tracks.filter(
    (t): t is { lat: number; lon: number } => Number.isFinite(t.lat) && Number.isFinite(t.lon),
  );
  if (pts.length === 0) return SECTOR_NORTH_FALLBACK;
  const lats = pts.map((p) => p.lat);
  const lons = pts.map((p) => p.lon);
  const south = Math.min(...lats) - 0.5;
  const north = Math.max(...lats) + 0.5;
  const west = Math.min(...lons) - 1;
  const east = Math.max(...lons) + 1;
  const lat = (south + north) / 2;
  const lon = (west + east) / 2;
  const spanKm = Math.max((north - south) * KM_PER_DEG, (east - west) * KM_PER_DEG * Math.cos(lat * RAD));
  const altKm = clampAltitude((spanKm * 1.15) / (2 * Math.tan((CAMERA_FOV_DEG * RAD) / 2)));
  return { id: 'sector-north', label: 'SECTOR NORTH', camera: { lat, lon, altKm }, bounds: { south, north, west, east } };
}

/** The REGION list: the fixed presets, then SECTOR NORTH. */
export function buildRegions(tracks: readonly { lat?: number; lon?: number }[]): Region[] {
  return [...PRESETS, sectorNorth(tracks)];
}

function contains(b: Bounds, lat: number, lon: number): boolean {
  return lat >= b.south && lat <= b.north && lon >= b.west && lon <= b.east;
}

function area(b: Bounds): number {
  return (b.north - b.south) * (b.east - b.west) * Math.cos(((b.north + b.south) / 2) * RAD);
}

/** The smallest region whose bounds hold the point, or null over open globe. */
export function regionAt(lat: number, lon: number, regions: readonly Region[]): Region | null {
  let best: Region | null = null;
  for (const r of regions) {
    if (!r.bounds || !contains(r.bounds, lat, lon)) continue;
    if (!best || area(r.bounds) < area(best.bounds!)) best = r;
  }
  return best;
}

function grouped(n: number): string {
  return String(Math.round(n)).replace(/\B(?=(\d{3})+(?!\d))/g, ',');
}

/** One sentence for a screen reader: altitude, position and the region below. */
export function describeCamera(c: CameraState, regions: readonly Region[]): string {
  const ns = c.lat < 0 ? 'S' : 'N';
  const ew = c.lon < 0 ? 'W' : 'E';
  const over = regionAt(c.lat, c.lon, regions)?.label ?? 'open globe';
  return `Camera ${grouped(c.altKm)} km above ${Math.abs(c.lat).toFixed(2)}° ${ns}, ${Math.abs(c.lon).toFixed(2)}° ${ew}, over ${over}.`;
}
