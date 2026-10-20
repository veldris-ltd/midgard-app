/* Heimdall over HTTP — the console's live data source. [FR-H-1]
 *
 * Reads the fused common operating picture from the `http-gateway` edge
 * (`GET /api/v1/heimdall/picture`), which authorises and audits the signed-in user
 * and fetches the picture from `heimdall-feed` over PQ-mTLS. The picture, the entity
 * catalogue and the feed list are live. Picture sync, workflows, decisions and the
 * decision-lineage stream have no service behind them yet, so they come from
 * `HeimdallMock` and the console labels them as simulated.
 *
 * Positions: Heimdall tracks carry `lat`/`lon` attributes; the operating-picture
 * canvas places tracks by percentage, so they are projected linearly into `region`
 * (default: the UK and Ireland).
 *
 * Trails: each track's reported positions are collected across fetches (the read holds
 * only the latest), so a trail starts when the console opens.
 *
 * Refresh: the picture is fetched at most every `fetchEverySecs` (each fetch is
 * audited by the gateway and by Heimdall), and every `getPicture` in between moves each
 * airborne track along its heading at its ground speed from its last reported position
 * (dead reckoning, capped at `extrapolateMaxSecs`). The next real position replaces the
 * estimate, so a turn shows late by up to one feed cycle. [FR-H-1]
 *
 * Worldwide: `getPicture` returns every positioned entity in `geo` (aircraft, balloons,
 * satellites, earthquakes, flood warnings, wherever they are) for the 3D globe, and only
 * those inside `region` in `tracks` for the 2D canvas. Wire layers map onto the
 * console's names (sea → maritime, land → ground).
 *
 * History: `getHistory` reads `GET /heimdall/history` for replay and trails; until the
 * gateway serves it (404 / 502) it rejects with `HeimdallHistoryUnavailableError` and the
 * console falls back to the trails collected here. [FR-P3-3] [FR-P3-7]
 *
 * Areas: SIGMETs, weather warnings, wind farms and road disruptions carry a `polygon`
 * ("lat,lon;lat,lon;…"), parsed into `detail.area`; one without a position is placed at
 * its outline's centre. Space weather (one entity, key SWPC, at 0,0) is not a place: it
 * is lifted out of the tracks into `picture.spaceWeather`. openAIP airspace volumes
 * (`airspace-volume`, wire layer `airspace`) carry lower/upper limits in feet with their
 * references; NASA FIRMS fire hotspots (`event-fire`) their radiative power.
 *
 * Alerts: `getAlerts` / `ackAlert` read and acknowledge `GET /heimdall/alerts` and
 * `POST /heimdall/alerts/{id}/ack`; a gateway without them (404) rejects with
 * `HeimdallAlertsUnavailableError`. [FR-H-1] [FR-H-5] */
import type { AuditRecord, PolicyContext } from '../types';
import {
  HeimdallAlertsUnavailableError,
  HeimdallHistoryUnavailableError,
  type HeimdallAlert,
  type HeimdallSpaceWeather,
  HeimdallMock,
  type HeimdallDecision,
  type HeimdallHistory,
  type HeimdallHistoryPoint,
  type HeimdallHistoryQuery,
  type HeimdallEntity,
  type HeimdallFeed,
  type HeimdallGateway,
  type HeimdallLayer,
  type HeimdallLayerId,
  type HeimdallPicture,
  type HeimdallRegion,
  type HeimdallCameraAnalysis,
  type HeimdallCameraPlayback,
  type HeimdallCameraTrend,
  type HeimdallSyncSite,
  type HeimdallTrack,
  type HeimdallWorkflow,
} from './heimdall';

/** The UK and Ireland, the region of the live ADS-B feed. */
export const UK_AND_IRELAND: HeimdallRegion = { west: -11, east: 3, south: 49, north: 61.5 };

export interface HeimdallHttpOptions {
  /** Base URL of the HTTP/JSON edge, e.g. http://127.0.0.1:7710/api/v1 */
  baseUrl: string;
  /** Bearer of the PQ-signed token from `login`. */
  token: string;
  /** The box tracks are projected from. */
  region?: HeimdallRegion;
  /** Called when the edge answers 401 (the session expired), so the shell can sign out. */
  onUnauthorized?: () => void;
  /** Fetch the picture from the gateway at most this often (default 15 s). */
  fetchEverySecs?: number;
  /** Move tracks forward from their last position for at most this long (default 300 s,
   *  Heimdall's track TTL in this deployment). */
  extrapolateMaxSecs?: number;
}

/** One entity as `heimdall-feed` serves it. */
interface WireEntity {
  id: string;
  kind: string;
  layer: string;
  sources: string[];
  firstSeen: number;
  lastSeen: number;
  observations: number;
  classification: { label: string; source: string; rationale: string } | null;
  attributes: Record<string, string | number | boolean>;
}

interface WirePicture {
  layers: { id: string; name: string; visible: boolean }[];
  entities: WireEntity[];
}

/** Labels that mark a track as needing attention. */
const ALERT_LABELS = new Set(['emergency', 'radio-failure', 'unlawful-interference']);
/** Heimdall's wire layer ids → the console's layer ids. Anything else is `other`. */
const WIRE_LAYERS: Record<string, HeimdallLayerId> = {
  air: 'air', sea: 'maritime', land: 'ground', space: 'space', events: 'events', other: 'other', airspace: 'airspace',
  utilities: 'utilities', infrastructure: 'infrastructure',
};
const layerOf = (wire: string): HeimdallLayerId => WIRE_LAYERS[wire] ?? 'other';
const wireLayer = (id: HeimdallLayerId): string =>
  Object.keys(WIRE_LAYERS).find((k) => WIRE_LAYERS[k] === id) ?? id;
/** Layers that hardly change, read every STATIC_EVERY_MS rather than with every
 *  picture read (airspace alone is most of the picture's size). [FR-H-1] */
const STATIC_LAYERS = ['airspace', 'infrastructure'];
const STATIC_EVERY_MS = 15 * 60 * 1000;

/** Reported positions kept per track for its trail (about 15 minutes at 45 s). */
const TRAIL_POINTS = 20;
/** The default class label of each entity kind, when no classification rule matched. */
const KIND_CLASS: Record<string, string> = {
  'air-track': 'AIRCRAFT',
  'air-balloon': 'BALLOON',
  'space-satellite': 'SATELLITE',
  'event-earthquake': 'EARTHQUAKE',
  'event-flood': 'FLOOD WARNING',
  'sea-vessel': 'VESSEL',
  'land-airport': 'AIRPORT',
  'event-sigmet': 'SIGMET',
  'event-weather-warning': 'WEATHER WARNING',
  'event-gnss-interference': 'GPS INTERFERENCE',
  'sea-windfarm': 'WIND FARM',
  'sea-platform': 'PLATFORM',
  'land-road-disruption': 'ROAD DISRUPTION',
  'airspace-volume': 'AIRSPACE',
  'event-fire': 'FIRE HOTSPOT',
  'land-train': 'TRAIN',
  'land-camera': 'CAMERA',
};
/** What kind of feed contributes each entity kind (the Feeds screen). */
const KIND_FEED: Record<string, string> = {
  'air-track': 'transponder (ADS-B)',
  'air-balloon': 'radiosonde telemetry',
  'space-satellite': 'orbital elements',
  'event-earthquake': 'seismic catalogue',
  'event-flood': 'flood warnings',
  'sea-vessel': 'ship transponders (AIS)',
  'land-airport': 'airport weather (METAR)',
  'event-sigmet': 'aviation hazards (SIGMET)',
  'event-weather-warning': 'weather warnings (Met Office)',
  'sea-windfarm': 'offshore wind farms',
  'sea-platform': 'offshore platforms',
  'land-road-disruption': 'road disruptions (TfL)',
  'space-weather': 'space weather (NOAA SWPC)',
  'airspace-volume': 'airspace (openAIP)',
  'event-fire': 'fire hotspots (NASA FIRMS)',
  'land-train': 'train movements (TRUST)',
  'land-camera': 'traffic and custom cameras',
};
const FT_PER_M = 3.28084;
const KT_PER_MS = 1.94384;

