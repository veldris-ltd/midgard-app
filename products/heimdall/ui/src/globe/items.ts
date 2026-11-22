/* Tracks to globe items: what the renderer draws for each track. [FR-H-1] [AC-H-1]
 *
 * A track is placed only when it carries both `lat` and `lon`. Its altitude is parsed
 * from `alt` and multiplied by the HEIGHTS setting, so airliners at FL340 stand clear
 * of the surface at globe scale; the true altitude is kept for the tooltip. Layer
 * switches are the 2D view's, passed in as `layerOn`, and the tone follows 2D:
 * anomalous tracks are --warn, the rest --accent. */
import type { HeimdallLayerId, HeimdallTrack } from '@midgard/sdk';
import { type AltitudeTrend, parseAltitude } from './altitude.ts';
import { type Vec3, toXyz } from './projection.ts';

/** HEIGHTS: altitude exaggeration, ×4 by default. */
export const DEFAULT_EXAGGERATION = 4;
export const MIN_EXAGGERATION = 1;
export const MAX_EXAGGERATION = 20;

export type GlobeTone = 'accent' | 'warn';

export interface GlobeItem {
  id: string;
  layer: HeimdallLayerId;
  tone: GlobeTone;
  lat: number;
  lon: number;
  /** True altitude in km, unexaggerated; 0 when unknown. */
  altitudeKm: number;
  altitudeKnown: boolean;
  trend: AltitudeTrend;
  /** Marker position on the unit globe, at the exaggerated altitude. */
  position: Vec3;
  /** The point on the surface below it. */
  ground: Vec3;
  /** The vertical stem from the marker down to the ground, so its height reads at any
   *  tilt; null for a track drawn on the surface. */
  stem: [Vec3, Vec3] | null;
  /** Bearing the marker points along (aircraft and vessels with a heading), or null. */
  heading: number | null;
}

export function clampExaggeration(x: number): number {
  if (!Number.isFinite(x)) return x > 0 ? MAX_EXAGGERATION : DEFAULT_EXAGGERATION;
  return Math.min(MAX_EXAGGERATION, Math.max(MIN_EXAGGERATION, x));
}

function positioned(t: HeimdallTrack): t is HeimdallTrack & { lat: number; lon: number } {
  return (
    typeof t.lat === 'number' && typeof t.lon === 'number' &&
    Number.isFinite(t.lat) && Number.isFinite(t.lon) &&
    Math.abs(t.lat) <= 90 && Math.abs(t.lon) <= 180
  );
}

export function tracksToGlobeItems(
  tracks: readonly HeimdallTrack[],
  layerOn: (layer: HeimdallLayerId) => boolean,
  exaggeration: number,
): GlobeItem[] {
  const k = clampExaggeration(exaggeration);
  const items: GlobeItem[] = [];
  for (const t of tracks) {
    if (!positioned(t) || !layerOn(t.layer)) continue;
    const alt = parseAltitude(t.alt);
    const altitudeKm = alt ? alt.metres / 1000 : 0;
    const position = toXyz(t.lat, t.lon, altitudeKm * k);
    const ground = toXyz(t.lat, t.lon, 0);
    const pointed = (t.layer === 'air' || t.layer === 'maritime') && Number.isFinite(t.hdg);
    items.push({
      id: t.id,
      layer: t.layer,
      tone: t.track === 'anomalous' ? 'warn' : 'accent',
      lat: t.lat,
      lon: t.lon,
      altitudeKm,
      altitudeKnown: alt !== null,
      trend: alt?.trend ?? 'level',
      position,
      ground,
      stem: altitudeKm > 0 ? [position, ground] : null,
      heading: pointed ? ((t.hdg % 360) + 360) % 360 : null,
    });
  }
  return items;
}
