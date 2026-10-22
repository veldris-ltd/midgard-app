/* The Forge console's live client: the enclosure's real data operations, read from the
 * `http-gateway` edge (`GET /forge/overview`), which signs the user in and authorises
 * and audits the read. Two kinds of pipeline feed the enclosure:
 *
 * - Forge jobs (the jobs file the `forge-worker` service runs): a source file is read,
 *   versioned in Forge's sealed dataset store, and projected into Midgard object and
 *   link types, each sync audited by the gateway;
 * - Heimdall feeds: each source is fetched, normalised and signed (ML-DSA) by the feed
 *   runner and fused into Heimdall's picture; its significant items reach Midgard as
 *   `heimdall_track` / `heimdall_alert` objects.
 *
 * Every figure comes from the overview: job definitions, each job's and feed's last
 * run (outcome, counts, timing, totals since its runner started) and object counts.
 * Nothing is invented; a value the system does not have is shown as "—". Pro-code
 * transforms are not editable here (jobs are declared in the jobs file), so preview and
 * apply are refused. [FR-F-2] [FR-F-3] [FR-F-4] [FR-H-1] */
import type { AuditRecord, MarkingLevel } from '../types';
import type {
  ForgeApplyResult,
  ForgeBuildResult,
  ForgeConnector,
  ForgeDataset,
  ForgeGateway,
  ForgeHealthCheck,
  ForgePipeline,
  ForgePipelineNode,
  ForgePreviewResult,
  ForgeProjection,
  ForgeSchedule,
  ForgeTransform,
} from './forge';
import { ForgeMock } from './forge';

export interface ForgeHttpOptions {
  /** Base URL of the HTTP/JSON edge, e.g. http://127.0.0.1:7710/api/v1 */
  baseUrl: string;
  /** Bearer of the PQ-signed token from `login`. */
  token: string;
  /** Called when the gateway answers 401 (the session expired). */
  onUnauthorized?: () => void;
}

interface WireColumn { name: string; type: string; nullable?: boolean }
interface WireJob {
  id: string;
  every: number;
  source: { file?: string; url?: string; columns: WireColumn[] };
  objects: { type: string; key: string; marking: string; purposes: string[]; properties: Record<string, string> };
  links?: { type: string; from: string; to: string }[];
}
interface WireJobStatus {
  id: string; every: number; runs: number; failures: number; lastRun?: number; durationMs?: number;
  outcome?: string; message?: string; lastSuccess?: number; version?: string; rows?: number; objects?: number; links?: number;
}
interface WireFeed {
  feed: string; every: number; description: string; lastRun?: number; durationS?: number; outcome?: string;
  message?: string; fetched?: number | null; sent?: number | null; accepted?: number | null; refused?: number | null;
  runs: number; failures: number; lastSuccess?: number;
}
interface Overview {
  jobs: WireJob[];
  jobStatus: WireJobStatus[];
  statusAt: number | null;
  feeds: Record<string, WireFeed>;
  feedsAt: number | null;
  objectCounts: Record<string, number>;
}

/** Where each Forge job's file comes from (the fetcher's sources). */
const JOB_UPSTREAM: Record<string, [string, string]> = {
  aircraft: ['adsb.lol', 'ADS-B aircraft positions, UK and Ireland · ODbL'],
  weather: ['aviationweather.gov', 'METAR airport weather reports · NOAA, public domain'],
  floods: ['environment.data.gov.uk', 'Flood warnings, England · Open Government Licence'],
  earthquakes: ['earthquake.usgs.gov', 'Earthquakes M2.5+ worldwide · USGS, public domain'],
  air_quality: ['data.sensor.community', 'Citizen air-quality sensors, GB · ODbL'],
  tfl: ['api.tfl.gov.uk', 'London line status · TfL Open Data'],
  grid_frequency: ['data.elexon.co.uk', 'GB system frequency · Elexon BMRS open data'],
  grid_fuel: ['data.elexon.co.uk', 'GB generation by fuel · Elexon BMRS open data'],
  grid_intensity_linked: ['carbonintensity.org.uk', 'Grid carbon intensity by region · NESO, CC BY 4.0'],
  grid_intensity_other: ['carbonintensity.org.uk', 'Grid carbon intensity, other regions · NESO, CC BY 4.0'],
  space_weather: ['services.swpc.noaa.gov', 'Planetary Kp index · NOAA, public domain'],
};