const num = (v: unknown): number | undefined => (typeof v === 'number' && Number.isFinite(v) ? v : undefined);
const hhmm = (unix: number): string =>
  new Date(unix * 1000).toISOString().slice(11, 16);
const ago = (unix: number): string => {
  const s = Math.max(0, Math.round(Date.now() / 1000 - unix));
  return s < 120 ? `${s}s` : `${Math.round(s / 60)}m`;
};

/** Metres above mean sea level: `alt_m` where reported, else from `alt_ft`. */
function altMetres(e: WireEntity): number | undefined {
  const m = num(e.attributes.alt_m);
  if (m !== undefined) return m;
  const ft = num(e.attributes.alt_ft);
  return ft === undefined ? undefined : ft / FT_PER_M;
}

function altitude(e: WireEntity): string {
  if (e.attributes.on_ground === true) return 'GND';
  if (e.kind === 'space-satellite') {
    const km = num(e.attributes.alt_km) ?? (altMetres(e) ?? NaN) / 1000;
    return Number.isFinite(km) ? `${Math.round(km).toLocaleString()} km` : '—';
  }
  if (e.kind === 'event-earthquake') {
    const d = num(e.attributes.depth_km);
    return d === undefined ? '—' : `${Math.round(d)} km deep`;
  }
  if (e.kind === 'airspace-volume') return airspaceLimits(e);
  if (e.kind === 'event-fire') return 'SURFACE';
  if (e.kind === 'event-sigmet') {
    const b = num(e.attributes.base_ft), t = num(e.attributes.top_ft);
    const fl = (ft: number) => (ft <= 0 ? 'SFC' : `FL${String(Math.round(ft / 100)).padStart(3, '0')}`);
    return t !== undefined ? `${fl(b ?? 0)}–${fl(t)}` : b !== undefined ? `ABV ${fl(b)}` : '—';
  }
  if (e.kind === 'event-flood' || e.kind === 'event-weather-warning' || e.kind.startsWith('sea-') && e.kind !== 'sea-vessel') return 'SURFACE';
  if (e.kind === 'land-road-disruption') return 'GND';
  const ft = num(e.attributes.alt_ft) ?? (num(e.attributes.alt_m) !== undefined ? Math.round(num(e.attributes.alt_m)! * FT_PER_M) : undefined);
  if (ft === undefined) return '—';
  return ft >= 6000 ? `FL${Math.round(ft / 100).toString().padStart(3, '0')}` : `${Math.round(ft).toLocaleString()} ft`;
}

function classOf(e: WireEntity): string {
  // SIGMETs by hazard, warnings by level: the inspector and the filter read better.
  if (!e.classification && e.kind === 'airspace-volume' && typeof e.attributes.airspace_type === 'string') return `AIRSPACE ${e.attributes.airspace_type}`.toUpperCase();
  if (!e.classification && e.kind === 'event-sigmet' && typeof e.attributes.hazard === 'string') return `SIGMET ${e.attributes.hazard}`.toUpperCase();
  if (!e.classification && e.kind === 'event-weather-warning' && typeof e.attributes.level === 'string') return `${e.attributes.level} warning`.toUpperCase();
  // Ships without a classification are labelled by their AIS ship type (cargo, tanker…).
  const shipType = e.kind === 'sea-vessel' && typeof e.attributes.ship_type === 'string' ? e.attributes.ship_type : undefined;
  return (e.classification?.label ?? shipType ?? KIND_CLASS[e.kind] ?? e.kind)
    .replace(/-/g, ' ')
    .toUpperCase();
}

const textAttr = (e: WireEntity, k: string): string | undefined => {
  const v = e.attributes[k];
  return typeof v === 'string' && v ? v : typeof v === 'number' ? String(v) : undefined;
};

/** Crewed stations, which the globe highlights. */
const isStation = (e: WireEntity): boolean =>
  e.kind === 'space-satellite' &&
  (textAttr(e, 'group') === 'stations' || /\b(ISS|ZARYA|TIANHE|CSS|TIANGONG)\b/i.test(textAttr(e, 'name') ?? ''));

/** The camera analytics' attributes on a camera, if it has been analysed. [FR-P3-5] */
function cameraAnalysis(e: WireEntity): HeimdallCameraAnalysis | undefined {
  const at = num(e.attributes.ai_at);
  if (at === undefined) return undefined;
  const pairs = (v: unknown) => (typeof v === 'string' && v ? v.split(',').map((p) => p.split(':')).filter((p) => p.length === 2) : []);
  const boxes = typeof e.attributes.ai_boxes === 'string' && e.attributes.ai_boxes
    ? e.attributes.ai_boxes.split(';').map((b) => b.split(',')).filter((b) => b.length >= 8).map((b) => ({
      cls: b[0], conf: +b[1], x: +b[2], y: +b[3], w: +b[4], h: +b[5], dx: +b[6], dy: +b[7],
    })).filter((b) => [b.conf, b.x, b.y, b.w, b.h, b.dx, b.dy].every(Number.isFinite))
    : [];
  return {
    at,
    model: textAttr(e, 'ai_model'),
    frames: num(e.attributes.ai_frames),
    counts: pairs(e.attributes.ai_counts).map(([cls, n]) => ({ cls, n: +n })),
    vehicles: num(e.attributes.ai_vehicles),
    people: num(e.attributes.ai_people),
    moving: num(e.attributes.ai_moving),
    stationary: num(e.attributes.ai_stationary),
    stopped: num(e.attributes.ai_stopped),
    queue: e.attributes.ai_queue === true,
    blocking: num(e.attributes.ai_blocking),
    wrongWay: num(e.attributes.ai_wrong_way),
    personRoad: num(e.attributes.ai_person_road),
    flow: pairs(e.attributes.ai_flow).map(([dir, n]) => ({ dir, n: +n })),
    heading: num(e.attributes.ai_heading),
    headingText: textAttr(e, 'ai_heading_text'),
    boxes,
  };
}

function watchOf(w: any): HeimdallCameraTrend['watch'] {
  return w && typeof w.until === 'number' ? { until: w.until, by: String(w.by ?? ''), everyS: typeof w.every_s === 'number' ? w.every_s : 300 } : null;
}

/** The per-frame tracks in an analysis answer, checked; undefined when absent or malformed. */
function playbackOf(v: any): HeimdallCameraPlayback | undefined {
  if (!v || typeof v.fps !== 'number' || !(v.fps > 0) || typeof v.frames !== 'number' || !Array.isArray(v.tracks)) return undefined;
  const tracks = v.tracks
    .filter((t: any) => t && typeof t.id === 'number' && typeof t.cls === 'string' && Array.isArray(t.obs))
    .map((t: any) => ({
      id: t.id, cls: t.cls, moving: t.moving === true,
      flag: t.flag === 'stopped' || t.flag === 'wrong-way' || t.flag === 'person-road' ? t.flag : undefined,
      obs: t.obs.filter((o: any) => Array.isArray(o) && o.length === 5 && o.every(Number.isFinite)) as [number, number, number, number, number][],
    }))
    .filter((t: HeimdallCameraPlayback['tracks'][number]) => t.obs.length > 0);
  return { fps: v.fps, frames: v.frames, tracks };
}

/** Runways from the feed's `lat,lon,lat,lon[,width_m];…` text (width 45 m when absent). */
function runwaysOf(geo: string | undefined): [number, number, number, number, number][] | undefined {
  if (!geo) return undefined;
  const out = geo.split(';').map((r) => r.split(',').map(Number))
    .filter((r) => (r.length === 4 || r.length === 5) && r.every(Number.isFinite))
    .map((r) => [r[0], r[1], r[2], r[3], r[4] || 45] as [number, number, number, number, number]);
  return out.length ? out : undefined;
}

