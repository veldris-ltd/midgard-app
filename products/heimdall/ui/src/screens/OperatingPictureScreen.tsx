import * as React from 'react';
import {
  Badge,
  Card,
  HumanGate,
  Icon,
  IconButton,
  MarkingChip,
  MeterBar,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  Switch,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import { HumanGateRequiredError } from '@midgard/sdk';
import type { HeimdallGateway, HeimdallLayerId, HeimdallPicture, HeimdallRegion, HeimdallTrack } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { BORDER_PATH, LAND_PATH } from '../app/basemap';

type LayerState = Record<HeimdallLayerId, boolean>;
type GateState = { rank: number; state: 'pending' | 'authorised' | 'rejected'; auditId?: string } | null;

const CAPABILITY: Record<string, string> = {
  AIRCRAFT: 'IFF · ADS-B · RADAR',
  VESSEL: 'AIS · RADAR',
  'GROUND UNIT': 'OBSERVER NET',
  UNKNOWN: 'RADAR ONLY · UNCORROBORATED',
};

/** Advisory option rank → the human-gated workflow it would open. Rank 3 (hold)
 *  opens no workflow — the operator simply defers. */
const WORKFLOW_FOR_RANK: Record<number, string | undefined> = { 1: 'W-0912', 2: 'W-0913', 3: undefined };

/* ---- live (geographic) picture helpers. [FR-H-1] [FR-H-2] ---------------------- */

/** Altitude bands that colour live tracks. */
const BANDS = [
  { id: 'gnd', label: 'ON GROUND', color: 'var(--text-faint)', test: (t: HeimdallTrack) => !!t.detail?.onGround },
  { id: 'low', label: 'BELOW 10,000 FT', color: 'var(--secure)', test: (t: HeimdallTrack) => (t.altFt ?? 0) < 10000 },
  { id: 'mid', label: '10,000–25,000 FT', color: 'var(--info)', test: (t: HeimdallTrack) => (t.altFt ?? 0) < 25000 },
  { id: 'high', label: 'ABOVE 25,000 FT', color: 'var(--accent)', test: () => true },
] as const;
const bandOf = (t: HeimdallTrack) => BANDS.find((b) => b.test(t))!;

/** The emergency squawks: hijack, radio failure, general emergency. */
const SQUAWKS: Record<string, string> = { '7500': 'UNLAWFUL INTERFERENCE', '7600': 'RADIO FAILURE', '7700': 'EMERGENCY' };

const secsAgo = (unix: number) => Math.max(0, Math.round(Date.now() / 1000 - unix));
const fmtAgo = (s: number) => (s < 120 ? `${s}s` : `${Math.round(s / 60)}m`);
const mono: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em' };

/** Faint land, borders and a 5° graticule behind the tracks, in the picture's region. */
function Basemap({ region }: { region: HeimdallRegion }) {
  const { west, east, south, north } = region;
  const lons: number[] = [];
  const lats: number[] = [];
  for (let l = Math.ceil(west / 5) * 5; l <= east; l += 5) lons.push(l);
  for (let l = Math.ceil(south / 5) * 5; l <= north; l += 5) lats.push(l);
  return (
    <svg
      aria-hidden
      viewBox={`${west} ${-north} ${east - west} ${north - south}`}
      preserveAspectRatio="none"
      style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none' }}
    >
      <path d={LAND_PATH} fill="var(--text-faint)" fillOpacity={0.07} stroke="var(--text-faint)" strokeOpacity={0.35} strokeWidth={1} vectorEffect="non-scaling-stroke" />
      <path d={BORDER_PATH} fill="none" stroke="var(--text-faint)" strokeOpacity={0.25} strokeWidth={1} strokeDasharray="3 3" vectorEffect="non-scaling-stroke" />
      {lons.map((l) => <line key={'o' + l} x1={l} x2={l} y1={-north} y2={-south} stroke="var(--divider)" strokeWidth={1} vectorEffect="non-scaling-stroke" />)}
      {lats.map((l) => <line key={'a' + l} x1={west} x2={east} y1={-l} y2={-l} stroke="var(--divider)" strokeWidth={1} vectorEffect="non-scaling-stroke" />)}
    </svg>
  );
}

/** Graticule labels, in HTML so the text is not stretched with the map. */
function GraticuleLabels({ region }: { region: HeimdallRegion }) {
  const { west, east, south, north } = region;
  const out: React.ReactNode[] = [];
  for (let l = Math.ceil(west / 5) * 5; l <= east; l += 5) {
    out.push(<span key={'o' + l} style={{ ...mono, fontSize: 8, position: 'absolute', bottom: 64, left: `${((l - west) / (east - west)) * 100}%`, transform: 'translateX(4px)', color: 'var(--text-faint)', opacity: 0.7 }}>{Math.abs(l)}°{l < 0 ? 'W' : l > 0 ? 'E' : ''}</span>);
  }
  for (let l = Math.ceil(south / 5) * 5; l <= north; l += 5) {
    out.push(<span key={'a' + l} style={{ ...mono, fontSize: 8, position: 'absolute', left: 6, top: `${((north - l) / (north - south)) * 100}%`, transform: 'translateY(-110%)', color: 'var(--text-faint)', opacity: 0.7 }}>{l}°N</span>);
  }
  return <div aria-hidden style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }}>{out}</div>;
}

