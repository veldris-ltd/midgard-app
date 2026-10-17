/* Recorded history for replay and trails, shared by the 2D canvas and the 3D globe.
 * [FR-P3-7] [FR-H-1]
 *
 * The store is a map from entity key (`HeimdallTrackDetail.key`, the id the history API
 * uses) to its points, oldest first. A window is read once and then topped up with only
 * the newest minutes; `mergeHistory` appends the new points and drops the ones that have
 * slid out of the window (keeping one before it, so interpolation at the window's start
 * still has a bracketing point).
 *
 * Replay places each track at the replay time by linear interpolation in lat, lon and
 * altitude between the two points either side of it. A track is hidden outside its
 * point span, with a short grace after its last point so a track that was reported a
 * few seconds before "now" does not blink out at the live end. */
import { UK_AND_IRELAND } from '@midgard/sdk';
import type {
  HeimdallHistory,
  HeimdallHistoryPoint,
  HeimdallHistoryTrack,
  HeimdallRegion,
  HeimdallTrack,
} from '@midgard/sdk';

export type HistoryStore = Map<string, HeimdallHistoryTrack>;

const FT_PER_M = 3.28084;
const R_NM = 3440.065;
const D2R = Math.PI / 180;

/** The key a track's history is filed under. */
export const keyOf = (t: HeimdallTrack): string => t.detail?.key ?? t.id;

/** Merge a read into the store (a new map, so React sees the change). */
export function mergeHistory(store: HistoryStore, h: HeimdallHistory, keepFrom: number): HistoryStore {
  const out: HistoryStore = new Map();
  for (const [id, t] of store) out.set(id, t);
  for (const t of h.tracks) {
    const prev = out.get(t.id);
    if (!prev) {
      out.set(t.id, { ...t, points: [...t.points] });
      continue;
    }
    const last = prev.points.length ? prev.points[prev.points.length - 1][0] : -Infinity;
    const fresh = t.points.filter((p) => p[0] > last);
    out.set(t.id, fresh.length ? { ...prev, points: prev.points.concat(fresh) } : prev);
  }
  for (const [id, t] of out) {
    const i = firstAtOrAfter(t.points, keepFrom);
    if (i > 1) out.set(id, { ...t, points: t.points.slice(i - 1) });
    if (!t.points.length || t.points[t.points.length - 1][0] < keepFrom) out.delete(id);
  }
  return out;
}