/** One airspace limit: "GND", "FL195", "2,500 ft MSL". */
function limitText(ft: number | undefined, ref: string | undefined): string {
  if (ft === undefined) return '—';
  if (ft <= 0 && (ref ?? 'GND').toUpperCase() !== 'MSL') return 'GND';
  if ((ref ?? '').toUpperCase() === 'STD') return `FL${String(Math.round(ft / 100)).padStart(3, '0')}`;
  return `${Math.round(ft).toLocaleString()} ft ${(ref ?? 'MSL').toUpperCase()}`;
}

/** An airspace's vertical limits, "GND–FL195". [FR-H-1] */
function airspaceLimits(e: WireEntity): string {
  const r = (k: string) => (typeof e.attributes[k] === 'string' ? (e.attributes[k] as string) : undefined);
  return `${limitText(num(e.attributes.lower_ft), r('lower_ref'))}–${limitText(num(e.attributes.upper_ft), r('upper_ref'))}`;
}

/** An area's outline from the feed's `lat,lon;lat,lon;…` text (null when absent or bad). */
export function parsePolygon(text: unknown): [number, number][] | undefined {
  if (typeof text !== 'string' || !text) return undefined;
  const pts = text.split(';').map((p) => p.split(',').map(Number))
    .filter((p) => p.length >= 2 && Number.isFinite(p[0]) && Number.isFinite(p[1]) && Math.abs(p[0]) <= 90)
    .map((p) => [p[0], p[1]] as [number, number]);
  return pts.length >= 3 ? pts : undefined;
}

/** A utility line's `line`: parts separated by "|", each "lat,lon;lat,lon;…". */
export function parseLines(text: unknown): [number, number][][] | undefined {
  if (typeof text !== 'string' || !text) return undefined;
  const parts = text.split('|').map((part) => part.split(';').map((p) => p.split(',').map(Number))
    .filter((p) => p.length >= 2 && Number.isFinite(p[0]) && Number.isFinite(p[1]) && Math.abs(p[0]) <= 90)
    .map((p) => [p[0], p[1]] as [number, number])).filter((part) => part.length >= 2);
  return parts.length ? parts : undefined;
}

/** A road disruption's `line` ("lat,lon;lat,lon;…", the affected stretch of road) as a
 *  narrow band around it, so it is drawn like any other area: about 4 km wide, enough to
 *  see at regional zoom. [FR-H-1] */
export function lineBand(text: unknown, halfKm = 2): [number, number][] | undefined {
  if (typeof text !== 'string' || !text) return undefined;
  const pts = text.split(';').map((p) => p.split(',').map(Number))
    .filter((p) => p.length >= 2 && Number.isFinite(p[0]) && Number.isFinite(p[1]) && Math.abs(p[0]) <= 90);
  if (pts.length < 2) return undefined;
  const left: [number, number][] = [], right: [number, number][] = [];
  for (let i = 0; i < pts.length; i++) {
    const a = pts[Math.max(0, i - 1)], b = pts[Math.min(pts.length - 1, i + 1)];
    const k = Math.cos((pts[i][0] * Math.PI) / 180);
    const dx = (b[1] - a[1]) * k, dy = b[0] - a[0];
    const len = Math.hypot(dx, dy) || 1;
    const nLat = (dx / len) * (halfKm / 111.32), nLon = (-dy / len) * (halfKm / 111.32) / k;
    left.push([pts[i][0] + nLat, pts[i][1] + nLon]);
    right.push([pts[i][0] - nLat, pts[i][1] - nLon]);
  }
  return [...left, ...right.reverse()];
}

/** A time attribute (unix s or text) as "17 14:00Z". */
function when(v: unknown): string | undefined {
  if (typeof v === 'number' && Number.isFinite(v)) return new Date(v * 1000).toISOString().slice(8, 16).replace('T', ' ') + 'Z';
  if (typeof v === 'string' && v) {
    const t = Date.parse(v);
    return Number.isFinite(t) ? new Date(t).toISOString().slice(8, 16).replace('T', ' ') + 'Z' : v;
  }
  return undefined;
}