/** Range rings round the canvas centre, a north arrow and a scale bar, at true scale
 *  north–south (one degree of latitude is 60 NM). East–west the linear projection
 *  stretches, so the rings are drawn on the north–south scale. */
function GeoRings({ region, height }: { region: HeimdallRegion; height: number }) {
  if (!height) return null;
  const pxPerNm = height / ((region.north - region.south) * 60);
  const lat = (region.north + region.south) / 2;
  const lon = (region.west + region.east) / 2;
  return (
    <div aria-hidden style={{ position: 'absolute', inset: 0, overflow: 'hidden', pointerEvents: 'none' }}>
      {[100, 200, 300].map((nm) => {
        const r = nm * pxPerNm;
        return (
          <React.Fragment key={nm}>
            <div style={{ position: 'absolute', left: '50%', top: '50%', width: r * 2, height: r * 2, marginLeft: -r, marginTop: -r, border: '1px solid var(--divider)', borderRadius: '50%' }} />
            <span style={{ ...mono, fontSize: 8, position: 'absolute', left: '50%', top: `calc(50% - ${r}px)`, transform: 'translate(-50%, -50%)', color: 'var(--text-faint)', background: 'var(--bg-sunken)', padding: '0 5px' }}>{nm} NM</span>
          </React.Fragment>
        );
      })}
      <div style={{ position: 'absolute', top: 12, left: 46, display: 'flex', flexDirection: 'column', gap: 6 }}>
        <span style={{ ...mono, fontSize: 11, color: 'var(--text-muted)' }}>N ↑</span>
        <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
          <span style={{ width: 50 * pxPerNm, height: 0, borderTop: '2px solid var(--text-faint)', position: 'relative' }}>
            <span style={{ position: 'absolute', left: 0, top: -4, width: 2, height: 6, background: 'var(--text-faint)' }} />
            <span style={{ position: 'absolute', right: 0, top: -4, width: 2, height: 6, background: 'var(--text-faint)' }} />
          </span>
          <span style={{ ...mono, fontSize: 8, color: 'var(--text-faint)' }}>50 NM</span>
        </span>
        <span style={{ ...mono, fontSize: 8, color: 'var(--text-faint)' }}>
          CENTRE {lat.toFixed(1)}°N {Math.abs(lon).toFixed(1)}°{lon < 0 ? 'W' : 'E'} · VECTOR ≈ 3 MIN
        </span>
      </div>
    </div>
  );
}

function Row({ k, v, tone }: { k: string; v: React.ReactNode; tone?: string }) {
  return (
    <div style={{ display: 'flex', gap: 10, padding: '3px 0', fontFamily: 'var(--font-mono)', fontSize: 10 }}>
      <span style={{ color: 'var(--text-faint)', width: 92, flex: 'none', letterSpacing: '0.08em' }}>{k}</span>
      <span style={{ color: tone ?? 'var(--text-body)', minWidth: 0, overflowWrap: 'anywhere' }}>{v}</span>
    </div>
  );
}

