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
 * console falls back to the trails collected here. [FR-P3-3] [FR-P3-7] */
import type { AuditRecord, PolicyContext } from '../types';
import {
  HeimdallHistoryUnavailableError,
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
  air: 'air', sea: 'maritime', land: 'ground', space: 'space', events: 'events', other: 'other',
};
const layerOf = (wire: string): HeimdallLayerId => WIRE_LAYERS[wire] ?? 'other';
const wireLayer = (id: HeimdallLayerId): string =>
  Object.keys(WIRE_LAYERS).find((k) => WIRE_LAYERS[k] === id) ?? id;
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
};
/** What kind of feed contributes each entity kind (the Feeds screen). */
const KIND_FEED: Record<string, string> = {
  'air-track': 'transponder (ADS-B)',
  'air-balloon': 'radiosonde telemetry',
  'space-satellite': 'orbital elements',
  'event-earthquake': 'seismic catalogue',
  'event-flood': 'flood warnings',
  'sea-vessel': 'ship transponders (AIS)',
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
  if (e.kind === 'event-flood') return 'SURFACE';
  const ft = num(e.attributes.alt_ft) ?? (num(e.attributes.alt_m) !== undefined ? Math.round(num(e.attributes.alt_m)! * FT_PER_M) : undefined);
  if (ft === undefined) return '—';
  return ft >= 6000 ? `FL${Math.round(ft / 100).toString().padStart(3, '0')}` : `${Math.round(ft).toLocaleString()} ft`;
}

function classOf(e: WireEntity): string {
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

/** The kind-specific attributes the inspector lists, in display order. */
function factsOf(e: WireEntity): { label: string; value: string }[] {
  const rows: [string, string | undefined][] = [];
  const a = (k: string) => textAttr(e, k);
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

  private async fetchPicture(): Promise<WirePicture> {
    const res = await fetch(`${this.base}/heimdall/picture`, {
      headers: { authorization: `Bearer ${this.token}` },
    });
    if (res.status === 401) {
      this.onUnauthorized?.();
      throw new Error('session expired: sign in again');
    }
    const body = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(body.error ?? `HTTP ${res.status}`);
    const picture = body as WirePicture;
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
    const reportedLat = num(e.attributes.lat);
    const reportedLon = num(e.attributes.lon);
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
        name: text('name') ?? text('place') ?? text('area'),
        callsign: text('callsign'),
        squawk: text('squawk') ?? (num(e.attributes.squawk) !== undefined ? String(e.attributes.squawk) : undefined),
        category: text('category'),
        onGround: e.attributes.on_ground === true,
        lat: reportedLat,
        lon: reportedLon,
        altM,
        magnitude: num(e.attributes.mag),
        severityLevel: num(e.attributes.severity_level),
        station: isStation(e) || undefined,
        shipType: e.kind === 'sea-vessel' ? textAttr(e, 'ship_type') : undefined,
        facts: factsOf(e),
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
    for (const e of p.entities) {
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
    };
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
      res = await fetch(`${this.base}/heimdall/history?${params}`, {
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