/** The kind-specific attributes the inspector lists, in display order. */
function factsOf(e: WireEntity): { label: string; value: string }[] {
  const rows: [string, string | undefined][] = [];
  const a = (k: string) => textAttr(e, k);
  const n = (k: string) => num(e.attributes[k]);
  const hm = (k: string) => { const s = n(k); return s === undefined ? undefined : new Date(s * 1000).toISOString().slice(0, 16).replace('T', ' ') + ' UTC'; };
  const unit = (k: string, u: string, dp = 0) => { const v = n(k); return v === undefined ? undefined : `${v.toLocaleString(undefined, { maximumFractionDigits: dp })} ${u}`; };
  // Utilities. [FR-H-1]
  switch (e.kind) {
    case 'utility-power-outage':
      rows.push(['OPERATOR', a('operator')], ['TYPE', e.attributes.planned === true ? 'Planned work' : 'Unplanned power cut'],
        ['CUSTOMERS', a('customers')], ['SINCE', hm('started')], ['RESTORE BY', hm('etr')], ['AREA', a('area')],
        ['POSTCODES', a('postcodes')], ['VOLTAGE', a('voltage')], ['CAUSE', a('cause')], ['REFERENCE', a('ref')]);
      return rows.filter((r): r is [string, string] => !!r[1]).map(([label, value]) => ({ label, value }));
    case 'utility-power-grid':
      rows.push(['FREQUENCY', unit('frequency_hz', 'Hz', 3)], ['5-MIN RANGE', n('frequency_min_5m') !== undefined ? `${n('frequency_min_5m')} – ${n('frequency_max_5m')} Hz` : undefined],
        ['DEMAND', unit('demand_mw', 'MW')], ['GENERATION', unit('generation_mw', 'MW')], ['NET IMPORT', unit('net_import_mw', 'MW')],
        ['WIND', n('wind_pct') !== undefined ? `${n('wind_pct')} %` : undefined], ['LOW CARBON', n('low_carbon_pct') !== undefined ? `${n('low_carbon_pct')} %` : undefined],
        ['CARBON', n('carbon_g_kwh') !== undefined ? `${n('carbon_g_kwh')} gCO₂/kWh (${a('carbon_index') ?? '—'})` : undefined],
        ['MIX (MW)', a('mix')?.replace(/,/g, ' · ').replace(/:/g, ' ')], ['READING', hm('frequency_at')]);
      return rows.filter((r): r is [string, string] => !!r[1]).map(([label, value]) => ({ label, value }));
    case 'utility-power-interconnector':
      rows.push(['TO / FROM', a('country')], ['FLOW', n('flow_mw') !== undefined ? `${Math.abs(n('flow_mw')!).toLocaleString()} MW ${a('direction') ?? ''}` : undefined],
        ['CAPACITY', unit('capacity_mw', 'MW')], ['USE', n('flow_pct') !== undefined ? `${n('flow_pct')} %` : undefined], ['HALF HOUR FROM', hm('flow_at')], ['ROUTE', 'approximate']);
      return rows.filter((r): r is [string, string] => !!r[1]).map(([label, value]) => ({ label, value }));
    case 'utility-water-overflow':
      rows.push(['COMPANY', a('operator')], ['STATUS', a('status') === 'discharging' ? 'Discharging now' : 'Stopped (last 48 h)'],
        ['SINCE', hm('since')], ['LAST EVENT', [hm('last_start'), hm('last_end')].filter(Boolean).join(' → ') || undefined],
        ['INTO', a('watercourse')], ['SITE', a('site') ?? a('ref')], ['UPDATED', hm('updated')]);
      return rows.filter((r): r is [string, string] => !!r[1]).map(([label, value]) => ({ label, value }));
    case 'utility-water-reservoir':
      rows.push(['LEVEL', unit('level_m', 'm', 2)], ['SPILLWAY', unit('spillway_m', 'm', 2)], ['READING', hm('reading_at')], ['SOURCE', 'Environment Agency']);
      return rows.filter((r): r is [string, string] => !!r[1]).map(([label, value]) => ({ label, value }));
    case 'utility-gas-terminal': case 'utility-gas-storage':
      rows.push(['FLOW', unit(e.kind === 'utility-gas-storage' ? 'withdrawal_mcmd' : 'flow_mcmd', 'mcm/d', 2)], ['ENTRY POINTS', a('entries')?.replace(/,/g, ' · ').replace(/:/g, ' ')],
        ['TYPE', a('site_type')], ['ESTIMATED', e.attributes.estimated === true ? 'yes' : undefined], ['AT', hm('flow_at')]);
      return rows.filter((r): r is [string, string] => !!r[1]).map(([label, value]) => ({ label, value }));
    case 'utility-gas-interconnector':
      rows.push(['TO / FROM', a('country')], ['FLOW', n('flow_mcmd') !== undefined ? `${n('flow_mcmd')} mcm/d ${a('direction') ?? ''}` : undefined], ['AT', hm('flow_at')], ['ROUTE', 'approximate']);
      return rows.filter((r): r is [string, string] => !!r[1]).map(([label, value]) => ({ label, value }));
    case 'utility-gas-system':
      rows.push(['SUPPLY', unit('supply_mcmd', 'mcm/d', 1)], ['DEMAND', unit('demand_mcmd', 'mcm/d', 1)], ['LINEPACK', unit('linepack_mcm', 'mcm', 1)],
        ['HOMES & BUSINESSES', unit('ldz_mcmd', 'mcm/d', 1)], ['POWER STATIONS', unit('power_mcmd', 'mcm/d', 1)], ['EXPORTS', unit('export_mcmd', 'mcm/d', 1)],
        ['INTO STORAGE', unit('storage_injection_mcmd', 'mcm/d', 1)], ['GAS DAY', a('gas_day')], ['PUBLISHED', hm('published')]);
      return rows.filter((r): r is [string, string] => !!r[1]).map(([label, value]) => ({ label, value }));
    case 'utility-telecom-outage':
      rows.push(['COUNTRY', a('country')], ['SIGNALS', a('signals')], ['SCORE', a('score')], ['EVENTS (6 H)', a('events')], ['SOURCE', 'IODA, Georgia Tech']);
      return rows.filter((r): r is [string, string] => !!r[1]).map(([label, value]) => ({ label, value }));
    default:
      if (e.kind.startsWith('infra-')) {
        rows.push(['OPERATOR', a('operator')], ['VOLTAGE', unit('voltage_kv', 'kV')], ['FUEL', a('fuel')], ['OUTPUT', unit('output_mw', 'MW', 1)],
          ['LINES', a('lines_n')], ['SOURCE', '© OpenStreetMap contributors']);
        return rows.filter((r): r is [string, string] => !!r[1]).map(([label, value]) => ({ label, value }));
      }
  }
  switch (e.kind) {
    case 'air-balloon':
      rows.push(['TYPE', a('type')], ['ASCENT', num(e.attributes.vel_v) !== undefined ? `${num(e.attributes.vel_v)!.toFixed(1)} m/s` : undefined],
        ['DRIFT', num(e.attributes.vel_h) !== undefined ? `${num(e.attributes.vel_h)!.toFixed(1)} m/s @ ${Math.round(num(e.attributes.heading) ?? 0)}°` : undefined]);
      break;
    case 'space-satellite':
      rows.push(['NORAD ID', a('norad_id')], ['GROUP', a('group')], ['ALTITUDE', num(e.attributes.alt_km) !== undefined ? `${Math.round(num(e.attributes.alt_km)!).toLocaleString()} km` : undefined],
        ['SPEED', num(e.attributes.speed_kms) !== undefined ? `${num(e.attributes.speed_kms)!.toFixed(2)} km/s` : undefined]);
      break;
    case 'event-earthquake': {
      const t = num(e.attributes.event_time);
      rows.push(['MAGNITUDE', num(e.attributes.mag) !== undefined ? `M${num(e.attributes.mag)!.toFixed(1)}` : undefined], ['DEPTH', num(e.attributes.depth_km) !== undefined ? `${num(e.attributes.depth_km)} km` : undefined],
        ['PLACE', a('place')], ['TIME', t !== undefined ? new Date(t * 1000).toISOString().slice(0, 19).replace('T', ' ') + ' UTC' : undefined]);
      break;
    }
    case 'land-airport': {
      const t = num(e.attributes.metar_time);
      const wind = num(e.attributes.wind_kt);
      rows.push(['ICAO / IATA', [a('icao'), a('iata')].filter(Boolean).join(' / ')], ['NAME', a('name')], ['TYPE', a('airport_type')],
        ['ELEVATION', a('elevation_ft') ? `${a('elevation_ft')} ft` : undefined], ['RUNWAYS', a('runway_names')],
        ['LONGEST', a('longest_runway_ft') ? `${Number(a('longest_runway_ft')).toLocaleString()} ft` : undefined],
        ['CONDITIONS', a('flight_category')],
        ['WIND', wind === undefined ? undefined : `${e.attributes.wind_variable === true ? 'VRB' : String(Math.round(num(e.attributes.wind_dir) ?? 0)).padStart(3, '0') + '°'} ${wind} kt${num(e.attributes.gust_kt) ? ` gust ${num(e.attributes.gust_kt)}` : ''}`],
        ['VISIBILITY', a('visibility') ? `${a('visibility')} mi` : undefined], ['CLOUD', a('clouds')],
        ['TEMP / DEW', num(e.attributes.temp_c) !== undefined ? `${num(e.attributes.temp_c)} / ${num(e.attributes.dewpoint_c) ?? '—'} °C` : undefined],
        ['QNH', a('qnh_hpa') ? `${a('qnh_hpa')} hPa` : undefined],
        ['METAR', a('metar')], ['OBSERVED', t !== undefined ? new Date(t * 1000).toISOString().slice(11, 16) + ' UTC' : undefined]);
      // The TAF itself is shown below the facts, with its worst category. [FR-H-1]
      break;
    }
    case 'air-track':
      // Where the position came from: ADS-B from the aircraft, or multilateration. [FR-H-1]
      rows.push(['POSITION BY', a('pos_source') ? `${a('pos_source')!.replace('adsb_icao', 'ADS-B').replace('mlat', 'MLAT')}${a('pos_feed') ? ` · ${a('pos_feed')}` : ''}` : undefined],
        ['AIRCRAFT TYPE', a('aircraft_type')], ['REGISTRATION', a('registration')],
        // GPS integrity: NIC 0 or NACp 0, or a lost GPS fix, usually means jamming or spoofing.
        ['GPS', e.attributes.gps_lost === true ? 'LOST (jamming or spoofing likely)'
          : num(e.attributes.nic) === 0 || num(e.attributes.nac_p) === 0 ? `DEGRADED (NIC ${a('nic') ?? '?'}, NACp ${a('nac_p') ?? '?'})`
          : num(e.attributes.nic) !== undefined ? `OK (NIC ${a('nic')}, NACp ${a('nac_p') ?? '?'})` : undefined]);
      break;
    case 'event-gnss-interference': {
      const share = num(e.attributes.share);
      rows.push(['LEVEL', a('level')?.toUpperCase()], ['AIRCRAFT AFFECTED', `${a('degraded') ?? '?'} of ${a('aircraft') ?? '?'}${share !== undefined ? ` (${Math.round(share * 100)} %)` : ''}`],
        ['METHOD', a('method')]);
      break;
    }
    case 'event-sigmet': {
      const b = num(e.attributes.base_ft), top = num(e.attributes.top_ft);
      rows.push(['HAZARD', [a('qualifier'), a('hazard')].filter(Boolean).join(' ') || undefined],
        ['LEVELS', b !== undefined || top !== undefined ? `${b !== undefined ? `${b.toLocaleString()} ft` : 'SFC'} – ${top !== undefined ? `${top.toLocaleString()} ft` : 'not given'}` : undefined],
        ['FIR', a('fir')], ['VALID', [when(e.attributes.valid_from), when(e.attributes.valid_to)].filter(Boolean).join(' – ') || undefined], ['RAW', a('raw')]);
      break;
    }
    case 'airspace-volume':
      rows.push(['NAME', a('name')], ['TYPE', a('airspace_type')?.toUpperCase()], ['CLASS', a('icao_class')],
        ['LOWER', limitText(num(e.attributes.lower_ft), a('lower_ref')) + (a('lower_ref') ? ` (${a('lower_ref')})` : '')],
        ['UPPER', limitText(num(e.attributes.upper_ft), a('upper_ref')) + (a('upper_ref') ? ` (${a('upper_ref')})` : '')],
        ['COUNTRY', a('country')]);
      break;
    case 'event-fire': {
      const t = num(e.attributes.acq_time);
      const conf = a('confidence');
      rows.push(['FRP', num(e.attributes.frp) !== undefined ? `${num(e.attributes.frp)!.toFixed(1)} MW` : undefined],
        ['CONFIDENCE', conf ? ({ l: 'low', n: 'nominal', h: 'high' } as Record<string, string>)[conf.toLowerCase()] ?? conf : undefined],
        ['SATELLITE', [a('satellite'), a('instrument')].filter(Boolean).join(' · ') || undefined],
        ['ACQUIRED', t !== undefined ? new Date(t * 1000).toISOString().slice(0, 16).replace('T', ' ') + ' UTC' : undefined],
        ['DAY / NIGHT', a('daynight') ? (a('daynight')!.toUpperCase() === 'D' ? 'day' : 'night') : undefined],
        ['BRIGHTNESS', num(e.attributes.brightness_k) !== undefined ? `${num(e.attributes.brightness_k)!.toFixed(0)} K` : undefined]);
      break;
    }
    case 'event-weather-warning':
      rows.push(['LEVEL', a('level')?.toUpperCase()], ['WEATHER', a('weather')], ['REGION', a('region')], ['TITLE', a('title')],
        ['VALID', [when(e.attributes.valid_from), when(e.attributes.valid_to)].filter(Boolean).join(' – ') || undefined], ['LINK', a('link')]);
      break;
    case 'sea-windfarm':
      rows.push(['NAME', a('name')], ['STATUS', a('status')], ['CAPACITY', num(e.attributes.capacity_mw) !== undefined ? `${num(e.attributes.capacity_mw)!.toLocaleString()} MW` : a('capacity_mw')], ['DEVELOPER', a('developer')]);
      break;
    case 'sea-platform':
      rows.push(['NAME', a('name')], ['OPERATOR', a('operator')], ['TYPE', a('type')], ['FIELD', a('field')], ['STATUS', a('status')]);
      break;
    case 'land-road-disruption':
      rows.push(['SEVERITY', a('severity')], ['CATEGORY', a('category')], ['LOCATION', a('location')], ['COMMENTS', a('comments')],
        ['FROM', when(e.attributes.start)], ['UNTIL', when(e.attributes.end)]);
      break;
    case 'land-camera':
      rows.push(['VIEW', a('view')], ['TYPE', a('camera_type')], ['OPERATOR', a('operator')]);
      break;
    case 'land-train': {
      const late = num(e.attributes.late_min) ?? 0;
      rows.push(['HEADCODE', a('headcode')], ['AT', [a('station'), a('crs') && `(${a('crs')})`].filter(Boolean).join(' ')],
        ['EVENT', a('event')], ['PLATFORM', a('platform')],
        ['PUNCTUALITY', late > 0 ? `${late} min late` : late < 0 ? `${-late} min early` : 'on time'],
        ['NEXT REPORT', a('next_station')], ['OPERATOR CODE', a('toc_id')], ['TRAIN ID', a('train_id')]);
      break;
    }
    case 'sea-vessel':
      rows.push(['NAME', a('name')], ['MMSI', a('mmsi')], ['IMO', a('imo')], ['TYPE', a('ship_type')], ['STATUS', a('nav_status')],
        ['DESTINATION', a('destination')], ['HEADING', num(e.attributes.heading) !== undefined ? `${Math.round(num(e.attributes.heading)!)}°` : undefined]);
      break;
    case 'event-flood':
      rows.push(['SEVERITY', [a('severity_level'), a('severity')].filter(Boolean).join(' · ') || undefined], ['RIVER', a('river')], ['AREA', a('area')], ['RAISED', a('time_raised')]);
      break;
  }
  return rows.filter((r): r is [string, string] => r[1] !== undefined).map(([label, value]) => ({ label, value }));
}