/** The inspector for one live track: what the feed reported and how Heimdall classed it. */
function LiveTrackPanel({ t, onClose }: { t: HeimdallTrack; onClose: () => void }) {
  const d = t.detail!;
  const band = bandOf(t);
  const sq = d.squawk && SQUAWKS[d.squawk];
  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <Icon name="crosshair" size={17} color={t.track === 'anomalous' ? 'var(--warn)' : band.color} />
        <h2 style={{ fontSize: 'var(--fs-h4)', minWidth: 0 }}>{d.callsign || d.key}</h2>
        <Badge tone={t.track === 'anomalous' ? 'warn' : 'neutral'}>{t.cls}</Badge>
        <button onClick={onClose} style={{ all: 'unset', marginLeft: 'auto', cursor: 'pointer', ...mono, color: 'var(--text-faint)' }}>← SUMMARY</button>
      </div>
      {sq && (
        <div style={{ padding: '9px 12px', border: '1px solid var(--warn)', background: 'var(--warn-tint)', borderRadius: 'var(--radius-sm)', ...mono, color: 'var(--warn)' }}>
          SQUAWK {d.squawk} · {sq}
        </div>
      )}
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 12 }}>
          <StatBlock value={t.alt} label="ALTITUDE" size="sm" />
          <StatBlock value={t.spd + ' KT'} label="GROUND SPEED" size="sm" />
          <StatBlock value={String(Math.round(t.hdg)).padStart(3, '0') + '°'} label="TRACK" size="sm" />
        </div>
      </Card>
      <Card size="sm">
        <Row k="ICAO ADDRESS" v={d.key} />
        <Row k="CALLSIGN" v={d.callsign ?? '—'} />
        <Row k="SQUAWK" v={d.squawk ?? '—'} tone={sq ? 'var(--warn)' : undefined} />
        <Row k="CATEGORY" v={d.category ?? '—'} />
        <Row k="POSITION" v={`${d.lat.toFixed(3)}, ${d.lon.toFixed(3)}`} />
        <Row k="REPORTED" v={`${fmtAgo(secsAgo(d.lastSeen))} ago`} />
        <Row k="SHOWN" v={d.estimatedSecs > 0 ? `estimated ${d.estimatedSecs}s ahead (dead reckoning)` : 'as reported'} tone={d.estimatedSecs > 0 ? 'var(--text-muted)' : undefined} />
        <Row k="FIRST SEEN" v={`${fmtAgo(secsAgo(d.firstSeen))} ago`} />
        <Row k="OBSERVATIONS" v={d.observations.toLocaleString()} />
        <Row k="SOURCES" v={d.sources.join(', ') + ' · ML-DSA signed'} />
        <Row k="TRAIL" v={`${t.trail?.length ?? 0} reported position${t.trail?.length === 1 ? '' : 's'} since this page opened`} />
      </Card>
      <Card size="sm">
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 6 }}>CLASSIFICATION</div>
        {d.label ? (
          <>
            <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--grey-100)', fontWeight: 500 }}>{d.label}</div>
            <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', marginTop: 3 }}>Rule: {d.rationale}</div>
          </>
        ) : (
          <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>No rule in state/rules.json matched this track.</div>
        )}
      </Card>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-H-1" title="Multi-source fusion into one common operating picture" />
        <ProvenanceTag id="FR-H-2" title="Rule-based, explainable classification" />
      </div>
    </>
  );
}