/** Index of the first point at or after `t` (points.length if none). */
function firstAtOrAfter(points: HeimdallHistoryPoint[], t: number): number {
  let lo = 0, hi = points.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (points[mid][0] < t) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

export interface Sample {
  lat: number;
  lon: number;
  altM: number | null;
  /** Course and ground speed between the bracketing points (null when they coincide). */
  hdg: number | null;
  spdKt: number | null;
}

/** Position at time `t`, interpolated between the bracketing points; null outside the
 *  track's span (beyond `grace` seconds after its last point). */
export function sampleAt(points: HeimdallHistoryPoint[], t: number, grace = 60): Sample | null {
  const n = points.length;
  if (!n || t < points[0][0] || t > points[n - 1][0] + grace) return null;
  const i = firstAtOrAfter(points, t);
  const j = Math.min(i, n - 1);
  let s: Sample;
  if (i >= n) s = motion(points[n - 1], null, points[n - 1], 1);
  else if (points[i][0] === t || i === 0) s = motion(points[i], null, points[i], 1);
  else s = motion(points[i - 1], points[i - 1], points[i], (t - points[i - 1][0]) / (points[i][0] - points[i - 1][0]));
  // Course and speed are the median over the steps of the last COURSE_WINDOW seconds,
  // each step at least COURSE_STEP long. Two feeds can place one aircraft miles apart
  // (one of them estimating, not reporting, its position) and take turns being last,
  // so any single step can read as an impossible speed; the median ignores it.
  const steps: { hdg: number; spdKt: number }[] = [];
  let end = j;
  for (let k = j - 1; k >= 0 && points[j][0] - points[k][0] <= COURSE_WINDOW; k--) {
    if (points[end][0] - points[k][0] < COURSE_STEP) continue;
    const c = motion(points[end], points[k], points[end], 1);
    if (c.hdg !== null && c.spdKt !== null) steps.push({ hdg: c.hdg, spdKt: c.spdKt });
    end = k;
  }
  // Too few steps to outvote a bad one: leave course and speed to what the track
  // itself reported (the caller falls back to it).
  if (steps.length < 3) return { ...s, hdg: null, spdKt: null };
  steps.sort((x, y) => x.spdKt - y.spdKt);
  const mid = steps[(steps.length - 1) >> 1];
  return { ...s, hdg: mid.hdg, spdKt: mid.spdKt };
}

/** Seconds of history a replayed course and speed are taken over, and the shortest step. */
const COURSE_WINDOW = 120;
const COURSE_STEP = 10;

function motion(at: HeimdallHistoryPoint, a: HeimdallHistoryPoint | null, b: HeimdallHistoryPoint, f: number): Sample {
  let lat = at[1], lon = at[2], altM = at[3];
  if (a && a !== b && f < 1) {
    let dlon = b[2] - a[2];
    if (dlon > 180) dlon -= 360;
    if (dlon < -180) dlon += 360;
    lat = a[1] + (b[1] - a[1]) * f;
    lon = a[2] + dlon * f;
    if (lon > 180) lon -= 360;
    if (lon < -180) lon += 360;
    altM = a[3] === null || b[3] === null ? (b[3] ?? a[3]) : a[3] + (b[3] - a[3]) * f;
  }
  if (!a || b[0] <= a[0]) return { lat, lon, altM, hdg: null, spdKt: null };
  const p1 = a[1] * D2R, p2 = b[1] * D2R, dl = (b[2] - a[2]) * D2R;
  const hdg = (Math.atan2(Math.sin(dl) * Math.cos(p2), Math.cos(p1) * Math.sin(p2) - Math.sin(p1) * Math.cos(p2) * Math.cos(dl)) / D2R + 360) % 360;
  const h = Math.sin((p2 - p1) / 2) ** 2 + Math.cos(p1) * Math.cos(p2) * Math.sin(dl / 2) ** 2;
  const nm = 2 * R_NM * Math.asin(Math.min(1, Math.sqrt(h)));
  const moved = nm > 0.01;
  return { lat, lon, altM, hdg: moved ? hdg : null, spdKt: moved ? (nm / (b[0] - a[0])) * 3600 : null };
}

/** The points between `from` and `to`, closed at `to` by the interpolated position so a
 *  trail ends exactly where the track is drawn. */
export function trailOf(points: HeimdallHistoryPoint[] | undefined, from: number, to: number): HeimdallHistoryPoint[] {
  if (!points?.length) return [];
  const i = firstAtOrAfter(points, from);
  const j = firstAtOrAfter(points, to + 1e-6);
  const out = points.slice(i, j);
  const end = sampleAt(points, to, 0);
  if (end && (!out.length || out[out.length - 1][0] < to)) out.push([to, end.lat, end.lon, end.altM]);
  return out;
}

const project = (region: HeimdallRegion, lat: number, lon: number): [number, number] => [
  ((lon - region.west) / (region.east - region.west)) * 100,
  ((region.north - lat) / (region.north - region.south)) * 100,
];

/** Pressure-altitude style text for an aircraft or balloon at `altM`. */
function airAlt(altM: number | null, onGround: boolean): string {
  if (onGround) return 'GND';
  if (altM === null) return '—';
  const ft = Math.round(altM * FT_PER_M);
  return ft >= 6000 ? `FL${Math.round(ft / 100).toString().padStart(3, '0')}` : `${ft.toLocaleString()} ft`;
}

const CLASS_OF_KIND: Record<string, string> = {
  'air-track': 'AIRCRAFT',
  'air-balloon': 'BALLOON',
  'space-satellite': 'SATELLITE',
  'event-earthquake': 'EARTHQUAKE',
  'event-flood': 'FLOOD WARNING',
};

/** Every track that has history at replay time `t`, placed there. Metadata comes from the
 *  current picture where the entity is still in it; entities that have since gone (an
 *  aircraft that landed an hour ago) are rebuilt from their history alone. */
export function replayTracks(current: HeimdallTrack[], store: HistoryStore, t: number, region: HeimdallRegion = UK_AND_IRELAND, grace = 60): HeimdallTrack[] {
  const byKey = new Map<string, HeimdallTrack>();
  for (const c of current) byKey.set(keyOf(c), c);
  const out: HeimdallTrack[] = [];
  const used = new Set<string>();
  for (const h of store.values()) {
    const s = sampleAt(h.points, t, grace);
    if (!s) continue;
    const base = byKey.get(h.id);
    const air = h.kind === 'air-track' || h.kind === 'air-balloon';
    const onGround = h.kind === 'air-track' && (s.altM === null ? !!base?.detail?.onGround : s.altM <= 15 && (s.spdKt ?? 0) < 60);
    const [x, y] = project(region, s.lat, s.lon);
    let id = base?.id ?? h.id;
    if (used.has(id)) id = h.id;
    used.add(id);
    const detail = {
      ...(base?.detail ?? {
        key: h.id, kind: h.kind, sources: [], observations: h.points.length,
        firstSeen: h.points[0][0], lastSeen: h.points[h.points.length - 1][0], estimatedSecs: 0,
      }),
      lat: s.lat,
      lon: s.lon,
      altM: s.altM ?? undefined,
      onGround,
      estimatedSecs: 0,
    };
    out.push({
      ...(base ?? { cls: CLASS_OF_KIND[h.kind] ?? h.kind.toUpperCase(), conf: 0.5, track: 'stable' as const, feeds: 0 }),
      id,
      layer: base?.layer ?? h.layer,
      x,
      y,
      lat: s.lat,
      lon: s.lon,
      hdg: Math.round(s.hdg ?? base?.hdg ?? 0),
      spd: s.spdKt !== null ? Math.round(s.spdKt) : (base?.spd ?? 0),
      alt: air ? airAlt(s.altM, onGround) : (base?.alt ?? '—'),
      altFt: air && s.altM !== null ? Math.round(s.altM * FT_PER_M) : base?.altFt,
      trail: undefined,
      geoTrail: undefined,
      options: undefined,
      detail,
    });
  }
  return out;
}

/** The read step for a window: every point for 15 minutes, thinner for longer ones, so a
 *  24-hour read stays a few thousand points per track at most. */
export function stepFor(windowSecs: number): number {
  if (windowSecs <= 15 * 60) return 0;
  if (windowSecs <= 3600) return 15;
  if (windowSecs <= 6 * 3600) return 30;
  return 60;
}