/** Times in unix seconds (a gateway sending milliseconds is read either way). */
function normaliseAlert(a: HeimdallAlert): HeimdallAlert {
  const secs = (v: number | null | undefined): number | null => (typeof v === 'number' && Number.isFinite(v) ? (v > 1e12 ? Math.round(v / 1000) : v) : null);
  // Camera analytics alerts are raised on the analysis entity (AI-<camera>), which is not
  // drawn: point them at the camera itself, so picking one selects it. [FR-P3-5]
  const onCamera = a.kind === 'land-camera-ai' && typeof a.entity === 'string' && a.entity.startsWith('AI-');
  return {
    ...a,
    ...(onCamera ? { entity: a.entity.slice(3), kind: 'land-camera' } : {}),
    raisedAt: secs(a.raisedAt) ?? 0,
    clearedAt: secs(a.clearedAt),
    acknowledgedAt: secs(a.acknowledgedAt),
    acknowledgedBy: a.acknowledgedBy ?? null,
    lat: typeof a.lat === 'number' ? a.lat : null,
    lon: typeof a.lon === 'number' ? a.lon : null,
  };
}

/** The SWPC entity's attributes as the picture's space weather. [FR-H-1] */
function spaceWeatherOf(entities: WireEntity[]): HeimdallSpaceWeather | undefined {
  const e = entities.find((x) => x.kind === 'space-weather');
  if (!e) return undefined;
  // Scales arrive as 0–5, or as text such as "G1".
  const scale = (k: string) => {
    const v = e.attributes[k];
    const n = typeof v === 'number' ? v : Number(String(v ?? '').replace(/\D/g, ''));
    return Number.isFinite(n) ? Math.max(0, Math.min(5, Math.round(n))) : 0;
  };
  const kpTime = e.attributes.kp_time;
  return {
    kp: num(e.attributes.kp) ?? null,
    kpTime: typeof kpTime === 'number' ? kpTime : typeof kpTime === 'string' && Number.isFinite(Date.parse(kpTime)) ? Date.parse(kpTime) / 1000 : undefined,
    gScale: scale('g_scale'),
    sScale: scale('s_scale'),
    rScale: scale('r_scale'),
    xrayClass: textAttr(e, 'xray_class'),
  };
}

export class HeimdallHttp implements HeimdallGateway {
  private readonly base: string;
  private readonly token: string;
  private readonly region: HeimdallRegion;
  private readonly onUnauthorized?: () => void;
  private readonly fetchEvery: number;
  private readonly extrapolateMax: number;
  private cached: { at: number; picture: Promise<WirePicture> } | null = null;
  /** Reported positions per entity, oldest first, collected across fetches: the read
   *  carries only the latest position, so the trail is what this console has seen. */
  private readonly history = new Map<string, HeimdallHistoryPoint[]>();
  private fetchedAt = 0;
  /** Screens without a service behind them yet. */
  private readonly simulated = new HeimdallMock();

  constructor(opts: HeimdallHttpOptions) {
    this.base = opts.baseUrl.replace(/\/$/, '');
    this.token = opts.token;
    this.region = opts.region ?? UK_AND_IRELAND;
    this.onUnauthorized = opts.onUnauthorized;
    this.fetchEvery = (opts.fetchEverySecs ?? 15) * 1000;
    this.extrapolateMax = opts.extrapolateMaxSecs ?? 300;
  }