/** The inspector with nothing selected: what is in the live picture right now. */
function LiveSummaryPanel({ picture, tracks, filter, setFilter, select }: {
  picture: HeimdallPicture;
  tracks: HeimdallTrack[];
  filter: string | null;
  setFilter: (f: string | null) => void;
  select: (id: string) => void;
}) {
  const airborne = tracks.filter((t) => !t.detail?.onGround);
  const byClass = new Map<string, number>();
  for (const t of tracks) byClass.set(t.cls, (byClass.get(t.cls) ?? 0) + 1);
  const classes = [...byClass.entries()].sort((a, b) => b[1] - a[1]);
  const fastest = [...airborne].sort((a, b) => b.spd - a.spd).slice(0, 5);
  const highest = [...airborne].filter((t) => t.altFt !== undefined).sort((a, b) => (b.altFt ?? 0) - (a.altFt ?? 0)).slice(0, 5);
  const squawks = tracks.filter((t) => t.detail?.squawk && SQUAWKS[t.detail.squawk]);
  const newest = Math.max(0, ...tracks.map((t) => t.detail?.lastSeen ?? 0));
  const fresh = tracks.filter((t) => t.detail && secsAgo(t.detail.lastSeen) <= 90).length;
  const meanAlt = airborne.length ? Math.round(airborne.reduce((s, t) => s + (t.altFt ?? 0), 0) / airborne.length) : 0;
  const top = (list: HeimdallTrack[], value: (t: HeimdallTrack) => string) => list.map((t) => (
    <button key={t.id} onClick={() => select(t.id)} style={{ all: 'unset', cursor: 'pointer', display: 'flex', gap: 8, padding: '3px 0', fontFamily: 'var(--font-mono)', fontSize: 10, width: '100%' }}>
      <span style={{ color: 'var(--text-body)', flex: 1 }}>{t.id}</span>
      <span style={{ color: 'var(--text-faint)' }}>{t.cls}</span>
      <span style={{ color: 'var(--accent-hover)', width: 64, textAlign: 'right' }}>{value(t)}</span>
    </button>
  ));
  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="radar" size={17} color="var(--accent)" />
        <h2 style={{ fontSize: 'var(--fs-h4)' }}>Live picture</h2>
        <Badge tone="secure">LIVE</Badge>
      </div>
      <div style={{ ...mono, color: 'var(--text-faint)' }}>
        READ {fmtAgo(Math.round((Date.now() - (picture.fetchedAt ?? Date.now())) / 1000))} AGO · NEWEST REPORT {newest ? fmtAgo(secsAgo(newest)) : '—'} AGO
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 12 }}>
          <StatBlock value={airborne.length.toLocaleString()} label="AIRBORNE" size="sm" />
          <StatBlock value={(tracks.length - airborne.length).toLocaleString()} label="ON GROUND" size="sm" />
          <StatBlock value={meanAlt ? `FL${Math.round(meanAlt / 100)}` : '—'} label="MEAN ALT" size="sm" />
        </div>
        <div style={{ marginTop: 10 }}>
          <MeterBar label="Reported in last 90 s" value={tracks.length ? Math.round((fresh / tracks.length) * 100) : 0} width={110} warnAt={60} invert />
        </div>
      </Card>

      {squawks.length > 0 && (
        <div style={{ padding: '9px 12px', border: '1px solid var(--warn)', background: 'var(--warn-tint)', borderRadius: 'var(--radius-sm)' }}>
          {squawks.map((t) => (
            <button key={t.id} onClick={() => select(t.id)} style={{ all: 'unset', cursor: 'pointer', display: 'block', ...mono, color: 'var(--warn)' }}>
              ⚠ {t.id} · SQUAWK {t.detail!.squawk} · {SQUAWKS[t.detail!.squawk!]}
            </button>
          ))}
        </div>
      )}

      <div>
        <SectionLabel>CLASSIFICATION · CLICK TO FILTER</SectionLabel>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 5, marginTop: 8 }}>
          {classes.map(([cls, n]) => (
            <button key={cls} onClick={() => setFilter(filter === cls ? null : cls)} style={{ all: 'unset', cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 8, opacity: filter && filter !== cls ? 0.45 : 1 }}>
              <span style={{ ...mono, color: filter === cls ? 'var(--accent-hover)' : 'var(--text-body)', width: 130 }}>{cls}</span>
              <MeterBar value={(n / Math.max(1, tracks.length)) * 100} width={110} showValue={false} />
              <span style={{ ...mono, color: 'var(--text-muted)', marginLeft: 'auto' }}>{n}</span>
            </button>
          ))}
        </div>
      </div>

      <div>
        <SectionLabel>ALTITUDE BANDS</SectionLabel>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 5, marginTop: 8 }}>
          {BANDS.map((b) => {
            const n = tracks.filter((t) => bandOf(t).id === b.id).length;
            return (
              <div key={b.id} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <span style={{ width: 8, height: 8, borderRadius: '50%', background: b.color, flex: 'none' }} />
                <span style={{ ...mono, color: 'var(--text-body)', width: 122 }}>{b.label}</span>
                <span style={{ flex: 1, height: 5, background: 'var(--surface-inset)', borderRadius: 3, overflow: 'hidden' }}>
                  <span style={{ display: 'block', height: '100%', width: `${(n / Math.max(1, tracks.length)) * 100}%`, background: b.color }} />
                </span>
                <span style={{ ...mono, color: 'var(--text-muted)', width: 28, textAlign: 'right' }}>{n}</span>
              </div>
            );
          })}
        </div>
      </div>

      <div>
        <SectionLabel>FASTEST</SectionLabel>
        <div style={{ marginTop: 6 }}>{top(fastest, (t) => t.spd + ' KT')}</div>
      </div>
      <div>
        <SectionLabel>HIGHEST</SectionLabel>
        <div style={{ marginTop: 6 }}>{top(highest, (t) => t.alt)}</div>
      </div>

      <div style={{ ...mono, fontSize: 8, color: 'var(--text-faint)', lineHeight: 1.6 }}>
        POSITIONS FROM ADS-B (ADSB.LOL), SIGNED AND FUSED BY HEIMDALL. BETWEEN REPORTS EACH AIRBORNE TRACK IS MOVED ALONG ITS TRACK AT ITS GROUND SPEED (ESTIMATED). TRAILS SHOW REPORTED POSITIONS SEEN SINCE THIS PAGE OPENED.
      </div>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-H-1" title="Multi-source fusion into one common operating picture" />
        <ProvenanceTag id="FR-H-2" title="Rule-based, explainable classification" />
      </div>
    </>
  );
}

