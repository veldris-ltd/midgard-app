/* Camera presets, the REGION list, and the camera summary for screen readers.
 * [FR-H-1]
 *
 * WORLD, UK & IRELAND, NORTH SEA and CHANNEL are fixed. SECTOR NORTH is fitted to the
 * positioned tracks of the picture, so it frames whatever the console is showing.
 * WORLD looks straight down; the others open tilted 45–60°, so the curve of the
 * Earth, the horizon and the atmosphere show. */
import { CAMERA_FOV_DEG, CLOSE_UP_ALTITUDE_KM, clampAltitude } from './camera.ts';
import { EARTH_RADIUS_KM } from './projection.ts';
import { type ViewState, lookTarget, viewLookingAt } from './view.ts';

export interface Bounds {
  south: number;
  north: number;
  west: number;
  east: number;
}

export interface Region {
  id: string;
  label: string;
  camera: ViewState;
  /** The area the region names; WORLD has none and never counts as "over" it. */
  bounds?: Bounds;
}

const RAD = Math.PI / 180;
const KM_PER_DEG = (Math.PI * EARTH_RADIUS_KM) / 180;

export const PRESETS: readonly Region[] = [
  { id: 'world', label: 'WORLD', camera: { lat: 30, lon: -10, altKm: 20000, pitch: 0, heading: 0 } },
  {
    id: 'uk-ireland',
    label: 'UK & IRELAND',
    camera: viewLookingAt({ lat: 54.3, lon: -4.5 }, 1100, 45, 0),
    bounds: { south: 49.5, north: 61.0, west: -11.0, east: 2.5 },
  },
  {
    id: 'north-sea',
    label: 'NORTH SEA',
    camera: viewLookingAt({ lat: 56.5, lon: 2.5 }, 800, 50, 10),
    bounds: { south: 51.0, north: 61.5, west: -3.5, east: 9.0 },
  },
  {
    id: 'channel',
    label: 'CHANNEL',
    camera: viewLookingAt({ lat: 50.2, lon: -1.5 }, 350, 55, 75),
    bounds: { south: 48.5, north: 51.2, west: -6.0, east: 2.0 },
  },
];

/**
 * Close, tilted views of relief, listed only while map tiles are drawn: the terrain
 * reads there at 12–15 km. Without tiles the camera's 50 km floor would flatten them.
 */
export const CLOSE_UP_PRESETS: readonly Region[] = [
  {
    id: 'snowdonia',
    label: 'SNOWDONIA',
    // Yr Wyddfa from the north-east, up the Llanberis pass.
    camera: viewLookingAt({ lat: 53.068, lon: -4.076 }, 12, 60, 215, CLOSE_UP_ALTITUDE_KM),
    bounds: { south: 52.8, north: 53.25, west: -4.35, east: -3.7 },
  },
  {
    id: 'southern-uplands',
    label: 'SOUTHERN UPLANDS',
    // The Moffat hills from the south-west, past the Devil's Beef Tub.
    camera: viewLookingAt({ lat: 55.42, lon: -3.38 }, 15, 58, 40, CLOSE_UP_ALTITUDE_KM),
    bounds: { south: 55.1, north: 55.9, west: -4.6, east: -2.3 },
  },
];

const SECTOR_NORTH_FALLBACK: Region = {
  id: 'sector-north',
  label: 'SECTOR NORTH',
  camera: viewLookingAt({ lat: 57.0, lon: -1.5 }, 700, 50, 0),
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
  // Tilted, the view reaches past its centre towards the horizon, so it sits a little lower.
  const camera = viewLookingAt({ lat, lon }, altKm * 0.9, 50, 0);
  return { id: 'sector-north', label: 'SECTOR NORTH', camera, bounds: { south, north, west, east } };
}

/** The REGION list: the fixed presets, the close-ups while tiles are drawn, then SECTOR NORTH. */
export function buildRegions(tracks: readonly { lat?: number; lon?: number }[], closeUps = false): Region[] {
  return [...PRESETS, ...(closeUps ? CLOSE_UP_PRESETS : []), sectorNorth(tracks)];
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

/** The heading as a three-digit bearing. */
export function bearingText(heading: number): string {
  return String(Math.round(heading) % 360).padStart(3, '0') + '°';
}

/** The region in the middle of the view: below the camera, or ahead of it when tilted. */
export function regionInView(c: ViewState, regions: readonly Region[]): Region | null {
  const t = c.pitch > 0 ? lookTarget(c) ?? c : c;
  return regionAt(t.lat, t.lon, regions);
}

/** One sentence for a screen reader: altitude, position, tilt, heading and the region in view. */
export function describeCamera(c: ViewState, regions: readonly Region[]): string {
  const ns = c.lat < 0 ? 'S' : 'N';
  const ew = c.lon < 0 ? 'W' : 'E';
  const region = regionInView(c, regions)?.label ?? 'open globe';
  const tilted = Math.round(c.pitch) >= 1;
  const look = tilted ? `tilted ${Math.round(c.pitch)}°` : 'looking straight down';
  const where = tilted ? `looking towards ${region}` : `over ${region}`;
  const alt = c.altKm < 10 ? c.altKm.toFixed(1) : grouped(c.altKm);
  return `Camera ${alt} km above ${Math.abs(c.lat).toFixed(2)}° ${ns}, ${Math.abs(c.lon).toFixed(2)}° ${ew}, ${look}, heading ${bearingText(c.heading)}, ${where}.`;
}