  policy(): PolicyContext {
    return this.simulated.policy();
  }

  /** The picture, fetched again only when the cached copy is older than `fetchEvery`. */
  private picture(): Promise<WirePicture> {
    const now = Date.now();
    if (!this.cached || now - this.cached.at >= this.fetchEvery) {
      const picture = this.fetchPicture();
      this.cached = { at: now, picture };
      picture.catch(() => {
        if (this.cached?.picture === picture) this.cached = null;
      });
    }
    return this.cached.picture;
  }

  /** Camera analytics by camera entity id, from the last picture. */
  private cameraAI = new Map<string, HeimdallCameraAnalysis>();

  /** Boxes the reads are limited to (setRegions), or null for everywhere. */
  private regions: [number, number, number, number][] | null = null;

  setRegions(boxes: [number, number, number, number][] | null): void {
    const key = (b: typeof boxes) => JSON.stringify(b);
    if (key(boxes) === key(this.regions)) return;
    this.regions = boxes;
    // Read again now, for the new area.
    this.cached = null;
    this.staticLayers = null;
  }

  private regionParams(): string[] {
    return (this.regions ?? []).map((b) => `bbox=${b.map((v) => +v.toFixed(3)).join(',')}`);
  }

  /** One read of the picture, optionally limited to some layers and the regions. */
  private async readPicture(layers?: string[]): Promise<WirePicture> {
    const parts = [...(layers?.length ? [`layers=${layers.join(',')}`] : []), ...this.regionParams()];
    const q = parts.length ? `?${parts.join('&')}` : '';
    const res = await fetch(`${this.base}/heimdall/picture${q}`, {
      headers: { authorization: `Bearer ${this.token}` },
    });
    if (res.status === 401) {
      this.onUnauthorized?.();
      throw new Error('session expired: sign in again');
    }
    const body = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(body.error ?? `HTTP ${res.status}`);
    return body as WirePicture;
  }

  /** The static layers (airspace: thousands of polygons that change daily), read every
   *  STATIC_EVERY_MS and kept between reads, so the frequent read stays small. */
  private staticLayers: { at: number; picture: Promise<WirePicture> } | null = null;

  private async fetchPicture(): Promise<WirePicture> {
    const now = Date.now();
    if (!this.staticLayers || now - this.staticLayers.at >= STATIC_EVERY_MS) {
      const read = this.readPicture(STATIC_LAYERS);
      this.staticLayers = { at: now, picture: read };
      read.catch(() => { if (this.staticLayers?.picture === read) this.staticLayers = null; });
    }
    // Every other layer: whatever Heimdall serves except the static ones. The layer list
    // comes from the static read (Heimdall lists every layer on every page).
    const stat = await this.staticLayers.picture.catch(() => ({ layers: [], entities: [] }) as WirePicture);
    const moving = stat.layers.length
      ? stat.layers.map((l) => l.id).filter((id) => !STATIC_LAYERS.includes(id))
      : undefined;
    const live = await this.readPicture(moving);
    const picture: WirePicture = { layers: live.layers.length ? live.layers : stat.layers, entities: [...live.entities, ...stat.entities] };
    this.fetchedAt = Date.now();
    this.remember(picture);
    return picture;
  }

  private remember(p: WirePicture): void {
    const live = new Set<string>();
    for (const e of p.entities) {
      const lat = num(e.attributes.lat);
      const lon = num(e.attributes.lon);
      if (lat === undefined || lon === undefined) continue;
      live.add(e.id);
      const h = this.history.get(e.id) ?? [];
      if (!h.length || h[h.length - 1][0] !== e.lastSeen) h.push([e.lastSeen, lat, lon, altMetres(e) ?? null]);
      if (h.length > TRAIL_POINTS) h.splice(0, h.length - TRAIL_POINTS);
      this.history.set(e.id, h);
    }
    for (const id of this.history.keys()) if (!live.has(id)) this.history.delete(id);
  }

  private entity(e: WireEntity): HeimdallEntity {
    const alert = ALERT_LABELS.has(e.classification?.label ?? '');
    return {
      id: e.id,
      cls: classOf(e),
      conf: e.classification ? 100 : 50,
      feeds: e.sources.length,
      track: alert ? 'anomalous' : 'stable',
      firstSeen: hhmm(e.firstSeen),
      marking: 'internal',
      compartment: null,
      status: alert ? 'danger' : 'online',
      statusLabel: alert ? (e.classification?.label ?? 'Alert') : `Seen ${ago(e.lastSeen)} ago`,
    };
  }

  /** Where the track is estimated to be now: its last position moved along its heading
   *  at its ground speed (great-circle), for at most `extrapolateMax` seconds. */
  private estimate(e: WireEntity, lat: number, lon: number): [number, number] {
    const speed = num(e.attributes.speed) ?? 0;
    if (e.kind !== 'air-track' || e.attributes.on_ground === true || speed <= 0) return [lat, lon];
    const dt = Math.min(this.extrapolateMax, Math.max(0, Date.now() / 1000 - e.lastSeen));
    const d = (speed * dt) / 3600 / 3440.065; // nautical miles over the earth's radius
    const brg = ((num(e.attributes.track) ?? 0) * Math.PI) / 180;
    const p1 = (lat * Math.PI) / 180;
    const l1 = (lon * Math.PI) / 180;
    const p2 = Math.asin(Math.sin(p1) * Math.cos(d) + Math.cos(p1) * Math.sin(d) * Math.cos(brg));
    const l2 = l1 + Math.atan2(Math.sin(brg) * Math.sin(d) * Math.cos(p1), Math.cos(d) - Math.sin(p1) * Math.sin(p2));
    return [(p2 * 180) / Math.PI, (l2 * 180) / Math.PI];
  }