export function OperatingPictureScreen() {
  const gateway = useGateway<HeimdallGateway>();
  const { data: picture, reload } = useAsync<HeimdallPicture, HeimdallGateway>((c) => c.getPicture(), []);
  // Redraw every 2 s; the live client fetches from the gateway at most every 15 s and
  // moves tracks along their heading in between. [FR-H-1]
  React.useEffect(() => {
    const timer = window.setInterval(reload, 2000);
    return () => window.clearInterval(timer);
  }, [reload]);

  const layerDefs = picture?.layers ?? [];
  const allTracks: HeimdallTrack[] = picture?.tracks ?? [];
  /** Geographic picture (live): a basemap, trails and altitude colours replace the
   *  reference layout's illustrative rings and sensor footprint. */
  const region = picture?.region;
  const [filter, setFilter] = React.useState<string | null>(null);
  // Canvas height in px, for the live view's true-scale rings and scale bar.
  const canvasRef = React.useRef<HTMLDivElement>(null);
  const [canvasH, setCanvasH] = React.useState(0);
  React.useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setCanvasH(el.clientHeight));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const pxPerNm = region && canvasH ? canvasH / ((region.north - region.south) * 60) : 0;
  const tracks = filter ? allTracks.filter((t) => t.cls === filter) : allTracks;

  const [layers, setLayers] = React.useState<LayerState | null>(null);
  const [sel, setSel] = React.useState('E-0871');
  const [chosen, setChosen] = React.useState(1);
  const [hover, setHover] = React.useState<string | null>(null);
  const [gate, setGate] = React.useState<GateState>(null);
  const [busy, setBusy] = React.useState(false);

  // Seed the toggle state once the picture's layers resolve.
  React.useEffect(() => {
    if (layerDefs.length && !layers) {
      setLayers(layerDefs.reduce((acc, l) => ({ ...acc, [l.id]: l.active }), {} as LayerState));
    }
  }, [layerDefs, layers]);

  const layerOn = (id: HeimdallLayerId) => layers?.[id] !== false;
  const setLayer = (id: HeimdallLayerId, v: boolean) => setLayers((l) => ({ ...(l ?? ({} as LayerState)), [id]: v }));

  const e = tracks.find((t) => t.id === sel);
  const visible = tracks.filter((t) => layerOn(t.layer));
  const options = e?.options ?? [];

  const authorise = async () => {
    const wfId = WORKFLOW_FOR_RANK[chosen];
    if (!wfId) return;
    setBusy(true);
    try {
      const res = await gateway.authoriseWorkflow(wfId, `HG-${Date.now().toString(36)}`);
      setGate({ rank: chosen, state: 'authorised', auditId: res.auditId });
    } catch (err) {
      if (err instanceof HumanGateRequiredError) setGate({ rank: chosen, state: 'pending' });
    } finally {
      setBusy(false);
    }
  };

  const aside = region && picture ? (
    e?.detail ? (
      <LiveTrackPanel t={e} onClose={() => setSel('')} />
    ) : (
      <LiveSummaryPanel picture={picture} tracks={allTracks} filter={filter} setFilter={setFilter} select={(id) => { setSel(id); setGate(null); }} />
    )
  ) : e ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <Icon name="crosshair" size={17} color={e.track === 'anomalous' ? 'var(--warn)' : 'var(--accent)'} />
        <h2 style={{ fontSize: 'var(--fs-h4)', minWidth: 0 }}>{e.id}</h2>
        <Badge tone={e.track === 'anomalous' ? 'warn' : 'neutral'}>{e.cls}</Badge>
        <span style={{ marginLeft: 'auto' }}><MarkingChip level="confidential" compartment="OPS-N" /></span>
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={Math.round(e.conf * 100) + '%'} label="CLASSIFICATION CONF." size="sm" accent={e.conf < 0.7} />
          <StatBlock value={e.track === 'anomalous' ? 'ANOM' : 'STABLE'} label="TRACK STATE" size="sm" />
        </div>
      </Card>

      {options.length > 0 && (
        <div>
          <SectionLabel number={5}>RANKED OPTIONS · ADVISORY</SectionLabel>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginTop: 10 }}>
            {options.map((o) => (
              <Card
                key={o.rank}
                size="sm"
                interactive
                onClick={() => { setChosen(o.rank); setGate(null); }}
                style={{ borderColor: chosen === o.rank ? 'var(--accent)' : undefined }}
              >
                <div style={{ display: 'flex', gap: 9, alignItems: 'baseline' }}>
                  <span style={{ fontFamily: 'var(--font-display)', fontWeight: 800, fontSize: 15, color: chosen === o.rank ? 'var(--accent-hover)' : 'var(--text-faint)' }}>
                    {String(o.rank).padStart(2, '0')}
                  </span>
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--grey-100)', fontWeight: 500 }}>{o.title}</div>
                    <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', marginTop: 3, lineHeight: 1.5 }}>{o.why}</div>
                    <div style={{ display: 'flex', gap: 5, marginTop: 6, flexWrap: 'wrap' }}>
                      {o.cites.map((c, i) => (
                        <span key={i} style={{ fontFamily: 'var(--font-mono)', fontSize: 9, color: 'var(--accent-hover)', background: 'var(--surface-inset)', border: '1px solid var(--border)', borderRadius: 4, padding: '1px 6px' }}>{c}</span>
                      ))}
                    </div>
                  </div>
                </div>
              </Card>
            ))}
          </div>
        </div>
      )}

      {options.length > 0 && (
        WORKFLOW_FOR_RANK[chosen] ? (
          <HumanGate
            title={`Authorise option ${String(chosen).padStart(2, '0')} · ${e.id}`}
            detail="HUMAN AUTHORISATION REQUIRED · AI IS ADVISORY"
            state={gate?.state ?? 'pending'}
            auditId={gate?.auditId}
            onAuthorise={busy ? undefined : authorise}
            onReject={() => setGate({ rank: chosen, state: 'rejected' })}
          />
        ) : (
          <div style={{ padding: '12px 14px', background: 'var(--surface-inset)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--text-faint)' }}>
            HOLD · NO ACTION · PASSIVE FUSION CONTINUES
          </div>
        )
      )}

      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-H-4" title="AI-assisted, explainable, cited ranked options, advisory" />
        <ProvenanceTag id="FR-H-5" title="Human-gated action workflows with full audit lineage" />
        <ProvenanceTag id="NFR-AI-1" title="Every agentic action traceable, reversible or human-gated" />
      </div>
    </>
  ) : null;

  const focus = e;
  const focusRng = focus ? Math.round(Math.hypot(focus.x - 50, focus.y - 50) * 1.3) : 0;

  return (
    <ConsolePage
      active="cop"
      context="COMMON OPERATING PICTURE / SECTOR NORTH"
      asideWidth={360}
      mainStyle={{ padding: 0, gap: 0, position: 'relative' }}
      toolbar={
        <>
          <IconButton label="Alerts" active>
            <Icon name="bell" size={17} />
          </IconButton>
          <IconButton label="Settings">
            <Icon name="settings" size={17} />
          </IconButton>
        </>
      }
      navFooter={
        <div style={{ padding: '10px 10px 4px', display: 'flex', flexDirection: 'column', gap: 9 }}>
          <div className="mg-label" style={{ color: 'var(--text-faint)' }}>LAYERS</div>
          {layerDefs.map((l) => (
            <Switch key={l.id} checked={layerOn(l.id)} onChange={(v) => setLayer(l.id, v)} label={l.label} />
          ))}
        </div>
      }
      aside={aside}
    >
      {/* The blueprint grid and glow stay in both views; the live view adds the map. */}
      <div ref={canvasRef} className="mg-grid-bg" style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }} />
      <div style={{ position: 'absolute', inset: 0, background: 'radial-gradient(ellipse at 55% 45%, var(--accent-tint), transparent 60%)', pointerEvents: 'none' }} />
      {region && (
        <>
          <Basemap region={region} />
          <GraticuleLabels region={region} />
          <GeoRings region={region} height={canvasH} />
        </>
      )}

      {region && (
        <svg aria-hidden viewBox="0 0 100 100" preserveAspectRatio="none" style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none' }}>
          {tracks.filter((t) => layerOn(t.layer) && (t.trail?.length ?? 0) > 0).map((t) => {
            const pts = [...t.trail!, { x: t.x, y: t.y }].map((p) => `${p.x},${p.y}`).join(' ');
            const on = t.id === sel || t.id === hover;
            return (
              <polyline key={t.id} points={pts} fill="none" stroke={t.track === 'anomalous' ? 'var(--warn)' : bandOf(t).color}
                strokeOpacity={on ? 0.9 : 0.3} strokeWidth={on ? 2 : 1} vectorEffect="non-scaling-stroke" />
            );
          })}
        </svg>
      )}

      {/* range rings + scale + north arrow (reference layout only) */}
      {!region && <div style={{ position: 'absolute', inset: 0, overflow: 'hidden', pointerEvents: 'none' }}>
        {([[160, '20 NM'], [320, '40 NM'], [480, '60 NM']] as [number, string][]).map(([rr, lb], i) => (
          <React.Fragment key={i}>
            <div style={{ position: 'absolute', left: '50%', top: '50%', width: rr * 2, height: rr * 2, marginLeft: -rr, marginTop: -rr, border: '1px solid var(--divider)', borderRadius: '50%' }} />
            <span style={{ position: 'absolute', left: '50%', top: `calc(50% - ${rr}px)`, transform: 'translate(-50%, -50%)', fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.12em', color: 'var(--text-faint)', background: 'var(--bg-sunken)', padding: '0 5px' }}>{lb}</span>
          </React.Fragment>
        ))}
        <div style={{ position: 'absolute', top: 12, left: 14, display: 'flex', flexDirection: 'column', gap: 6 }}>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, letterSpacing: '0.14em', color: 'var(--text-muted)' }}>N ↑</span>
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <span style={{ width: 80, height: 0, borderTop: '2px solid var(--text-faint)', position: 'relative' }}>
              <span style={{ position: 'absolute', left: 0, top: -4, width: 2, height: 6, background: 'var(--text-faint)' }} />
              <span style={{ position: 'absolute', right: 0, top: -4, width: 2, height: 6, background: 'var(--text-faint)' }} />
            </span>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.12em', color: 'var(--text-faint)' }}>10 NM</span>
          </span>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.12em', color: 'var(--text-faint)' }}>VECTOR · 1 MIN PROJECTION</span>
        </div>
      </div>}

      {!region && layerOn('feeds') && (
        <>
          <div style={{ position: 'absolute', left: '12%', top: '8%', width: '55%', height: '62%', border: '1px dashed var(--accent-glow)', borderRadius: '50%', pointerEvents: 'none' }} />
          <span style={{ position: 'absolute', left: '39%', top: '8%', transform: 'translate(-50%, -50%)', fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.12em', color: 'var(--accent-hover)', background: 'var(--bg-sunken)', padding: '1px 6px', pointerEvents: 'none' }}>NORTH-2 · RADAR · 4 HZ · R 60 NM</span>
        </>
      )}

      {visible.map((en) => {
        const anomalous = en.track === 'anomalous';
        const selected = en.id === sel;
        const color = anomalous ? 'var(--warn)' : region ? bandOf(en).color : 'var(--accent)';
        const rad = (en.hdg * Math.PI) / 180;
        // Live: the vector is where the track will be in 3 minutes (speed in NM/min × 3).
        const vlen = region ? (en.detail?.onGround ? 0 : (en.spd / 20) * pxPerNm) : Math.min(46, 12 + en.spd / 14);
        const size = region ? 7 : 14;
        // With hundreds of live tracks, label only the ones that matter right now.
        const labelled = !region || selected || anomalous || en.id === hover || visible.length <= 40;
        return (
          <button
            key={en.id}
            onClick={() => { setSel(en.id); setGate(null); }}
            onMouseEnter={() => setHover(en.id)}
            onMouseLeave={() => setHover(null)}
            onFocus={() => setHover(en.id)}
            onBlur={() => setHover(null)}
            aria-label={`${en.id} · ${en.cls} · heading ${en.hdg} degrees`}
            style={{ all: 'unset', position: 'absolute', left: en.x + '%', top: en.y + '%', transform: 'translate(-50%,-50%)', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 5, cursor: 'pointer', zIndex: selected ? 3 : undefined, transition: region ? 'left 2s linear, top 2s linear' : undefined }}
          >
            <span style={{ position: 'relative', width: size, height: size }}>
              {!region && [1, 2, 3].map((k) => (
                <span
                  key={k}
                  style={{
                    position: 'absolute', left: '50%', top: '50%', width: 4 - k * 0.7, height: 4 - k * 0.7, borderRadius: '50%',
                    background: color, opacity: 0.45 - k * 0.11,
                    transform: `translate(-50%,-50%) translate(${-Math.sin(rad) * k * 10}px, ${Math.cos(rad) * k * 10}px)`,
                  }}
                />
              ))}
              <span
                style={{
                  position: 'absolute', left: '50%', bottom: '50%', width: 2, height: vlen,
                  background: `linear-gradient(to top, ${color}, transparent)`,
                  transformOrigin: 'bottom center', transform: `translateX(-50%) rotate(${en.hdg}deg)`,
                  opacity: 0.9, pointerEvents: 'none',
                }}
              />
              <span
                style={{
                  position: 'absolute', inset: 0, borderRadius: en.layer === 'maritime' ? 2 : '50%',
                  background: color,
                  boxShadow: selected
                    ? `0 0 0 5px ${anomalous ? 'var(--warn-tint)' : 'var(--accent-tint)'}, 0 0 18px -2px ${anomalous ? 'var(--warn)' : 'var(--accent-glow)'}`
                    : region ? `0 0 8px -1px ${color}` : 'none',
                  outline: selected ? `1px solid ${color}` : 'none', outlineOffset: 3,
                }}
              />
            </span>
            {labelled && (
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.08em', color: selected ? 'var(--grey-100)' : 'var(--text-faint)', whiteSpace: 'nowrap', position: region ? 'absolute' : undefined, top: region ? size + 3 : undefined }}>
                {en.id}{anomalous ? ' ⚠' : ''}{region ? ` · ${en.alt}` : ` · ${en.hdg}°`}
              </span>
            )}
          </button>
        );
      })}

      {(() => {
        const t = tracks.find((x) => x.id === hover);
        if (!t || !layerOn(t.layer)) return null;
        const rng = Math.round(Math.hypot(t.x - 50, t.y - 50) * 1.3);
        const flipY = t.y < 36;
        const d = t.detail;
        const rows: [string, string][] = region && d ? [
          ['TRK / SPD', String(Math.round(t.hdg)).padStart(3, '0') + '° · ' + t.spd + ' KT'],
          ['ALT', t.alt],
          ['SQUAWK', d.squawk ?? '—'],
          ['POSITION', `${d.lat.toFixed(2)}, ${d.lon.toFixed(2)}`],
          ['REPORTED', `${fmtAgo(secsAgo(d.lastSeen))} ago${d.estimatedSecs > 0 ? ' · moved on (est.)' : ''}`],
        ] : [
          ['HDG / SPD', String(t.hdg).padStart(3, '0') + '° · ' + t.spd + ' KT'],
          ['ALT', t.alt],
          ['RNG', rng + ' NM FROM SITE'],
          ['CAP', CAPABILITY[t.cls] ?? '—'],
          ['TRACK', t.track.toUpperCase() + ' · ' + t.feeds + ' FEEDS'],
        ];
        return (
          <div
            role="tooltip"
            style={{
              position: 'absolute', left: `clamp(110px, ${t.x}%, calc(100% - 110px))`, top: t.y + '%',
              transform: `translate(-50%, ${flipY ? '30px' : 'calc(-100% - 26px)'})`,
              zIndex: 5, pointerEvents: 'none', minWidth: 200,
              background: 'var(--surface-card)', border: '1px solid ' + (t.track === 'anomalous' ? 'var(--warn)' : 'var(--border-strong)'),
              borderRadius: 'var(--radius-sm)', padding: '9px 11px', boxShadow: 'var(--shadow-md)',
            }}
          >
            <div style={{ display: 'flex', gap: 8, alignItems: 'baseline', marginBottom: 6 }}>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, fontWeight: 600, color: 'var(--text-heading)' }}>{t.id}</span>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: 'var(--text-faint)' }}>{t.cls}</span>
              <span style={{ marginLeft: 'auto', fontFamily: 'var(--font-mono)', fontSize: 10, color: t.conf < 0.7 ? 'var(--warn)' : 'var(--secure)' }}>{Math.round(t.conf * 100)}%</span>
            </div>
            {rows.map(([k, v]) => (
              <div key={k} style={{ display: 'flex', gap: 10, padding: '2px 0', fontFamily: 'var(--font-mono)', fontSize: 9 }}>
                <span style={{ color: 'var(--text-faint)', width: 62, flex: 'none', letterSpacing: '0.08em' }}>{k}</span>
                <span style={{ color: 'var(--text-body)' }}>{v}</span>
              </div>
            ))}
            {t.id === sel && <div style={{ marginTop: 5, fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.14em', color: 'var(--accent-hover)' }}>FOCUSED · {region ? 'DETAIL' : 'OPTIONS'} IN INSPECTOR →</div>}
          </div>
        );
      })()}

      {focus && !region && (
        <div style={{ position: 'absolute', top: 12, right: 14, pointerEvents: 'none', textAlign: 'right', fontFamily: 'var(--font-mono)', display: 'flex', flexDirection: 'column', gap: 3 }}>
          <span style={{ fontSize: 8, letterSpacing: '0.14em', color: 'var(--accent-hover)' }}>FOCUS · {focus.id}</span>
          <span style={{ fontSize: 9, color: 'var(--text-muted)' }}>{String(focus.hdg).padStart(3, '0')}° · {focus.spd} KT · {focus.alt}</span>
          <span style={{ fontSize: 9, color: 'var(--text-faint)' }}>{focusRng} NM · {focus.feeds} FEEDS · {focus.track.toUpperCase()}</span>
        </div>
      )}

      <div style={{ position: 'absolute', left: 12, right: 12, bottom: 10, display: 'flex', alignItems: 'flex-end', gap: '8px 18px', flexWrap: 'wrap', zIndex: 4, background: 'color-mix(in srgb, var(--bg-sunken) 80%, transparent)', backdropFilter: 'var(--blur-panel)', WebkitBackdropFilter: 'var(--blur-panel)', border: '1px solid var(--divider)', borderRadius: 'var(--radius-sm)', padding: '8px 12px', width: 'fit-content', maxWidth: 'calc(100% - 24px)' }}>
        {/* Counted from the picture the gateway serves, not fixed figures. [FR-H-1] [FR-H-2] */}
        <StatBlock value={(picture?.entities.length ?? 0).toLocaleString()} label="TRACKED ENTITIES" size="sm" />
        <StatBlock value={visible.length.toLocaleString()} label="ON CANVAS" size="sm" />
        <StatBlock value={String(tracks.filter((t) => t.track === 'anomalous').length)} label="ANOMALIES" size="sm" accent />
        {region && <StatBlock value={String(allTracks.filter((t) => t.detail?.label === 'fast-mover').length)} label="FAST MOVERS" size="sm" />}
        {region && <StatBlock value={String(allTracks.filter((t) => t.detail?.label === 'rotorcraft').length)} label="ROTORCRAFT" size="sm" />}
        {region && (
          <span style={{ display: 'flex', gap: 10, flexWrap: 'wrap', alignItems: 'center' }}>
            {BANDS.map((b) => (
              <span key={b.id} style={{ display: 'inline-flex', alignItems: 'center', gap: 5, ...mono, fontSize: 8, color: 'var(--text-faint)' }}>
                <span style={{ width: 7, height: 7, borderRadius: '50%', background: b.color }} />{b.label}
              </span>
            ))}
          </span>
        )}
        {filter && (
          <button onClick={() => setFilter(null)} style={{ all: 'unset', cursor: 'pointer', ...mono, color: 'var(--accent-hover)' }}>FILTER: {filter} ✕</button>
        )}
        <span style={{ marginLeft: 'auto', fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.14em', color: 'var(--text-faint)', whiteSpace: 'nowrap' }}>
          {region ? 'MAP: NATURAL EARTH 1:50M · LINEAR LAT/LON' : 'REFERENCE LAYOUT · POSITIONS NOT TO MAP SCALE'}
        </span>
      </div>
    </ConsolePage>
  );
}