/** Heimdall feeds grouped into pipelines by what they describe. */
const FEED_GROUPS: [string, string, RegExp][] = [
  ['heimdall_air', 'Air', /^(adsb|openaip|aviationweather|airports-metar|gnss|sondehub)/],
  ['heimdall_maritime', 'Maritime', /^(aisstream|digitraffic|crown-estate|nsta)/],
  ['heimdall_land', 'Land & transport', /^(network-rail|national-highways|tfl|drivebc|mrwa|nzta)/],
  ['heimdall_cameras', 'Cameras', /cams$|^custom-cameras$/],
  ['heimdall_utilities', 'Utilities', /^(power|gb-grid|storm|reservoirs|water|gas|gie|internet|cloudflare|telecom)/],
  ['heimdall_hazards', 'Weather & hazards', /^(ea-floods|metoffice|meteoalarm|eccc|metservice|gdacs|nasa-firms|usgs)/],
  ['heimdall_space', 'Space', /^(celestrak|noaa-swpc)/],
];
const groupOf = (name: string) => FEED_GROUPS.find(([, , re]) => re.test(name)) ?? ['heimdall_other', 'Other', /./];

const now = () => Date.now() / 1000;
const ago = (t?: number) => {
  if (!t) return '—';
  const s = Math.max(0, Math.round(now() - t));
  return s < 60 ? `${s}s ago` : s < 3600 ? `${Math.round(s / 60)}m ago` : s < 172800 ? `${Math.round(s / 3600)}h ago` : `${Math.round(s / 86400)}d ago`;
};
const every = (s: number) => (s % 3600 === 0 ? `${s / 3600}h` : s % 60 === 0 ? `${s / 60}m` : `${s}s`);
const n = (v?: number | null) => (v == null ? '—' : v.toLocaleString('en-GB'));
const short = (v?: string) => (v ? v.slice(0, 12) : '—');
const pct = (runs: number, failures: number) => (runs ? Math.round(((runs - failures) / runs) * 1000) / 10 : 0);
const clock = (t?: number) => (t ? new Date(t * 1000).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' }) : '—');
const band = (m: string): MarkingLevel => (['open', 'internal', 'restricted', 'confidential'].includes(m) ? (m as MarkingLevel) : 'internal');

/** How fresh a run must be: within twice the interval (and at least five minutes). */
const fresh = (lastSuccess: number | undefined, interval: number) => !!lastSuccess && now() - lastSuccess <= Math.max(2 * interval, 300);

type State = 'healthy' | 'online' | 'warn';
const feedState = (f: WireFeed): [State, string] => {
  if (!f.lastRun) return ['warn', 'Not run yet'];
  if (f.outcome === 'failed') return ['warn', 'Failing'];
  if (f.outcome === 'rate limited') return ['warn', 'Rate limited'];
  if (!fresh(f.lastSuccess, f.every)) return ['warn', `Stale · ${ago(f.lastSuccess)}`];
  return f.every <= 120 ? ['online', 'Live'] : ['healthy', 'Healthy'];
};
const jobState = (s?: WireJobStatus): [State, string] => {
  if (!s?.lastRun) return ['warn', 'Not run yet'];
  if (s.outcome === 'failed') return ['warn', 'Failing'];
  if (!fresh(s.lastSuccess, s.every)) return ['warn', `Stale · ${ago(s.lastSuccess)}`];
  return s.every <= 60 ? ['online', 'Live'] : ['healthy', 'Healthy'];
};

export class ForgeHttp extends ForgeMock implements ForgeGateway {
  /** This client shows the running system: builds run on their schedules. */
  readonly live = true;
  private readonly base: string;
  private readonly token: string;
  private readonly onUnauthorized?: () => void;
  private cache: { at: number; value: Promise<Overview> } | null = null;

  constructor(opts: ForgeHttpOptions) {
    super();
    this.base = opts.baseUrl.replace(/\/$/, '');
    this.token = opts.token;
    this.onUnauthorized = opts.onUnauthorized;
  }

  private async get(path: string): Promise<any> {
    const res = await fetch(`${this.base}${path}`, { headers: { authorization: `Bearer ${this.token}` } });
    if (res.status === 401) {
      this.onUnauthorized?.();
      throw new Error('session expired: sign in again');
    }
    const body = await res.json().catch(() => null);
    if (!res.ok) throw new Error(body?.error ?? `HTTP ${res.status}`);
    return body;
  }

  /** The overview, shared by the screens for a few seconds (one audited read). */
  private overview(): Promise<Overview> {
    if (this.cache && Date.now() - this.cache.at < 5000) return this.cache.value;
    const value = this.get('/forge/overview') as Promise<Overview>;
    this.cache = { at: Date.now(), value };
    value.catch(() => { this.cache = null; });
    return value;
  }

  private static status(o: Overview, id: string) {
    return o.jobStatus.find((s) => s.id === id);
  }

  private static feedList(o: Overview): [string, WireFeed][] {
    return Object.entries(o.feeds).sort(([a], [b]) => a.localeCompare(b));
  }

  // ---- pipelines --------------------------------------------------------------

  async listPipelines(): Promise<ForgePipeline[]> {
    const o = await this.overview();
    const forge = o.jobs.map((j): ForgePipeline => {
      const s = ForgeHttp.status(o, j.id);
      const [state] = jobState(s);
      const [host, what] = JOB_UPSTREAM[j.id] ?? [j.source.file ?? j.id, ''];
      const links = j.links ?? [];
      const nodes: ForgePipelineNode[] = [
        { id: 'src', kind: 'source', label: host, icon: 'radio-tower', x: 0, y: 1, status: state === 'warn' ? 'warn' : 'healthy', meta: `HTTPS · pull ${every(j.every)}` },
        { id: 'fetch', kind: 'transform', label: `${j.source.file ?? j.id}`, icon: 'workflow', x: 1, y: 1, status: 'healthy', meta: `${j.source.columns.length} typed columns` },
        { id: 'ds', kind: 'dataset', label: j.id, icon: 'database', x: 2, y: 1, status: state === 'warn' ? 'warn' : 'healthy', meta: `${short(s?.version)} · ${n(s?.rows)} rows` },
        { id: 'proj', kind: 'projection', label: `${j.objects.type}`, icon: 'boxes', x: 3, y: 1, status: 'online', meta: `→ Midgard · ${n(o.objectCounts[j.objects.type])} objects` },
        ...links.map((l, i): ForgePipelineNode => ({ id: `link${i}`, kind: 'projection', label: l.type, icon: 'network', x: 3, y: 2 + i, status: 'online', meta: `link · ${l.from} → ${l.to}` })),
      ];
      return {
        id: j.id,
        name: j.id,
        context: j.id.toUpperCase(),
        branch: 'main',
        currentVersion: short(s?.version),
        expectations: s ? `${s.runs - s.failures}/${s.runs} runs` : '—',
        marking: band(j.objects.marking),
        compartment: null,
        nodes,
        edges: [
          { from: 'src', to: 'fetch' }, { from: 'fetch', to: 'ds' }, { from: 'ds', to: 'proj' },
          ...links.map((_, i) => ({ from: 'ds', to: `link${i}` })),
        ],
        versions: s?.lastRun ? [{ version: short(s.version), branch: 'main', ts: ago(s.lastRun), status: s.outcome === 'failed' ? 'FAIL' : 'OK' }] : [],
        suggestion: what,
      };
    });
    const groups = new Map<string, { title: string; feeds: [string, WireFeed][] }>();
    for (const [name, f] of ForgeHttp.feedList(o)) {
      const [id, title] = groupOf(name);
      const g = groups.get(id) ?? { title, feeds: [] };
      g.feeds.push([name, f]);
      groups.set(id, g);
    }
    const heimdall = [...groups.entries()].map(([id, g]): ForgePipeline => {
      const runs = g.feeds.reduce((a, [, f]) => a + f.runs, 0);
      const failures = g.feeds.reduce((a, [, f]) => a + f.failures, 0);
      // Feeds in a grid of three columns (rows closer than the canvas's default), then
      // the picture and the fabric, so a group of a dozen feeds fits on one screen.
      const COLS = 3;
      const STEP = 0.62;
      const rows = Math.ceil(g.feeds.length / COLS);
      const mid = 1 + ((rows - 1) * STEP) / 2;
      const nodes: ForgePipelineNode[] = [
        ...g.feeds.map(([name, f], i): ForgePipelineNode => {
          const [state] = feedState(f);
          return { id: `f${i}`, kind: 'source', label: name, icon: 'radio-tower', x: (i % COLS) * 0.8, y: 1 + Math.floor(i / COLS) * STEP, status: state === 'warn' ? 'warn' : 'healthy', meta: `pull ${every(f.every)} · ${n(f.fetched)} items` };
        }),
        { id: 'fuse', kind: 'dataset', label: `Heimdall picture · ${g.title.toLowerCase()}`, icon: 'layers', x: 2.45, y: mid, status: 'healthy', meta: 'signed (ML-DSA) · fused tracks' },
        { id: 'fabric', kind: 'projection', label: 'heimdall_track · alert', icon: 'boxes', x: 3.25, y: mid, status: 'online', meta: `→ Midgard · ${n((o.objectCounts.heimdall_track ?? 0) + (o.objectCounts.heimdall_alert ?? 0))} objects` },
      ];
      return {
        id,
        name: `heimdall · ${g.title.toLowerCase()}`,
        context: `HEIMDALL · ${g.title.toUpperCase()}`,
        branch: 'main',
        currentVersion: `${g.feeds.length} feeds`,
        expectations: runs ? `${runs - failures}/${runs} runs` : '—',
        marking: 'internal',
        compartment: null,
        nodes,
        edges: [...g.feeds.map((_, i) => ({ from: `f${i}`, to: 'fuse' })), { from: 'fuse', to: 'fabric' }],
        versions: g.feeds
          .filter(([, f]) => f.lastRun)
          .sort(([, a], [, b]) => (b.lastRun ?? 0) - (a.lastRun ?? 0))
          .slice(0, 6)
          .map(([name, f]) => ({ version: name, branch: f.outcome ?? '—', ts: ago(f.lastRun), status: f.outcome === 'failed' ? 'FAIL' : 'OK' })),
        suggestion: '',
      };
    });
    return [...forge, ...heimdall];
  }

  /** The pipeline's last real run, step by step (pipelines run on their schedules;
   *  the console does not start one). */
  async runPipeline(id: string): Promise<ForgeBuildResult> {
    const o = await this.overview();
    const job = o.jobs.find((j) => j.id === id);
    if (job) {
      const s = ForgeHttp.status(o, id);
      const failed = s?.outcome === 'failed';
      return {
        runId: `${id}@${s?.lastRun ?? 0}`,
        version: short(s?.version),
        durationLabel: s?.durationMs != null ? `${(s.durationMs / 1000).toFixed(1)}s` : '—',
        steps: [
          { stage: 'last run', detail: `${clock(s?.lastRun)} · ${ago(s?.lastRun)} · runs every ${every(job.every)}`, status: 'ok' },
          { stage: 'read', detail: `${job.source.file ?? job.id} · ${n(s?.rows)} rows · ${job.source.columns.length} typed columns`, status: 'ok' },
          { stage: 'version', detail: `dataset ${id} @ ${short(s?.version)} · sealed`, status: 'ok' },
          { stage: 'project', detail: `${n(s?.objects)} ${job.objects.type} objects · ${n(s?.links)} links`, status: 'ok' },
          { stage: 'sync', detail: s?.message ?? '—', status: failed ? 'warn' : 'ok' },
          { stage: 'done', detail: `${s?.outcome ?? 'not run yet'} · ${s ? `${s.runs - s.failures}/${s.runs} runs ok since the worker started` : ''}`, status: failed ? 'warn' : 'ok' },
        ],
      };
    }
    const feeds = ForgeHttp.feedList(o).filter(([name]) => groupOf(name)[0] === id);
    return {
      runId: `${id}@${Math.round(now())}`,
      version: `${feeds.length} feeds`,
      durationLabel: '—',
      steps: [
        ...feeds.map(([name, f]) => ({
          stage: name.slice(0, 18),
          detail: `${ago(f.lastRun)} · ${f.outcome ?? 'not run'} · fetched ${n(f.fetched)} · sent ${n(f.sent)}${f.accepted != null ? ` · accepted ${n(f.accepted)}` : ''}`,
          status: (f.outcome === 'failed' || f.outcome === 'rate limited' ? 'warn' : 'ok') as 'ok' | 'warn',
        })),
        { stage: 'done', detail: `${feeds.filter(([, f]) => f.outcome !== 'failed').length}/${feeds.length} feeds ok on their last run`, status: 'ok' },
      ],
    };
  }

  // ---- connectors -------------------------------------------------------------

  async listConnectors(): Promise<ForgeConnector[]> {
    const o = await this.overview();
    const forge = o.jobs.map((j): ForgeConnector => {
      const s = ForgeHttp.status(o, j.id);
      const [state, stateLabel] = jobState(s);
      const [host, what] = JOB_UPSTREAM[j.id] ?? [j.source.file ?? j.id, ''];
      return {
        id: `forge-${j.id}`,
        name: j.id,
        kind: 'Forge job',
        cadence: `pull · ${every(j.every)}`,
        datasetsFed: 1,
        volume: s?.rows != null ? `${n(s.rows)} rows` : '—',
        state,
        stateLabel,
        transport: `HTTPS from ${host} → CSV → PQ-mTLS sync`,
        egress: 'none · inbound pull only',
        schema: `${j.source.columns.length} typed columns → ${j.objects.type}${what ? ` · ${what}` : ''}`,
        action: s?.outcome === 'failed' ? (s.message ?? 'last run failed') : null,
        lastRun: [
          `> forge job ${j.id} · ${ago(s?.lastRun)}`,
          `outcome  ${s?.outcome ?? 'not run yet'}`,
          `rows     ${n(s?.rows)} · objects ${n(s?.objects)} · links ${n(s?.links)}`,
          `version  ${short(s?.version)}`,
          `# ${s?.message ?? '—'}`,
        ],
      };
    });
    const feeds = ForgeHttp.feedList(o).map(([name, f]): ForgeConnector => {
      const [state, stateLabel] = feedState(f);
      const stream = /^(aisstream|digitraffic)/.test(name);
      return {
        id: `feed-${name}`,
        name,
        kind: stream ? 'Heimdall feed · stream' : 'Heimdall feed',
        cadence: `${stream ? 'stream' : 'pull'} · ${every(f.every)}`,
        datasetsFed: 1,
        volume: f.fetched != null ? `${n(f.fetched)} items` : '—',
        state,
        stateLabel,
        transport: 'HTTPS → signed observations (ML-DSA) → PQ-mTLS',
        egress: 'none · inbound pull only',
        schema: f.description || name,
        action: f.outcome === 'failed' || f.outcome === 'rate limited' ? (f.message ?? f.outcome) : null,
        lastRun: [
          `> heimdall feed ${name} · ${ago(f.lastRun)}`,
          `outcome  ${f.outcome ?? 'not run yet'} · ${f.durationS ?? '—'}s`,
          `fetched  ${n(f.fetched)} · sent ${n(f.sent)} new`,
          ...(f.accepted != null ? [`heimdall accepted ${n(f.accepted)} · refused ${n(f.refused)}`] : []),
          `# ${f.runs - f.failures}/${f.runs} runs ok since the feeds started`,
        ],
      };
    });
    return [...forge, ...feeds];
  }

  // ---- datasets -----------------------------------------------------------------

  async listDatasets(): Promise<ForgeDataset[]> {
    const o = await this.overview();
    const forge = o.jobs.map((j): ForgeDataset => {
      const s = ForgeHttp.status(o, j.id);
      const [state, stateLabel] = jobState(s);
      const [host] = JOB_UPSTREAM[j.id] ?? [j.source.file ?? j.id];
      return {
        id: `ds-${j.id}`,
        name: j.id,
        version: short(s?.version),
        branch: 'main',
        rows: n(s?.rows),
        columns: j.source.columns.length,
        marking: band(j.objects.marking),
        state: state === 'warn' ? 'warn' : 'secure',
        stateLabel: state === 'warn' ? stateLabel : 'Sealed',
        diff: j.source.columns.map((c) => ({ text: `${c.name} : ${c.type}${c.nullable ? ' (optional)' : ''}`, tone: 'add' as const })),
        diffTitle: `SCHEMA · ${j.source.columns.length} TYPED COLUMNS`,
        seal: [
          `> forge dataset ${j.id}`,
          `version  ${short(s?.version)} (content hash)`,
          `store    Forge dataset store · KMS-sealed at rest`,
          `audit    every run in the worker's ML-DSA ledger`,
          `# last synced ${ago(s?.lastSuccess)}`,
        ],
        lineage: [
          { icon: 'radio-tower', label: host },
          { icon: 'file-text', label: j.source.file ?? j.id },
          { icon: 'database', label: `${j.id} @ ${short(s?.version)}` },
          { icon: 'boxes', label: `${j.objects.type} · ${n(o.objectCounts[j.objects.type])} objects` },
        ],
      };
    });
    const feeds = ForgeHttp.feedList(o).map(([name, f]): ForgeDataset => {
      const [state, stateLabel] = feedState(f);
      return {
        id: `ds-${name}`,
        name: `${name} · observations`,
        version: f.lastSuccess ? clock(f.lastSuccess) : '—',
        branch: 'live',
        rows: n(f.fetched),
        columns: 0,
        marking: 'internal',
        state: state === 'warn' ? 'warn' : 'healthy',
        stateLabel: state === 'warn' ? stateLabel : 'Signed',
        diff: [
          { text: `fetched ${n(f.fetched)} · sent ${n(f.sent)} new`, tone: 'add' },
          ...(f.accepted != null ? [{ text: `accepted ${n(f.accepted)} · refused ${n(f.refused)}`, tone: 'change' as const }] : []),
        ],
        diffTitle: 'LAST RUN',
        seal: [
          `> heimdall feed ${name}`,
          'sign     ML-DSA per observation, checked by Heimdall',
          'carry    PQ-mTLS into the enclosure',
          'audit    one ledger entry per batch',
          `# last success ${ago(f.lastSuccess)}`,
        ],
        lineage: [
          { icon: 'radio-tower', label: name },
          { icon: 'shield-check', label: 'signed observations (ML-DSA)' },
          { icon: 'layers', label: `Heimdall picture · ${groupOf(name)[1].toLowerCase()}` },
        ],
      };
    });
    return [...forge, ...feeds];
  }

  // ---- health ---------------------------------------------------------------------

  async listHealthChecks(): Promise<ForgeHealthCheck[]> {
    const o = await this.overview();
    const checks: ForgeHealthCheck[] = [];
    for (const j of o.jobs) {
      const s = ForgeHttp.status(o, j.id);
      const runs = s?.runs ?? 0;
      const ok = pct(runs, s?.failures ?? 0);
      const isFresh = fresh(s?.lastSuccess, j.every);
      checks.push({
        id: `fresh-${j.id}`, dataset: j.id, check: `synced within ${every(Math.max(2 * j.every, 300))}`, kind: 'freshness',
        passRate: isFresh ? 'pass' : 'fail', passValue: isFresh ? 100 : 0, trend: `last ${ago(s?.lastSuccess)}`,
        state: isFresh ? 'secure' : 'warn', stateLabel: isFresh ? 'Passing' : 'Stale', projectionGate: isFresh ? 'CLEAR' : 'BLOCKS', diagnosis: isFresh ? null : (s?.message ?? 'no successful run yet'),
      });
      checks.push({
        id: `runs-${j.id}`, dataset: j.id, check: `typed columns parse and the gateway accepts the sync`, kind: 'schema · sync',
        passRate: runs ? `${ok}%` : '—', passValue: ok, trend: `${runs - (s?.failures ?? 0)}/${runs} runs`,
        state: (s?.failures ?? 0) === 0 ? 'secure' : 'warn', stateLabel: s?.outcome === 'failed' ? 'Failing' : 'Passing',
        projectionGate: s?.outcome === 'failed' ? 'BLOCKS' : 'CLEAR', diagnosis: s?.outcome === 'failed' ? (s.message ?? null) : null,
      });
    }
    for (const [name, f] of ForgeHttp.feedList(o)) {
      const ok = pct(f.runs, f.failures);
      const isFresh = fresh(f.lastSuccess, f.every);
      checks.push({
        id: `feed-${name}`, dataset: name, check: `fetches succeed and Heimdall accepts the signed batch`, kind: 'freshness · auth',
        passRate: f.runs ? `${ok}%` : '—', passValue: ok, trend: `${f.runs - f.failures}/${f.runs} runs · last ${ago(f.lastSuccess)}`,
        state: isFresh && f.outcome !== 'failed' ? 'secure' : 'warn', stateLabel: f.outcome === 'failed' ? 'Failing' : isFresh ? 'Passing' : 'Stale',
        projectionGate: f.outcome === 'failed' ? 'BLOCKS' : 'CLEAR', diagnosis: f.outcome === 'failed' || f.outcome === 'rate limited' ? (f.message ?? null) : null,
      });
    }
    return checks;
  }

  // ---- schedules --------------------------------------------------------------------

  async listSchedules(): Promise<ForgeSchedule[]> {
    const o = await this.overview();
    const next = (last: number | undefined, interval: number) => {
      if (!last) return 'on start';
      const d = Math.round(last + interval - now());
      return d <= 0 ? 'due' : d < 60 ? `in ${d}s` : `in ${Math.round(d / 60)}m`;
    };
    const forge = o.jobs.map((j): ForgeSchedule => {
      const s = ForgeHttp.status(o, j.id);
      const [state, stateLabel] = jobState(s);
      return {
        id: `sch-${j.id}`, pipeline: j.id, mode: 'batch', trigger: `every ${every(j.every)}`,
        last: s?.lastRun ? `${s.outcome === 'failed' ? 'FAILED' : 'OK'} · ${s.durationMs != null ? `${(s.durationMs / 1000).toFixed(1)}s` : '—'} · ${ago(s.lastRun)}` : '—',
        next: next(s?.lastRun, j.every), state, stateLabel,
        backfill: 'from checkpoint', concurrency: '1', onFailure: 'retry in 10s', note: s?.outcome === 'failed' ? (s.message ?? null) : null,
      };
    });
    const feeds = ForgeHttp.feedList(o).map(([name, f]): ForgeSchedule => {
      const [state, stateLabel] = feedState(f);
      const stream = /^(aisstream|digitraffic)/.test(name);
      return {
        id: `sch-${name}`, pipeline: name, mode: stream ? 'stream' : 'batch', trigger: `every ${every(f.every)}`,
        last: f.lastRun ? `${(f.outcome ?? '').toUpperCase()} · ${f.durationS ?? '—'}s · ${ago(f.lastRun)}` : '—',
        next: next(f.lastRun, f.every), state, stateLabel,
        backfill: 'only new items', concurrency: '1', onFailure: f.outcome === 'rate limited' ? 'back off' : 'retry next cycle',
        note: f.outcome === 'failed' || f.outcome === 'rate limited' ? (f.message ?? null) : null,
      };
    });
    return [...forge, ...feeds];
  }

  // ---- transforms (read-only: jobs are declared in the jobs file) ------------------

  async getTransform(): Promise<ForgeTransform> {
    const o = await this.overview();
    const j = o.jobs.find((x) => x.id === 'weather') ?? o.jobs[0];
    const code = JSON.stringify(j, null, 2).split('\n');
    return {
      id: j?.id ?? '—',
      branch: 'main',
      nextVersion: short(ForgeHttp.status(o, j?.id ?? '')?.version),
      filesCount: o.jobs.length,
      expectations: `${j?.source.columns.length ?? 0} typed columns`,
      files: o.jobs.map((x) => `jobs.json · ${x.id}`),
      activeFile: `jobs.json · ${j?.id ?? ''}`,
      code,
      codeByFile: Object.fromEntries(o.jobs.map((x) => [`jobs.json · ${x.id}`, JSON.stringify(x, null, 2).split('\n')])),
      contractChecks: [
        ['columns typed', `${j?.source.columns.length ?? 0}/${j?.source.columns.length ?? 0}`],
        ['object key', j?.objects.key ?? '—'],
        ['marking', j?.objects.marking ?? '—'],
        ['purposes', (j?.objects.purposes ?? []).join(', ') || '—'],
        ['links', String(j?.links?.length ?? 0)],
      ],
    };
  }

  async previewTransform(): Promise<ForgePreviewResult> {
    throw new Error('Forge jobs are declared in the jobs file; they are not edited from the console');
  }

  async applyTransform(): Promise<ForgeApplyResult> {
    throw new Error('Forge jobs are declared in the jobs file; they are not edited from the console');
  }

  // ---- projections --------------------------------------------------------------------

  async listProjections(): Promise<ForgeProjection[]> {
    const o = await this.overview();
    return o.jobs.map((j): ForgeProjection => {
      const s = ForgeHttp.status(o, j.id);
      const props = Object.entries(j.objects.properties);
      const links = j.links ?? [];
      const typed = (col: string) => j.source.columns.find((c) => c.name === col)?.type ?? '?';
      return {
        id: `proj-${j.id}`,
        name: `${j.id} → ${j.objects.type}`,
        source: `${j.id} · ${short(s?.version)}`,
        target: `${j.objects.type} · object fabric`,
        objects: n(o.objectCounts[j.objects.type]),
        syncLag: ago(s?.lastSuccess),
        columnsMapped: `${props.length + 1 + links.length}/${j.source.columns.length}`,
        linkTypes: links.length,
        unmarkedRows: 0,
        marking: band(j.objects.marking),
        compartment: null,
        mappings: [
          { id: 'key', column: j.objects.key, to: `${j.objects.type} key`, kind: 'key', note: 'one object per key; a newer row replaces it' },
          ...props.map(([to, col]) => ({ id: `p-${to}`, column: col, to: `${j.objects.type}.${to}`, kind: 'property' as const, note: typed(col) })),
          ...links.map((l, i) => ({ id: `l${i}`, column: `${l.from} → ${l.to}`, to: l.type, kind: 'link' as const, note: 'the target must exist in the fabric' })),
          { id: 'mk', column: '(every row)', to: `marking ${j.objects.marking}`, kind: 'governance', note: `purposes: ${j.objects.purposes.join(', ')}` },
        ],
      };
    });
  }

  // ---- audit (the enclosure's ledger: Forge's syncs and their decisions) ---------------

  async getAudit(limit = 40): Promise<AuditRecord[]> {
    const rows = (await this.get(`/audit?limit=500`)) as AuditRecord[];
    return rows.filter((r) => r.actor === 'forge-worker' || r.actor === 'heimdall-fabric').slice(0, limit);
  }

  auditStream(onRecord: (record: AuditRecord) => void): () => void {
    const seen = new Set<string>();
    let first = true;
    const poll = async () => {
      try {
        const rows = await this.getAudit(40);
        for (const r of rows.slice().reverse()) {
          if (seen.has(r.id)) continue;
          seen.add(r.id);
          if (!first) onRecord(r);
        }
        first = false;
      } catch {
        /* the next poll tries again */
      }
    };
    void poll();
    const t = setInterval(poll, 5000);
    return () => clearInterval(t);
  }
}