  private track(e: WireEntity): HeimdallTrack | null {
    // Space weather is global, not a place: it goes to picture.spaceWeather instead.
    if (e.kind === 'space-weather') return null;
    const area = parsePolygon(e.attributes.polygon) ?? (e.kind === 'land-road-disruption' ? lineBand(e.attributes.line) : undefined);
    let reportedLat = num(e.attributes.lat);
    let reportedLon = num(e.attributes.lon);
    if ((reportedLat === undefined || reportedLon === undefined) && area) {
      reportedLat = area.reduce((s, p) => s + p[0], 0) / area.length;
      reportedLon = area.reduce((s, p) => s + p[1], 0) / area.length;
    }
    if (reportedLat === undefined || reportedLon === undefined) return null;
    const [lat, lon] = this.estimate(e, reportedLat, reportedLon);
    const [x, y] = this.project(lat, lon);
    const text = (k: string) => (typeof e.attributes[k] === 'string' ? (e.attributes[k] as string) : undefined);
    const moved = lat !== reportedLat || lon !== reportedLon;
    const layer = layerOf(e.layer);
    const callsign = typeof e.attributes.callsign === 'string' ? e.attributes.callsign.trim() : '';
    const altM = altMetres(e);
    const air = e.kind === 'air-track' || e.kind === 'air-balloon';
    // Ground speed in knots: aircraft report knots, balloons m/s, satellites km/s.
    const spd = num(e.attributes.speed) ?? (num(e.attributes.vel_h) ?? 0) * KT_PER_MS + (num(e.attributes.speed_kms) ?? 0) * 1000 * KT_PER_MS;
    const trail = this.history.get(e.id) ?? [];
    return {
      id: callsign || e.id,
      cls: classOf(e),
      conf: e.classification ? 1 : 0.5,
      x,
      y,
      lat,
      lon,
      layer,
      track: ALERT_LABELS.has(e.classification?.label ?? '') ? 'anomalous' : 'stable',
      hdg: num(e.attributes.track) ?? num(e.attributes.heading) ?? 0,
      spd: Math.round(spd),
      alt: altitude(e),
      feeds: e.sources.length,
      altFt: num(e.attributes.alt_ft) ?? (air && altM !== undefined ? Math.round(altM * FT_PER_M) : undefined),
      trail: trail.map(([, la, lo]) => {
        const [tx, ty] = this.project(la, lo);
        return { x: tx, y: ty };
      }),
      geoTrail: trail.map((p) => [...p] as HeimdallHistoryPoint),
      detail: {
        key: e.id,
        kind: e.kind,
        name: text('name') ?? text('place') ?? text('area') ?? text('title') ?? text('location') ?? (e.kind === 'event-sigmet' ? [text('fir'), 'SIGMET', text('hazard')].filter(Boolean).join(' ') : undefined),
        callsign: text('callsign'),
        squawk: text('squawk') ?? (num(e.attributes.squawk) !== undefined ? String(e.attributes.squawk) : undefined),
        category: text('category'),
        onGround: e.attributes.on_ground === true,
        lat: reportedLat,
        lon: reportedLon,
        altM,
        magnitude: num(e.attributes.mag),
        lateMin: e.kind === 'land-train' ? num(e.attributes.late_min) : undefined,
        camera: e.kind === 'land-camera' ? {
          id: e.id, imageUrl: textAttr(e, 'image_url'), videoUrl: textAttr(e, 'video_url'),
          streamUrl: textAttr(e, 'stream_url'), streamType: textAttr(e, 'stream_type'),
          analysis: this.cameraAI.get(e.id) ?? cameraAnalysis(e),
        } : undefined,
        severityLevel: num(e.attributes.severity_level),
        station: isStation(e) || undefined,
        shipType: e.kind === 'sea-vessel' ? textAttr(e, 'ship_type') : undefined,
        flightCategory: e.kind === 'land-airport' ? textAttr(e, 'flight_category') : undefined,
        runways: e.kind === 'land-airport' ? runwaysOf(textAttr(e, 'runways_geo')) : undefined,
        facts: factsOf(e),
        area,
        hazard: e.kind === 'event-sigmet' ? text('hazard') : undefined,
        baseFt: e.kind === 'event-sigmet' ? num(e.attributes.base_ft) : e.kind === 'airspace-volume' ? num(e.attributes.lower_ft) : undefined,
        topFt: e.kind === 'event-sigmet' ? num(e.attributes.top_ft) : e.kind === 'airspace-volume' ? num(e.attributes.upper_ft) : undefined,
        airspaceType: e.kind === 'airspace-volume' ? text('airspace_type')?.toLowerCase() : undefined,
        icaoClass: e.kind === 'airspace-volume' ? text('icao_class') : undefined,
        lowerRef: e.kind === 'airspace-volume' ? text('lower_ref') : undefined,
        upperRef: e.kind === 'airspace-volume' ? text('upper_ref') : undefined,
        frp: e.kind === 'event-fire' ? num(e.attributes.frp) : undefined,
        validFrom: e.kind === 'event-weather-warning' || e.kind === 'event-sigmet' ? num(e.attributes.valid_from) : undefined,
        validTo: e.kind === 'event-weather-warning' || e.kind === 'event-sigmet' ? num(e.attributes.valid_to) : undefined,
        weather: e.kind === 'event-weather-warning' ? text('weather') : undefined,
        lines: e.kind.startsWith('infra-') || e.kind.endsWith('-interconnector') ? parseLines(e.attributes.line) : undefined,
        network: e.kind.startsWith('infra-') || e.kind.startsWith('utility-') ? text('network') : undefined,
        planned: e.kind === 'utility-power-outage' ? e.attributes.planned === true : undefined,
        status: e.kind.startsWith('utility-') ? text('status') : undefined,
        flowing: e.kind.startsWith('utility-gas') ? e.attributes.flowing === true : undefined,
        direction: e.kind.endsWith('-interconnector') ? text('direction') : undefined,
        fuel: e.kind === 'infra-power-plant' ? text('fuel') : undefined,
        level: e.kind === 'event-weather-warning' ? text('level')?.toLowerCase() : e.kind === 'land-road-disruption' ? text('severity') : e.kind === 'event-gnss-interference' ? text('level') : undefined,
        taf: e.kind === 'land-airport' ? text('taf') : undefined,
        tafWorst: e.kind === 'land-airport' ? text('taf_worst_category_12h') : undefined,
        label: e.classification?.label,
        rationale: e.classification?.rationale,
        sources: e.sources,
        observations: e.observations,
        firstSeen: e.firstSeen,
        lastSeen: e.lastSeen,
        estimatedSecs: moved ? Math.round(Math.min(this.extrapolateMax, Date.now() / 1000 - e.lastSeen)) : 0,
      },
    };
  }

  private project(lat: number, lon: number): [number, number] {
    const { west, east, south, north } = this.region;
    return [((lon - west) / (east - west)) * 100, ((north - lat) / (north - south)) * 100];
  }

  async getPicture(): Promise<HeimdallPicture> {
    const p = await this.picture();
    const layers: HeimdallLayer[] = [];
    for (const l of p.layers) {
      const id = layerOf(l.id);
      if (!layers.some((x) => x.id === id)) layers.push({ id, label: l.name, active: l.visible });
    }
    // Track ids label the canvas: a callsign where it is unique and real, else the
    // ICAO address (some transponders send the placeholder callsign 00000000).
    const used = new Set<string>();
    const geo: HeimdallTrack[] = [];
    // Camera analytics arrive as their own entities (kind land-camera-ai, camera_id naming
    // the camera); they are joined onto their camera here and not drawn themselves.
    this.cameraAI = new Map();
    for (const e of p.entities) {
      if (e.kind !== 'land-camera-ai') continue;
      const cam = textAttr(e, 'camera_id');
      const ai = cameraAnalysis(e);
      if (cam && ai) this.cameraAI.set(cam, ai);
    }
    for (const e of p.entities) {
      if (e.kind === 'land-camera-ai') continue;
      const t = this.track(e);
      if (!t) continue;
      if (used.has(t.id) || /^0+$/.test(t.id)) t.id = e.id;
      used.add(t.id);
      geo.push(t);
    }
    // The 2D canvas spans the region; the globe takes them all.
    const tracks = geo.filter((t) => t.x >= 0 && t.x <= 100 && t.y >= 0 && t.y <= 100);
    return {
      layers,
      tracks,
      geo,
      entities: p.entities.map((e) => this.entity(e)),
      region: this.region,
      fetchedAt: this.fetchedAt,
      spaceWeather: spaceWeatherOf(p.entities),
    };
  }

  /** Alerts from `GET /heimdall/alerts?state=…`, newest first. [FR-H-1] */
  async getAlerts(state: 'open' | 'all' = 'open'): Promise<HeimdallAlert[]> {
    const body = await this.alertsCall(`/heimdall/alerts?state=${state}`, 'GET');
    const list = Array.isArray(body?.alerts) ? (body.alerts as HeimdallAlert[]) : null;
    if (!list) throw new HeimdallAlertsUnavailableError('alerts unavailable: unexpected reply');
    return list.map(normaliseAlert).sort((a, b) => b.raisedAt - a.raisedAt);
  }

  /** Acknowledge an alert as the signed-in user (the gateway audits it). [FR-H-5] */
  async ackAlert(id: string): Promise<HeimdallAlert> {
    const body = await this.alertsCall(`/heimdall/alerts/${encodeURIComponent(id)}/ack`, 'POST');
    const alert = (body?.alert ?? body) as HeimdallAlert | null;
    if (!alert || typeof alert.id !== 'string') throw new Error('acknowledge failed: unexpected reply');
    return normaliseAlert(alert);
  }

