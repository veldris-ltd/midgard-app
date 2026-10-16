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
 * (default: the UK and Ireland). */
import type { AuditRecord, PolicyContext } from '../types';
import {
  HeimdallMock,
  type HeimdallDecision,
  type HeimdallEntity,
  type HeimdallFeed,
  type HeimdallGateway,
  type HeimdallLayer,
  type HeimdallLayerId,
  type HeimdallPicture,
  type HeimdallSyncSite,
  type HeimdallTrack,
  type HeimdallWorkflow,
} from './heimdall';

/** A lat/lon box the canvas maps onto 0–100 %. */
export interface HeimdallRegion {
  west: number;
  east: number;
  south: number;
  north: number;
}

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
const LAYERS: HeimdallLayerId[] = ['air', 'maritime', 'ground', 'feeds'];

const num = (v: unknown): number | undefined => (typeof v === 'number' ? v : undefined);
const hhmm = (unix: number): string =>
  new Date(unix * 1000).toISOString().slice(11, 16);
const ago = (unix: number): string => {
  const s = Math.max(0, Math.round(Date.now() / 1000 - unix));
  return s < 120 ? `${s}s` : `${Math.round(s / 60)}m`;
};

function altitude(e: WireEntity): string {
  if (e.attributes.on_ground === true) return 'GND';
  const ft = num(e.attributes.alt_ft);
  if (ft === undefined) return '—';
  return ft >= 6000 ? `FL${Math.round(ft / 100).toString().padStart(3, '0')}` : `${ft} ft`;
}

function classOf(e: WireEntity): string {
  return (e.classification?.label ?? (e.kind === 'air-track' ? 'aircraft' : e.kind))
    .replace(/-/g, ' ')
    .toUpperCase();
}

export class HeimdallHttp implements HeimdallGateway {
  private readonly base: string;
  private readonly token: string;
  private readonly region: HeimdallRegion;
  private readonly onUnauthorized?: () => void;
  /** Screens without a service behind them yet. */
  private readonly simulated = new HeimdallMock();

  constructor(opts: HeimdallHttpOptions) {
    this.base = opts.baseUrl.replace(/\/$/, '');
    this.token = opts.token;
    this.region = opts.region ?? UK_AND_IRELAND;
    this.onUnauthorized = opts.onUnauthorized;
  }

  policy(): PolicyContext {
    return this.simulated.policy();
  }

  private async picture(): Promise<WirePicture> {
    const res = await fetch(`${this.base}/heimdall/picture`, {
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

  private track(e: WireEntity): HeimdallTrack | null {
    const lat = num(e.attributes.lat);
    const lon = num(e.attributes.lon);
    if (lat === undefined || lon === undefined) return null;
    const { west, east, south, north } = this.region;
    const x = ((lon - west) / (east - west)) * 100;
    const y = ((north - lat) / (north - south)) * 100;
    if (x < 0 || x > 100 || y < 0 || y > 100) return null;
    const layer = (LAYERS as string[]).includes(e.layer) ? (e.layer as HeimdallLayerId) : 'air';
    const callsign = typeof e.attributes.callsign === 'string' ? e.attributes.callsign : '';
    return {
      id: callsign || e.id,
      cls: classOf(e),
      conf: e.classification ? 1 : 0.5,
      x,
      y,
      layer,
      track: ALERT_LABELS.has(e.classification?.label ?? '') ? 'anomalous' : 'stable',
      hdg: num(e.attributes.track) ?? 0,
      spd: Math.round(num(e.attributes.speed) ?? 0),
      alt: altitude(e),
      feeds: e.sources.length,
    };
  }

  async getPicture(): Promise<HeimdallPicture> {
    const p = await this.picture();
    const layers: HeimdallLayer[] = p.layers
      .filter((l) => (LAYERS as string[]).includes(l.id))
      .map((l) => ({ id: l.id as HeimdallLayerId, label: l.name, active: l.visible }));
    // Track ids label the canvas: a callsign where it is unique and real, else the
    // ICAO address (some transponders send the placeholder callsign 00000000).
    const used = new Set<string>();
    const tracks: HeimdallTrack[] = [];
    for (const e of p.entities) {
      const t = this.track(e);
      if (!t) continue;
      if (used.has(t.id) || /^0+$/.test(t.id)) t.id = e.id;
      used.add(t.id);
      tracks.push(t);
    }
    return { layers, tracks, entities: p.entities.map((e) => this.entity(e)) };
  }

  async listEntities(): Promise<HeimdallEntity[]> {
    return (await this.picture()).entities.map((e) => this.entity(e));
  }

  /** One row per feed that has contributed to the picture: every observation was
   *  admitted only after its ML-DSA source signature verified. */
  async listFeeds(): Promise<HeimdallFeed[]> {
    const p = await this.picture();
    const feeds = new Map<string, { tracks: number; newest: number }>();
    for (const e of p.entities) {
      for (const s of e.sources) {
        const f = feeds.get(s) ?? { tracks: 0, newest: 0 };
        f.tracks += 1;
        f.newest = Math.max(f.newest, e.lastSeen);
        feeds.set(s, f);
      }
    }
    return [...feeds.entries()].map(([name, f]) => {
      const stale = Date.now() / 1000 - f.newest > 300;
      return {
        name,
        kind: 'transponder (ADS-B)',
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