  /** Analyse one camera now via `POST /heimdall/cameras/{id}/analyse` (audited as an
   *  action). Nothing is analysed unless asked. [FR-P3-5] */
  async analyseCamera(id: string): Promise<HeimdallCameraAnalysis> {
    let res: Response;
    try {
      res = await fetch(`${this.base}/heimdall/cameras/${encodeURIComponent(id)}/analyse`, {
        method: 'POST', headers: { authorization: `Bearer ${this.token}` },
      });
    } catch (err) {
      throw new Error(`camera analytics unavailable: ${err instanceof Error ? err.message : String(err)}`);
    }
    if (res.status === 401) {
      this.onUnauthorized?.();
      throw new Error('session expired: sign in again');
    }
    const body = await res.json().catch(() => null);
    if (!res.ok) throw new Error(body?.error ?? `camera analytics failed (HTTP ${res.status})`);
    const ai = body?.attrs && typeof body.attrs === 'object'
      ? cameraAnalysis({ attributes: body.attrs } as WireEntity)
      : undefined;
    if (!ai) throw new Error('camera analytics sent no result');
    ai.playback = playbackOf(body.playback);
    this.cameraAI.set(id, ai);
    return ai;
  }

  /** `GET /heimdall/cameras/{id}/trend`. [FR-P3-5] */
  async cameraTrend(id: string): Promise<HeimdallCameraTrend> {
    const body = await this.camerasCall(`/heimdall/cameras/${encodeURIComponent(id)}/trend`, 'GET');
    const n = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : undefined);
    const points = (Array.isArray(body?.points) ? body.points : [])
      .filter((p: any) => p && typeof p.at === 'number')
      .map((p: any) => ({
        at: p.at, vehicles: n(p.vehicles), people: n(p.people), moving: n(p.moving), stationary: n(p.stationary),
        stopped: n(p.stopped), queue: p.queue === true, blocking: n(p.blocking), wrongWay: n(p.wrong_way), personRoad: n(p.person_road),
      }))
      .sort((a: { at: number }, b: { at: number }) => a.at - b.at);
    return { points, watch: watchOf(body?.watch) };
  }

  /** `POST /heimdall/cameras/{id}/watch?hours=N` (audited as an action). [FR-P3-5] */
  async watchCamera(id: string, hours: number): Promise<HeimdallCameraTrend['watch']> {
    const body = await this.camerasCall(`/heimdall/cameras/${encodeURIComponent(id)}/watch?hours=${Math.round(hours)}`, 'POST');
    return watchOf(body?.watch);
  }

  private async camerasCall(path: string, method: 'GET' | 'POST'): Promise<any> {
    let res: Response;
    try {
      res = await fetch(`${this.base}${path}`, { method, headers: { authorization: `Bearer ${this.token}` } });
    } catch (err) {
      throw new Error(`camera analytics unavailable: ${err instanceof Error ? err.message : String(err)}`);
    }
    if (res.status === 401) {
      this.onUnauthorized?.();
      throw new Error('session expired: sign in again');
    }
    const body = await res.json().catch(() => null);
    if (!res.ok) throw new Error(body?.error ?? `camera analytics failed (HTTP ${res.status})`);
    return body;
  }

  private async alertsCall(path: string, method: 'GET' | 'POST'): Promise<any> {
    let res: Response;
    try {
      res = await fetch(`${this.base}${path}`, { method, headers: { authorization: `Bearer ${this.token}` } });
    } catch (err) {
      throw new HeimdallAlertsUnavailableError(`alerts unavailable: ${err instanceof Error ? err.message : String(err)}`);
    }
    if (res.status === 401) {
      this.onUnauthorized?.();
      throw new Error('session expired: sign in again');
    }
    if (res.status === 404 || res.status === 501 || res.status === 502 || res.status === 503) {
      throw new HeimdallAlertsUnavailableError(`alerts unavailable (HTTP ${res.status})`);
    }
    const body = await res.json().catch(() => null);
    if (!res.ok) throw new Error(body?.error ?? `HTTP ${res.status}`);
    return body;
  }

  async listEntities(): Promise<HeimdallEntity[]> {
    return (await this.picture()).entities.map((e) => this.entity(e));
  }

  /** One row per feed that has contributed to the picture: every observation was
   *  admitted only after its ML-DSA source signature verified. */
  async listFeeds(): Promise<HeimdallFeed[]> {
    const p = await this.picture();
    const feeds = new Map<string, { tracks: number; newest: number; kind: string }>();
    for (const e of p.entities) {
      for (const s of e.sources) {
        const f = feeds.get(s) ?? { tracks: 0, newest: 0, kind: KIND_FEED[e.kind] ?? e.kind };
        f.tracks += 1;
        f.newest = Math.max(f.newest, e.lastSeen);
        feeds.set(s, f);
      }
    }
    return [...feeds.entries()].map(([name, f]) => {
      const stale = Date.now() / 1000 - f.newest > 300;
      return {
        name,
        kind: f.kind,
        rate: `${f.tracks} tracks`,
        latency: ago(f.newest),
        weight: 'medium',
        auth: true,
        scheme: 'ML-DSA',
        status: stale ? 'warn' : 'online',
        statusLabel: stale ? `Stale ${ago(f.newest)}` : 'Live',
      } satisfies HeimdallFeed;
    });
  }

  /** Recorded positions from `GET /heimdall/history`. A gateway without the endpoint
   *  (404) or without a history store behind it (502, 503) rejects with
   *  `HeimdallHistoryUnavailableError`, which the console shows as "history
   *  unavailable" while keeping its own trails. [FR-P3-7] */
  async getHistory(q: HeimdallHistoryQuery): Promise<HeimdallHistory> {
    const params = new URLSearchParams({ from: String(Math.floor(q.from)), to: String(Math.ceil(q.to)) });
    if (q.layer) params.set('layer', wireLayer(q.layer));
    if (q.entity) params.set('entity', q.entity);
    if (q.step) params.set('step', String(Math.round(q.step)));
    let res: Response;
    try {
      res = await fetch(`${this.base}/heimdall/history?${[params.toString(), ...this.regionParams()].join('&')}`, {
        headers: { authorization: `Bearer ${this.token}` },
      });
    } catch (err) {
      throw new HeimdallHistoryUnavailableError(`history unavailable: ${err instanceof Error ? err.message : String(err)}`);
    }
    if (res.status === 401) {
      this.onUnauthorized?.();
      throw new Error('session expired: sign in again');
    }
    if (res.status === 404 || res.status === 501 || res.status === 502 || res.status === 503) {
      throw new HeimdallHistoryUnavailableError(`history unavailable (HTTP ${res.status})`);
    }
    const body = await res.json().catch(() => null);
    if (!res.ok || !body || !Array.isArray(body.tracks)) {
      throw new HeimdallHistoryUnavailableError(`history unavailable: ${body?.error ?? `HTTP ${res.status}`}`);
    }
    const wire = body as { from: number; to: number; truncated?: boolean; tracks: { id: string; kind: string; layer: string; points: HeimdallHistoryPoint[] }[] };
    return {
      from: wire.from,
      to: wire.to,
      truncated: !!wire.truncated,
      tracks: wire.tracks.map((t) => ({ id: t.id, kind: t.kind, layer: layerOf(t.layer), points: t.points })),
    };
  }

  // ---- simulated until Heimdall serves them ---------------------------------
  listSyncSites(): Promise<HeimdallSyncSite[]> {
    return this.simulated.listSyncSites();
  }
  listWorkflows(): Promise<HeimdallWorkflow[]> {
    return this.simulated.listWorkflows();
  }
  authoriseWorkflow(id: string, humanGateToken?: string): Promise<{ auditId: string; applied: boolean }> {
    return this.simulated.authoriseWorkflow(id, humanGateToken);
  }
  listDecisions(): Promise<HeimdallDecision[]> {
    return this.simulated.listDecisions();
  }
  getAudit(limit?: number): Promise<AuditRecord[]> {
    return this.simulated.getAudit(limit);
  }
  auditStream(onRecord: (record: AuditRecord) => void): () => void {
    return this.simulated.auditStream(onRecord);
  }
}
