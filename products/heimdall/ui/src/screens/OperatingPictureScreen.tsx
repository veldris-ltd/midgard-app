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
import { HumanGateRequiredError, UK_AND_IRELAND } from '@midgard/sdk';
import type { HeimdallAlert, HeimdallGateway, HeimdallHistoryPoint, HeimdallLayerId, HeimdallPicture, HeimdallRegion, HeimdallTrack, HeimdallTrackDetail, HeimdallCameraAnalysis, HeimdallCameraPlayback } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { BORDER_PATH, LAND_PATH } from '../app/basemap';
import { LAKES, MOTORWAYS, RAILWAYS, RIVERS, ROADS } from '../app/hydro';
import { AREA_LEGEND, BANDS, KIND_LEGEND, areaColour, bandOf, colourOf, inForce, isFixedKind, shapeOf, warningText } from '../app/trackStyle';
import { useStatus } from '../app/status';
import { AlertsBell, AlertsDrawer, agoText } from '../app/alerts';
import { radarLabel, useRadar, RADAR_BOX } from '../app/radar';
import { trafficLabel, useTraffic, TRAFFIC_COLOURS, type TrafficLines } from '../app/traffic';
import { OFFLINE } from '../app/offline';
import { useWatchlist, type Watchlist } from '../app/watchlist';
import { AreaLayer, ToolLayer } from '../app/MapOverlays2D';
import type { LatLon } from '../app/geoMath';
import type { GlobeArea, PresetId } from '../app/globe/GlobeView';
import { useFlights, type Flight, type KnownAirport } from '../app/flights';
import { REGION_FOR_PRESET, REGION_LIST, WORLD_ID, inBox as inRegionBox, readFocus, readRegions, regionAt, saveFocus, saveRegions, type Region } from '../app/regions';
import { keyOf, replayTracks, stepFor, trailOf } from '../app/history';
import { useEntityPath, useHistory } from '../app/useHistory';
import { Timeline, useReplayClock } from '../app/Timeline';
import type { GlobeTrail, InfraLineGroup } from '../app/globe/GlobeView';
import { AreaCamerasCard } from '../app/AreaCameras';
import { CameraTrend } from '../app/CameraTrend';
import { AlertsPanel } from '../app/AlertsPanel';
import { GulfWatchPanel, gulfWatchName, isGulfWatch } from '../app/GulfWatchPanel';
import { AdviceCard } from '../app/AdviceCard';
import { AskCard } from '../app/AskCard';
import { utilityLineGroups, utilityLineKey } from '../app/utilityLines';
import { ALERT_COLOURS, ALERT_PULSE_CSS, WARNING_LEVEL_COLOURS, alertIconSvg, alertText, markStyle, type AlertMark } from '../app/alertMarks';

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

/* Altitude bands and kind colours live in app/trackStyle, shared with the 3D globe. */

/** The emergency squawks: hijack, radio failure, general emergency. */
const SQUAWKS: Record<string, string> = { '7500': 'UNLAWFUL INTERFERENCE', '7600': 'RADIO FAILURE', '7700': 'EMERGENCY' };

const secsAgo = (unix: number) => Math.max(0, Math.round(Date.now() / 1000 - unix));
const fmtAgo = (s: number) => (s < 120 ? `${s}s` : `${Math.round(s / 60)}m`);
const mono: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em' };

/** Faint land, borders and a 5° graticule behind the tracks, in the picture's region. */
/** Rivers and lakes as SVG path data in this map's lon / -lat space. */
const HYDRO_2D = (() => {
  const d = (text: string, close: boolean) => text.split('|').map((ln) => 'M' + ln.split(' ').map((p) => { const [la, lo] = p.split(','); return `${lo} ${-Number(la)}`; }).join(' ') + (close ? 'Z' : '')).join('');
  return { rivers: d(RIVERS, false), lakes: d(LAKES, true), motorways: d(MOTORWAYS, false), roads: d(ROADS, false), rail: d(RAILWAYS, false) };
})();

/** The map style the 3D view stores (tactical unless set otherwise). */
const mapStyle2d = () => { try { const v = window.localStorage.getItem('heimdall.globe.imagery'); return v === 'satellite' || v === 'dark' ? v : 'tactical'; } catch { return 'tactical'; } };

function Basemap({ region, radarUrl, traffic, utilityLines }: { region: HeimdallRegion; radarUrl?: string; traffic?: TrafficLines; utilityLines?: InfraLineGroup[] }) {
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
      {/* NASA Blue Marble (public domain), the same bundled image the 3D terrain uses; its
          box is -12..4 E, 48..62 N, linear in lat/lon like this map. Off when the 3D
          view's MAP chip is set to DARK. [FR-P3-3] */}
      {mapStyle2d() !== 'dark' && (
        <image href={`${import.meta.env.BASE_URL}imagery/${mapStyle2d() === 'tactical' ? 'uk-ie-tactical.jpg' : 'uk-ie-2560.jpg'}`} x={-12} y={-62} width={16} height={14} preserveAspectRatio="none" opacity={0.9} />
      )}
      {/* RainViewer radar, reprojected to this map's lat/lon box, under the tracks. [FR-H-1] */}
      {radarUrl && (
        <image href={radarUrl} x={RADAR_BOX.west} y={-RADAR_BOX.north} width={RADAR_BOX.east - RADAR_BOX.west} height={RADAR_BOX.north - RADAR_BOX.south} preserveAspectRatio="none" opacity={0.8} />
      )}
      <path d={LAND_PATH} fill="var(--text-faint)" fillOpacity={0.07} stroke="var(--text-faint)" strokeOpacity={0.35} strokeWidth={1} vectorEffect="non-scaling-stroke" />
      <path d={BORDER_PATH} fill="none" stroke="var(--text-faint)" strokeOpacity={0.25} strokeWidth={1} strokeDasharray="3 3" vectorEffect="non-scaling-stroke" />
      {/* Main roads (Natural Earth 1:10m), then rivers and lakes in water blue. [FR-P3-3] */}
      {/* Road-atlas styling: primary roads green and motorways blue over dark casings;
          railways black with white dashes. */}
      <path d={HYDRO_2D.roads} fill="none" stroke="#0f1a12" strokeOpacity={0.8} strokeWidth={3} vectorEffect="non-scaling-stroke" />
      <path d={HYDRO_2D.roads} fill="none" stroke="#3fb35c" strokeWidth={1.6} vectorEffect="non-scaling-stroke" />
      <path d={HYDRO_2D.motorways} fill="none" stroke="#0b1630" strokeOpacity={0.85} strokeWidth={4.2} vectorEffect="non-scaling-stroke" />
      <path d={HYDRO_2D.motorways} fill="none" stroke="#3b82e0" strokeWidth={2.6} vectorEffect="non-scaling-stroke" />
      <path d={HYDRO_2D.rail} fill="none" stroke="#111" strokeOpacity={0.9} strokeWidth={3} vectorEffect="non-scaling-stroke" />
      <path d={HYDRO_2D.rail} fill="none" stroke="#f2f2f2" strokeWidth={1.4} strokeDasharray="5 5" vectorEffect="non-scaling-stroke" />
      {/* Utility networks: power lines by voltage, gas pipelines, cables, interconnectors. [FR-H-1] */}
      {utilityLines?.map((g) => (
        <path key={g.key} d={g.lines.map((l) => 'M' + l.map(([la, lo]) => `${lo} ${-la}`).join(' ')).join('')} fill="none" stroke={g.colour}
          strokeOpacity={g.opacity ?? 0.85} strokeWidth={g.px} strokeDasharray={g.dashed ? '5 3' : undefined} vectorEffect="non-scaling-stroke" />
      ))}
      {/* Live traffic (TomTom), crisp lines over the roads: light, moderate, heavy, closed. [FR-H-1] */}
      {traffic && (['light', 'moderate', 'closed', 'heavy'] as const).map((cls) => (
        <path key={cls} d={traffic[cls].map((l) => 'M' + l.map(([la, lo]) => `${lo} ${-la}`).join(' ')).join('')} fill="none" stroke={TRAFFIC_COLOURS[cls]} strokeWidth={cls === 'heavy' ? 2.4 : 1.8} vectorEffect="non-scaling-stroke" />
      ))}
      <path d={HYDRO_2D.rivers} fill="none" stroke="#6eb9f5" strokeOpacity={0.95} strokeWidth={1.8} vectorEffect="non-scaling-stroke" />
      <path d={HYDRO_2D.lakes} fill="#5aa9e6" fillOpacity={0.35} stroke="#5aa9e6" strokeOpacity={0.85} strokeWidth={1} vectorEffect="non-scaling-stroke" />
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

const utc = (unix: number) => new Date(unix * 1000).toISOString().slice(11, 19);

/** The inspector for one live track: what the feed reported and how Heimdall classed it.
 *  Aircraft show their transponder fields; other kinds (balloons, satellites, earthquakes,
 *  flood warnings) the facts their feed reports. In replay the position is the recorded
 *  one at the replay time. [FR-H-1] [FR-P3-7] */
/** The recorded flight of one track over the history window: when Heimdall first and
 *  last recorded it, how far it went, and its altitude against time. [FR-H-1] */
function FlightHistory({ points, replayAt }: { points: HeimdallHistoryPoint[]; replayAt: number | null }) {
  const n = points.length;
  const t0 = points[0][0], t1 = points[n - 1][0];
  let nm = 0;
  for (let i = 1; i < n; i++) {
    const [, la1, lo1] = points[i - 1], [, la2, lo2] = points[i];
    const p1 = la1 * Math.PI / 180, p2 = la2 * Math.PI / 180, dl = (lo2 - lo1) * Math.PI / 180;
    const h = Math.sin((p2 - p1) / 2) ** 2 + Math.cos(p1) * Math.cos(p2) * Math.sin(dl / 2) ** 2;
    nm += 2 * 3440.065 * Math.asin(Math.min(1, Math.sqrt(h)));
  }
  const alts = points.map((p) => p[3]).filter((a): a is number => a !== null);
  const top = Math.max(1, ...alts);
  const W = 300, H = 64;
  const x = (tt: number) => (t1 > t0 ? ((tt - t0) / (t1 - t0)) * W : 0);
  const y = (a: number | null) => H - 4 - ((a ?? 0) / top) * (H - 10);
  const line = points.map((p) => `${x(p[0]).toFixed(1)},${y(p[3]).toFixed(1)}`).join(' ');
  const mins = Math.round((t1 - t0) / 60);
  return (
    <Card size="sm">
      <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 6 }}>FLIGHT HISTORY · RECORDED BY HEIMDALL</div>
      <Row k="RECORDED" v={`${utc(t0)}–${utc(t1)} UTC · ${mins >= 60 ? `${Math.floor(mins / 60)} h ${mins % 60} min` : `${mins} min`}`} />
      <Row k="POSITIONS" v={n.toLocaleString()} />
      <Row k="DISTANCE" v={`${Math.round(nm).toLocaleString()} NM along the recorded path`} />
      {alts.length > 0 && <Row k="ALTITUDE" v={`${Math.round(Math.min(...alts) * 3.28084).toLocaleString()}–${Math.round(Math.max(...alts) * 3.28084).toLocaleString()} ft`} />}
      {alts.length > 1 && (
        <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" style={{ width: '100%', height: H, marginTop: 8, display: 'block' }} aria-label="Altitude against time">
          <polygon points={`0,${H} ${line} ${W},${H}`} fill="var(--accent)" fillOpacity={0.12} />
          <polyline points={line} fill="none" stroke="var(--accent-hover)" strokeWidth={1.5} vectorEffect="non-scaling-stroke" />
          {replayAt !== null && replayAt >= t0 && replayAt <= t1 && (
            <line x1={x(replayAt)} x2={x(replayAt)} y1={0} y2={H} stroke="var(--warn)" strokeWidth={1} vectorEffect="non-scaling-stroke" />
          )}
        </svg>
      )}
      <div style={{ ...mono, fontSize: 8, color: 'var(--text-faint)', marginTop: 4 }}>ALTITUDE AGAINST TIME · THE PATH IS DRAWN ON THE MAP AND GLOBE</div>
    </Card>
  );
}

/** A camera's live view: the still image (reloaded every 30 s, as its source refreshes
 *  it), with its short clip or live stream on demand. Images and video come straight from
 *  the camera's source, so they need the internet; in the offline build (the Mac app) they
 *  are the recording's stored copies, app-relative `/media/…` URLs served by the app.
 *  [FR-H-1] */
function CameraCard({ camera, name }: { camera: NonNullable<HeimdallTrackDetail['camera']>; name: string }) {
  const gateway = useGateway<HeimdallGateway>();
  const [tick, setTick] = React.useState(() => Date.now());
  const [mode, setMode] = React.useState<'image' | 'clip' | 'stream'>('image');
  const [failed, setFailed] = React.useState(false);
  const [showBoxes, setShowBoxes] = React.useState(true);
  const [expanded, setExpanded] = React.useState(false);
  const [clipEl, setClipEl] = React.useState<HTMLVideoElement | null>(null);
  // Analysis runs only when asked (ANALYSE); the answer shows at once, and the picture's
  // copy of it (shared with other users) takes over when it is newer. [FR-P3-5]
  const [asked, setAsked] = React.useState<{ status: 'idle' } | { status: 'running' } | { status: 'done'; ai: HeimdallCameraAnalysis } | { status: 'failed'; reason: string }>({ status: 'idle' });
  const mine = asked.status === 'done' ? asked.ai : undefined;
  const ai = mine && (!camera.analysis || mine.at >= camera.analysis.at) ? mine : camera.analysis;
  React.useEffect(() => {
    setMode('image');
    setFailed(false);
    setAsked({ status: 'idle' });
    const timer = window.setInterval(() => setTick(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, [camera.id, camera.imageUrl, camera.streamUrl]);
  React.useEffect(() => {
    if (!expanded) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') setExpanded(false); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [expanded]);
  const analyse = () => {
    if (!gateway.analyseCamera || asked.status === 'running') return;
    const id = camera.id;
    setAsked({ status: 'running' });
    gateway.analyseCamera(id).then(
      (res) => setAsked((s) => (camera.id === id ? { status: 'done', ai: res } : s)),
      (err) => setAsked({ status: 'failed', reason: err instanceof Error ? err.message : String(err) }),
    );
  };
  const bust = (u: string) => `${u}${u.includes('?') ? '&' : '?'}t=${Math.floor(tick / 30_000)}`;
  const mjpeg = camera.streamType === 'mjpeg';
  // 'fill', not 'cover': the detection overlay is in the image's own 0-1 coordinates.
  const box: React.CSSProperties = expanded
    ? { width: '100%', maxWidth: 'calc(80vh * 4 / 3)', aspectRatio: '4 / 3', objectFit: 'fill', display: 'block', background: '#000', borderRadius: 'var(--radius-sm)' }
    : { width: '100%', aspectRatio: '4 / 3', objectFit: 'fill', display: 'block', background: '#000', borderRadius: 'var(--radius-sm)' };
  const media = (
    <>
      {failed && <div style={{ ...mono, color: 'var(--warn)', padding: '24px 0', textAlign: 'center' }}>CAMERA UNAVAILABLE</div>}
      {!failed && mode === 'image' && camera.imageUrl && (
        <div style={{ position: 'relative' }}>
          <img src={bust(camera.imageUrl)} alt={`Latest image from ${name}`} style={box} onError={() => setFailed(true)} />
          {ai && showBoxes && <DetectionOverlay boxes={ai.boxes} />}
        </div>
      )}
      {!failed && mode === 'clip' && camera.videoUrl && (
        <div style={{ position: 'relative' }}>
          <video key={camera.videoUrl} ref={setClipEl} src={bust(camera.videoUrl)} style={box} autoPlay muted loop playsInline controls onError={() => setFailed(true)} />
          {ai && showBoxes && (ai.playback && clipEl
            ? <PlaybackOverlay playback={ai.playback} video={clipEl} />
            : <DetectionOverlay boxes={ai.boxes} />)}
        </div>
      )}
      {!failed && mode === 'stream' && camera.streamUrl && (mjpeg
        ? <img src={camera.streamUrl} alt={`Live stream from ${name}`} style={box} onError={() => setFailed(true)} />
        : <video key={camera.streamUrl} src={camera.streamUrl} style={box} autoPlay muted playsInline controls onError={() => setFailed(true)} />)}
    </>
  );
  const buttons = (
    <span style={{ marginLeft: 'auto', display: 'flex', gap: 6 }}>
      {camera.imageUrl && <button type="button" onClick={() => setMode('image')} aria-pressed={mode === 'image'} style={camBtn(mode === 'image')}>IMAGE</button>}
      {camera.videoUrl && <button type="button" onClick={() => setMode('clip')} aria-pressed={mode === 'clip'} style={camBtn(mode === 'clip')}>CLIP</button>}
      {camera.streamUrl && <button type="button" onClick={() => setMode('stream')} aria-pressed={mode === 'stream'} style={camBtn(mode === 'stream')}>LIVE</button>}
      <button type="button" onClick={() => setExpanded(!expanded)} aria-pressed={expanded} style={camBtn(expanded)}>{expanded ? 'CLOSE' : 'EXPAND'}</button>
    </span>
  );
  const label = <span className="mg-label" style={{ color: 'var(--text-faint)' }}>CAMERA · {mode === 'image' ? 'LATEST IMAGE' : mode === 'clip' ? 'CLIP' : 'LIVE'}</span>;
  const analysis = (
    <AnalysisPanel ai={ai} showBoxes={showBoxes} setShowBoxes={setShowBoxes}
      canAnalyse={!!gateway.analyseCamera} running={asked.status === 'running'}
      failure={asked.status === 'failed' ? asked.reason : null} analyse={analyse} />
  );
  const trend = <CameraTrend gateway={gateway} cameraId={camera.id} refresh={mine?.at ?? 0} />;
  return (
    <Card size="sm">
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>{label}{buttons}</div>
      {expanded
        ? <div style={{ ...mono, fontSize: 9, color: 'var(--text-faint)', padding: '12px 0', textAlign: 'center' }}>SHOWING IN THE LARGE VIEW</div>
        : media}
      {analysis}
      {!expanded && trend}
      <div style={{ ...mono, fontSize: 8, color: 'var(--text-faint)', marginTop: 6 }}>
        {OFFLINE ? (camera.videoUrl ? 'STORED IMAGE AND CLIP FROM THE RECORDING' : 'STORED IMAGE FROM THE RECORDING') : 'IMAGE RELOADS EVERY 30 S'} · {camera.streamType === 'hls' ? 'HLS PLAYS IN SAFARI; OTHER BROWSERS NEED AN MP4 OR MJPEG URL' : 'MEDIA FROM THE CAMERA OPERATOR'}
      </div>
      {expanded && (
        <div role="dialog" aria-label={`Camera ${name}`} onClick={() => setExpanded(false)}
          style={{ position: 'fixed', inset: 0, zIndex: 1000, background: 'rgba(4,8,12,0.82)', display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 16 }}>
          <div onClick={(e) => e.stopPropagation()}
            style={{ background: 'var(--surface-raised, #0d141b)', border: '1px solid var(--divider)', borderRadius: 'var(--radius-md, 8px)', padding: 12, width: '96vw', maxHeight: '96vh', overflow: 'auto', boxSizing: 'border-box' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
              {label}<span style={{ ...mono, fontSize: 10, color: 'var(--text-body)' }}>{name}</span>{buttons}
            </div>
            {/* The analysis sits beside the picture on a wide screen, below it on a narrow one. */}
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 16, alignItems: 'flex-start' }}>
              <div style={{ flex: '1 1 480px', minWidth: 0, display: 'flex', justifyContent: 'center' }}><div style={{ width: '100%', maxWidth: 'calc(80vh * 4 / 3)' }}>{media}</div></div>
              <div style={{ flex: '0 1 320px', minWidth: 260 }}>{analysis}{trend}</div>
            </div>
          </div>
        </div>
      )}
    </Card>
  );
}
/** Detection colours by class: vehicles amber, people cyan, the rest violet. */
const DET_COLOUR: Record<string, string> = { car: '#ffb020', truck: '#ff8a20', bus: '#ffd040', motorcycle: '#ffc070', bicycle: '#7fffb0', person: '#3fe0ff' };

/** The analysed clip's tracks played in step with the video: each object's box at the
 *  video's current time (interpolated between the sampled frames), its track number, and
 *  a short trail of where it has been. Beyond the analysed part of the clip nothing shows.
 *  The boxes follow the clip that was analysed; if the camera has published a newer clip
 *  since, they no longer line up (RE-ANALYSE). [FR-P3-5] */
function PlaybackOverlay({ playback, video }: { playback: HeimdallCameraPlayback; video: HTMLVideoElement }) {
  const [frame, setFrame] = React.useState(0);
  React.useEffect(() => {
    let raf = 0;
    const step = () => {
      const f = Math.round(video.currentTime * playback.fps * 20) / 20;
      setFrame((old) => (old === f ? old : f));
      raf = window.requestAnimationFrame(step);
    };
    raf = window.requestAnimationFrame(step);
    return () => window.cancelAnimationFrame(raf);
  }, [video, playback]);
  const TRAIL = 2 * playback.fps; // two seconds of trail
  return (
    <svg viewBox="0 0 100 75" preserveAspectRatio="none" aria-hidden
      style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none' }}>
      {playback.tracks.map((t) => {
        const first = t.obs[0][0], last = t.obs[t.obs.length - 1][0];
        if (frame < first || frame > last + 0.5) return null;
        // The observations either side of now, and the box between them.
        let i = 0;
        while (i < t.obs.length - 1 && t.obs[i + 1][0] <= frame) i++;
        const a = t.obs[i], b = t.obs[Math.min(i + 1, t.obs.length - 1)];
        const k = b[0] > a[0] ? Math.min(1, Math.max(0, (frame - a[0]) / (b[0] - a[0]))) : 0;
        const at = (j: number) => a[j] + (b[j] - a[j]) * k;
        const x = at(1) * 100, y = at(2) * 75, w = at(3) * 100, h = at(4) * 75;
        const c = t.flag ? FLAG_COLOUR : DET_COLOUR[t.cls] ?? '#b48cff';
        const trail = t.obs.filter((o) => o[0] <= frame && o[0] >= frame - TRAIL)
          .map((o) => `${(o[1] + o[3] / 2) * 100},${(o[2] + o[4] / 2) * 75}`);
        trail.push(`${x + w / 2},${y + h / 2}`);
        return (
          <g key={t.id}>
            {t.moving && trail.length > 1 && <polyline points={trail.join(' ')} fill="none" stroke={c} strokeWidth={1.2} strokeOpacity={0.7} vectorEffect="non-scaling-stroke" />}
            <rect x={x} y={y} width={w} height={h} fill="none" stroke={c} strokeWidth={t.moving ? 1.4 : 0.8} strokeDasharray={t.moving ? undefined : '2 1.5'} vectorEffect="non-scaling-stroke" />
            <text x={x + 0.4} y={Math.max(2.4, y - 0.6)} fontSize={2.4} fill={c} style={{ fontFamily: 'var(--font-mono)' }}>{t.cls.toUpperCase()} {t.id}{t.flag ? ` · ${FLAG_TEXT[t.flag]}` : ''}</text>
          </g>
        );
      })}
    </svg>
  );
}

/** Flagged objects (incidents) are drawn in red with what was seen. */
const FLAG_COLOUR = '#ff3b30';
const FLAG_TEXT: Record<NonNullable<HeimdallCameraPlayback['tracks'][number]['flag']>, string> = {
  stopped: 'STOPPED IN FLOW', 'wrong-way': 'WRONG WAY?', 'person-road': 'ON ROAD',
};

/** Boxes and motion arrows from the analysed clip's last frame, over the camera image. */
function DetectionOverlay({ boxes }: { boxes: HeimdallCameraAnalysis['boxes'] }) {
  return (
    <svg viewBox="0 0 100 75" preserveAspectRatio="none" aria-hidden
      style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none' }}>
      {boxes.map((b, i) => {
        const c = DET_COLOUR[b.cls] ?? '#b48cff';
        const x = b.x * 100, y = b.y * 75, w = b.w * 100, h = b.h * 75;
        const cx = x + w / 2, cy = y + h / 2;
        const moving = Math.hypot(b.dx, b.dy) > 0.004;
        return (
          <g key={i}>
            <rect x={x} y={y} width={w} height={h} fill="none" stroke={c} strokeWidth={0.4} vectorEffect="non-scaling-stroke" />
            {moving && <line x1={cx} y1={cy} x2={cx + b.dx * 300} y2={cy + b.dy * 225} stroke={c} strokeWidth={1.4} vectorEffect="non-scaling-stroke" />}
          </g>
        );
      })}
    </svg>
  );
}

interface AnalysisProps {
  ai: HeimdallCameraAnalysis | undefined;
  showBoxes: boolean;
  setShowBoxes: (v: boolean) => void;
  canAnalyse: boolean;
  running: boolean;
  failure: string | null;
  analyse: () => void;
}

/** What the analytics found: counts by class, moving vs stationary, flow and heading;
 *  ANALYSE asks for a fresh analysis of the camera's latest clip. [FR-P3-5] */
function AnalysisPanel({ ai, showBoxes, setShowBoxes, canAnalyse, running, failure, analyse }: AnalysisProps) {
  const age = ai ? Math.max(0, Math.round(Date.now() / 1000 - ai.at)) : 0;
  return (
    <div style={{ marginTop: 10, paddingTop: 8, borderTop: '1px solid var(--divider)' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6 }}>
        <span className="mg-label" style={{ color: 'var(--text-faint)' }}>
          AI ANALYSIS{running ? ' · ANALYSING…' : ai ? ` · ${age < 120 ? `${age}s` : `${Math.round(age / 60)}m`} AGO` : ''}
        </span>
        {ai?.queue && <span style={flagChip('var(--warn)')}>QUEUE</span>}
        {!!ai?.wrongWay && <span style={flagChip('var(--danger)')}>WRONG WAY?</span>}
        {!!ai?.blocking && <span style={flagChip('var(--danger)')}>STOPPED IN FLOW</span>}
        {!!ai?.personRoad && <span style={flagChip('var(--info)')}>PERSON ON ROAD</span>}
        <span style={{ marginLeft: 'auto', display: 'flex', gap: 6 }}>
          {ai && <button type="button" onClick={() => setShowBoxes(!showBoxes)} aria-pressed={showBoxes} style={camBtn(showBoxes)}>BOXES</button>}
          {canAnalyse && (
            <button type="button" onClick={analyse} disabled={running} style={{ ...camBtn(running), opacity: running ? 0.6 : 1, cursor: running ? 'progress' : 'pointer' }}>
              {running ? 'ANALYSING' : ai ? 'RE-ANALYSE' : 'ANALYSE'}
            </button>
          )}
        </span>
      </div>
      {failure && <div style={{ ...mono, fontSize: 9, color: 'var(--warn)', marginBottom: 4, overflowWrap: 'anywhere' }}>ANALYSIS FAILED · {failure.toUpperCase()}</div>}
      {!ai && !running && (
        <div style={{ ...mono, fontSize: 9, color: 'var(--text-faint)', lineHeight: 1.6 }}>
          {canAnalyse ? 'NOT ANALYSED YET · PRESS ANALYSE TO DETECT VEHICLES AND PEOPLE IN THE LATEST CLIP' : 'CAMERA ANALYTICS NOT AVAILABLE ON THIS GATEWAY'}
        </div>
      )}
      {ai && <AnalysisBody ai={ai} />}
      {ai?.playback && (
        <div style={{ ...mono, fontSize: 8, color: 'var(--text-faint)', marginTop: 4, lineHeight: 1.6 }}>
          CLIP · BOXES FOLLOW THE VIDEO ({ai.playback.tracks.length} TRACKS, FIRST {Math.round(ai.playback.frames / ai.playback.fps)} S) · DASHED = STATIONARY
        </div>
      )}
    </div>
  );
}

const flagChip = (c: string): React.CSSProperties => ({ ...mono, fontSize: 9, color: c, border: `1px solid ${c}`, borderRadius: 3, padding: '1px 5px', whiteSpace: 'nowrap' });

function AnalysisBody({ ai }: { ai: HeimdallCameraAnalysis }) {
  return (
    <>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '4px 10px', ...mono, fontSize: 10, color: 'var(--text-body)' }}>
        {ai.counts.map((c) => <span key={c.cls}><span style={{ color: DET_COLOUR[c.cls] ?? '#b48cff' }}>■</span> {c.cls.toUpperCase()} {c.n}</span>)}
      </div>
      <Row k="MOVING" v={`${ai.moving ?? 0} moving · ${ai.stationary ?? 0} stationary${ai.stopped ? ` · ${ai.stopped} stopped throughout` : ''}`} />
      {ai.flow.length > 0 && <Row k="FLOW" v={ai.flow.filter((f) => f.n > 0).map((f) => `${f.dir} ${f.n}`).join(' · ') || '—'} />}
      {(!!ai.wrongWay || !!ai.blocking || !!ai.personRoad) && (
        <Row k="INCIDENTS" tone="var(--warn)" v={[
          ai.wrongWay ? `${ai.wrongWay} possibly wrong way` : '',
          ai.blocking ? `${ai.blocking} stopped in the flow` : '',
          ai.personRoad ? `${ai.personRoad} ${ai.personRoad === 1 ? 'person' : 'people'} on the road` : '',
        ].filter(Boolean).join(' · ')} />
      )}
      {ai.headingText && <Row k="MAIN FLOW" v={`${ai.headingText}${ai.heading !== undefined ? ` (≈${Math.round(ai.heading)}°)` : ''}`} />}
      <div style={{ ...mono, fontSize: 8, color: 'var(--text-faint)', marginTop: 4, lineHeight: 1.6 }}>
        {ai.model ?? 'OBJECT DETECTION'} · {ai.frames ?? '?'} FRAMES · DIRECTIONS FROM THE CAMERA'S VIEW, APPROXIMATE · PEOPLE COUNTED ONLY, NOT IDENTIFIED
      </div>
    </>
  );
}

const camBtn = (on: boolean): React.CSSProperties => ({
  all: 'unset', cursor: 'pointer', ...mono, fontSize: 9, padding: '4px 8px', borderRadius: 'var(--radius-sm)',
  border: `1px solid ${on ? 'var(--accent)' : 'var(--divider)'}`, color: on ? 'var(--accent-hover)' : 'var(--text-muted)',
});

/** The selected aircraft's flights over Heimdall's retention window, newest first:
 *  click one to show its path on the map and its profile below. [FR-H-1] */
function FlightsCard({ flights, since, pick, setPick }: { flights: Flight[] | null; since: number | null; pick: number | null; setPick: (i: number | null) => void }) {
  const hm = (s: number) => new Date(s * 1000).toISOString().slice(11, 16);
  const dur = (s: number) => { const m = Math.round(s / 60); return m >= 60 ? `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, '0')}` : `${m} min`; };
  const place = (a: KnownAirport | null, fallback: string) => (a ? a.icao : fallback);
  return (
    <Card size="sm">
      <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 6 }}>
        FLIGHTS · RECORDED BY HEIMDALL{since ? ` SINCE ${hm(since)} UTC` : ''}
      </div>
      {flights === null && <div style={{ ...mono, color: 'var(--text-faint)' }}>READING HISTORY…</div>}
      {flights?.length === 0 && <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>No complete flight recorded yet: only ground positions, or under three minutes airborne.</div>}
      <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        {flights?.map((f, i) => {
          const on = pick === i;
          return (
            <button key={f.start} type="button" onClick={() => setPick(on ? null : i)} aria-pressed={on}
              style={{ all: 'unset', cursor: 'pointer', display: 'grid', gridTemplateColumns: '1fr auto', gap: '2px 8px', padding: '7px 9px', borderRadius: 'var(--radius-sm)', border: `1px solid ${on ? 'var(--accent)' : 'var(--divider)'}`, background: on ? 'var(--accent-tint)' : 'transparent' }}>
              <span style={{ ...mono, fontSize: 11, color: 'var(--text-heading)' }}>
                {place(f.from, 'EN ROUTE')} → {f.current ? 'IN FLIGHT' : place(f.to, f.landed ? 'FIELD' : 'LOST')}
              </span>
              <span style={{ ...mono, color: f.current ? 'var(--secure)' : 'var(--text-faint)' }}>{f.current ? '● NOW' : dur(f.end - f.start)}</span>
              <span style={{ ...mono, color: 'var(--text-muted)' }}>
                {hm(f.start)}–{f.current ? 'now' : hm(f.end)} UTC · {Math.round(f.distanceNm).toLocaleString()} NM
              </span>
              <span style={{ ...mono, color: 'var(--text-muted)' }}>{f.maxAltM !== null ? `FL${String(Math.round((f.maxAltM * 3.28084) / 100)).padStart(3, '0')}` : '—'}</span>
              {(f.from || f.to) && (
                <span style={{ gridColumn: '1 / -1', fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                  {[f.from?.name, f.to?.name].filter(Boolean).join(' → ')}
                </span>
              )}
            </button>
          );
        })}
      </div>
      <div style={{ ...mono, fontSize: 8, color: 'var(--text-faint)', marginTop: 6, lineHeight: 1.6 }}>
        SPLIT FROM RECORDED POSITIONS AT LANDINGS AND GAPS OVER 20 MIN. AIRPORTS ARE THE NEAREST WITHIN 25 KM WHERE A FLIGHT STARTS LOW OR ENDS ON THE GROUND (INFERRED). CLICK A FLIGHT TO SHOW IT.
      </div>
    </Card>
  );
}

function LiveTrackPanel({ t, onClose, replayAt, trailNote, path, flights, watch, rings, setRings, advice }: { advice?: React.ReactNode; t: HeimdallTrack; onClose: () => void; replayAt: number | null; trailNote: string; path: HeimdallHistoryPoint[] | null; flights?: { flights: Flight[] | null; since: number | null; pick: number | null; setPick: (i: number | null) => void }; watch: Watchlist; rings: boolean; setRings: (on: boolean) => void }) {
  const d = t.detail!;
  const colour = colourOf(t);
  const aircraft = d.kind === 'air-track';
  const event = d.kind.startsWith('event-');
  // Fixed things (airports, events, areas, platforms) have no speed, track, trail or flight history.
  const fixed = isFixedKind(d.kind);
  const watched = watch.has(d.key);
  const sq = aircraft && d.squawk ? SQUAWKS[d.squawk] : undefined;
  const title = aircraft ? d.callsign || d.key : d.name || t.id;
  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <Icon name="crosshair" size={17} color={colour} />
        <h2 style={{ fontSize: 'var(--fs-h4)', minWidth: 0 }}>{title}</h2>
        <Badge tone={t.track === 'anomalous' ? 'warn' : 'neutral'}>{t.cls}</Badge>
        {/* Watch this track: listed at the top of the summary, toasts as it comes and goes. [FR-H-1] */}
        <button type="button" onClick={() => watch.toggle(d.key, title)} aria-pressed={watched} aria-label={watched ? 'Remove from watchlist' : 'Add to watchlist'} title={watched ? 'Watching: click to stop' : 'Watch this track'}
          style={{ all: 'unset', cursor: 'pointer', marginLeft: 'auto', fontSize: 17, lineHeight: 1, padding: '6px 4px', color: watched ? 'var(--accent-hover)' : 'var(--text-faint)' }}>{watched ? '★' : '☆'}</button>
        <button onClick={onClose} style={{ all: 'unset', cursor: 'pointer', ...mono, color: 'var(--text-faint)', padding: '6px 0' }}>← SUMMARY</button>
      </div>
      <label style={{ display: 'flex', alignItems: 'center', gap: 8, ...mono, color: 'var(--text-muted)', cursor: 'pointer' }}>
        <input type="checkbox" checked={rings} onChange={(ev) => setRings(ev.target.checked)} style={{ accentColor: 'var(--accent)', width: 15, height: 15, margin: 0 }} />
        RANGE RINGS 10 · 25 · 50 NM
      </label>
      {replayAt !== null && (
        <div style={{ padding: '7px 12px', border: '1px solid var(--warn)', borderRadius: 'var(--radius-sm)', ...mono, color: 'var(--warn)' }}>
          REPLAY · RECORDED POSITION AT {utc(replayAt)} UTC
        </div>
      )}
      {sq && (
        <div style={{ padding: '9px 12px', border: '1px solid var(--warn)', background: 'var(--warn-tint)', borderRadius: 'var(--radius-sm)', ...mono, color: 'var(--warn)' }}>
          SQUAWK {d.squawk} · {sq}
        </div>
      )}
      {!fixed && (
        <Card size="sm">
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 12 }}>
            <StatBlock value={t.alt} label="ALTITUDE" size="sm" />
            <StatBlock value={t.spd.toLocaleString() + ' KT'} label="GROUND SPEED" size="sm" />
            <StatBlock value={String(Math.round(t.hdg)).padStart(3, '0') + '°'} label="TRACK" size="sm" />
          </div>
        </Card>
      )}
      <Card size="sm">
        <Row k={aircraft ? 'ICAO ADDRESS' : 'ENTITY'} v={d.key} />
        {aircraft && <Row k="CALLSIGN" v={d.callsign ?? '—'} />}
        {aircraft && <Row k="SQUAWK" v={d.squawk ?? '—'} tone={sq ? 'var(--warn)' : undefined} />}
        {aircraft && <Row k="CATEGORY" v={d.category ?? '—'} />}
        {!aircraft && <Row k="KIND" v={d.kind} />}
        {(d.facts ?? []).map((f) => <Row key={f.label} k={f.label} v={f.value} />)}
        <Row k="POSITION" v={`${d.lat.toFixed(3)}, ${d.lon.toFixed(3)}${d.altM !== undefined && !event ? ` · ${Math.round(d.altM).toLocaleString()} m` : ''}`} />
        {replayAt === null && <Row k="REPORTED" v={`${fmtAgo(secsAgo(d.lastSeen))} ago`} />}
        {replayAt === null && <Row k="SHOWN" v={d.estimatedSecs > 0 ? `estimated ${d.estimatedSecs}s ahead (dead reckoning)` : 'as reported'} tone={d.estimatedSecs > 0 ? 'var(--text-muted)' : undefined} />}
        {replayAt !== null && <Row k="SHOWN" v="interpolated between recorded positions" tone="var(--text-muted)" />}
        <Row k="FIRST SEEN" v={`${fmtAgo(secsAgo(d.firstSeen))} ago`} />
        <Row k="OBSERVATIONS" v={d.observations.toLocaleString()} />
        <Row k="SOURCES" v={d.sources.length ? d.sources.join(', ') + ' · ML-DSA signed' : '—'} />
        {!fixed && <Row k="TRAIL" v={trailNote} />}
      </Card>
      {d.taf && (
        <Card size="sm">
          {/* The airport's forecast, and the worst it says for the next 12 hours. [FR-H-1] */}
          <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 6 }}>TAF</div>
          {d.tafWorst && <Row k="NEXT 12 H" v={`worst: ${d.tafWorst}`} tone={d.tafWorst === 'IFR' || d.tafWorst === 'LIFR' ? 'var(--danger)' : d.tafWorst === 'MVFR' ? 'var(--info)' : undefined} />}
          <div style={{ ...mono, fontSize: 10, letterSpacing: '0.04em', color: 'var(--text-body)', lineHeight: 1.6, overflowWrap: 'anywhere', marginTop: 4 }}>{d.taf}</div>
        </Card>
      )}
      {flights && <FlightsCard {...flights} />}
      {replayAt === null && advice}
      {d.camera && <CameraCard camera={d.camera} name={title} />}
      {!fixed && path && path.length > 1 && <FlightHistory points={path} replayAt={replayAt} />}
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
        <ProvenanceTag id="FR-P3-3" title="Shared 3D scene core: networked entities as navigable 3D" />
      </div>
    </>
  );
}

/** The inspector with nothing selected: what is in the live picture right now. */
/** Find a track by callsign, name or ICAO address and select it. [FR-H-1] */
function FindTrack({ tracks, select }: { tracks: HeimdallTrack[]; select: (id: string) => void }) {
  const [q, setQ] = React.useState('');
  const needle = q.trim().toUpperCase();
  const hits = needle.length < 2 ? [] : tracks.filter((t) =>
    t.id.toUpperCase().includes(needle) || (t.detail?.key ?? '').toUpperCase().includes(needle) || (t.detail?.name ?? '').toUpperCase().includes(needle),
  ).slice(0, 6);
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      <input
        type="search" value={q} onChange={(ev) => setQ(ev.target.value)} placeholder="Find aircraft, ship or airport (callsign, ICAO, name)"
        aria-label="Find a track"
        onKeyDown={(ev) => { if (ev.key === 'Enter' && hits[0]) { select(hits[0].id); setQ(''); } }}
        style={{ minHeight: 40, padding: '0 10px', ...mono, fontSize: 11, color: 'var(--text-body)', background: 'var(--surface-inset)', border: '1px solid var(--border)', borderRadius: 'var(--radius-sm)', outline: 'none' }}
      />
      {hits.map((t) => (
        <button key={t.id} type="button" onClick={() => { select(t.id); setQ(''); }}
          style={{ all: 'unset', cursor: 'pointer', display: 'flex', gap: 8, padding: '8px 10px', ...mono, fontSize: 10, borderRadius: 'var(--radius-sm)', border: '1px solid var(--divider)' }}>
          <span style={{ color: 'var(--text-heading)', flex: 1 }}>{t.id}</span>
          <span style={{ color: 'var(--text-faint)' }}>{t.cls}</span>
          <span style={{ color: 'var(--accent-hover)' }}>{t.alt}</span>
        </button>
      ))}
    </div>
  );
}

/** Watched tracks with their live state, at the top of the summary. [FR-H-1] */
function WatchlistCard({ watch, present, select }: { watch: Watchlist; present: Map<string, HeimdallTrack>; select: (id: string) => void }) {
  if (!watch.entries.length) return null;
  return (
    <div>
      <SectionLabel>WATCHLIST</SectionLabel>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 4, marginTop: 8 }}>
        {watch.entries.map((w) => {
          const t = present.get(w.key);
          return (
            <div key={w.key} style={{ display: 'flex', alignItems: 'center', gap: 8, ...mono, fontSize: 10 }}>
              <button type="button" disabled={!t} onClick={() => t && select(t.id)} style={{ all: 'unset', cursor: t ? 'pointer' : 'default', display: 'flex', gap: 8, flex: 1, minWidth: 0, padding: '4px 0' }}>
                <span style={{ color: t ? 'var(--secure)' : 'var(--text-faint)' }}>{t ? '●' : '○'}</span>
                <span style={{ color: t ? 'var(--text-heading)' : 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{t?.id ?? w.label}</span>
                <span style={{ color: 'var(--text-faint)', marginLeft: 'auto', whiteSpace: 'nowrap' }}>{t ? `IN PICTURE · ${t.alt}` : w.lastSeen ? `LAST SEEN ${agoText(w.lastSeen).toUpperCase()}` : 'NOT SEEN'}</span>
              </button>
              <button type="button" aria-label={`Stop watching ${w.label}`} onClick={() => watch.toggle(w.key, w.label)} style={{ all: 'unset', cursor: 'pointer', color: 'var(--text-faint)', padding: '4px 2px' }}>✕</button>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function LiveSummaryPanel({ picture, tracks, filter, setFilter, select, replayAt, watchCard, areaCard, askCard }: {
  askCard?: React.ReactNode;
  watchCard?: React.ReactNode;
  areaCard?: React.ReactNode;
  picture: HeimdallPicture;
  tracks: HeimdallTrack[];
  filter: string | null;
  setFilter: (f: string | null) => void;
  select: (id: string) => void;
  /** Replay time (unix s), or null when live. */
  replayAt: number | null;
}) {
  // Air figures count aircraft and balloons only; satellites and events have their own rows.
  const air = tracks.filter((t) => t.layer === 'air');
  const airborne = air.filter((t) => !t.detail?.onGround);
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
        <h2 style={{ fontSize: 'var(--fs-h4)' }}>{replayAt === null ? 'Live picture' : 'Replayed picture'}</h2>
        {replayAt === null ? <Badge tone="secure">LIVE</Badge> : <Badge tone="warn">REPLAY</Badge>}
      </div>
      {replayAt === null && askCard}
      {watchCard}
      {areaCard}
      <div style={{ padding: '8px 10px', border: '1px solid var(--divider)', borderRadius: 'var(--radius-sm)', ...mono, color: 'var(--text-muted)', lineHeight: 1.6 }}>
        FLIGHT HISTORY: POINT AT ANY AIRCRAFT TO SEE ITS RECORDED PATH; CLICK IT (OR FIND IT BELOW) FOR ITS FLIGHTS. TRAIL 30M OR ALL SHOWS EVERY PATH.
      </div>
      <FindTrack tracks={tracks} select={select} />
      <div style={{ ...mono, color: replayAt === null ? 'var(--text-faint)' : 'var(--warn)' }}>
        {replayAt === null
          ? `READ ${fmtAgo(Math.round((Date.now() - (picture.fetchedAt ?? Date.now())) / 1000))} AGO · NEWEST REPORT ${newest ? fmtAgo(secsAgo(newest)) : '—'} AGO`
          : `RECORDED POSITIONS AT ${utc(replayAt)} UTC · ${tracks.length.toLocaleString()} TRACKS`}
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 12 }}>
          <StatBlock value={airborne.length.toLocaleString()} label="AIRBORNE" size="sm" />
          <StatBlock value={(air.length - airborne.length).toLocaleString()} label="ON GROUND" size="sm" />
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
            const n = air.filter((t) => bandOf(t).id === b.id).length;
            return (
              <div key={b.id} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <span style={{ width: 8, height: 8, borderRadius: '50%', background: b.color, flex: 'none' }} />
                <span style={{ ...mono, color: 'var(--text-body)', width: 122 }}>{b.label}</span>
                <span style={{ flex: 1, height: 5, background: 'var(--surface-inset)', borderRadius: 3, overflow: 'hidden' }}>
                  <span style={{ display: 'block', height: '100%', width: `${(n / Math.max(1, air.length)) * 100}%`, background: b.color }} />
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
        POSITIONS FROM THE FEEDS HEIMDALL FUSES (ADS-B, RADIOSONDES, ORBITAL ELEMENTS, SEISMIC AND FLOOD WARNINGS), EACH SIGNED. BETWEEN REPORTS EACH AIRBORNE AIRCRAFT IS MOVED ALONG ITS TRACK AT ITS GROUND SPEED (ESTIMATED). TRAILS COME FROM RECORDED HISTORY, OR FROM POSITIONS SEEN SINCE THIS PAGE OPENED WHEN THE GATEWAY HAS NONE.
      </div>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-H-1" title="Multi-source fusion into one common operating picture" />
        <ProvenanceTag id="FR-H-2" title="Rule-based, explainable classification" />
      </div>
    </>
  );
}

// three.js loads only when the 3D view is first opened. [FR-P3-7]
const Globe3D = React.lazy(() => import('../app/globe/Globe3D').then((m) => ({ default: m.Globe3D })));

const VIEW_KEY = 'heimdall.cop.view';
function readView(): '2d' | '3d' {
  try {
    return window.localStorage.getItem(VIEW_KEY) === '3d' ? '3d' : '2d';
  } catch {
    return '2d';
  }
}

const inBox = (t: HeimdallTrack) => t.x >= 0 && t.x <= 100 && t.y >= 0 && t.y <= 100;

/** The wall display's tour, one stop every 45 s. [FR-P3-7] */
const TOUR: PresetId[] = ['uk', 'london', 'scotland', 'baltic', 'globe'];
const TOUR_MS = 45_000;
/** After any interaction the tour waits this long before carrying on. */
const TOUR_PAUSE_MS = 120_000;
const RING_NMS = [10, 25, 50];

/** A small chip on the canvas (RADAR, MEASURE), matching the 3D view's MAP chip. */
function Chip({ on, onClick, children, label, tone }: { on: boolean; onClick: () => void; children: React.ReactNode; label?: string; tone?: string }) {
  return (
    <button type="button" onClick={onClick} aria-pressed={on} aria-label={label}
      style={{ all: 'unset', cursor: 'pointer', minHeight: 32, padding: '0 10px', display: 'inline-flex', alignItems: 'center', fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: tone ?? (on ? 'var(--accent-hover)' : 'var(--text-muted)'), background: on ? 'var(--surface-raised)' : 'color-mix(in srgb, var(--bg-sunken) 85%, transparent)', border: `1px solid ${on ? 'var(--accent)' : 'var(--divider)'}`, borderRadius: 'var(--radius-sm)', whiteSpace: 'nowrap' }}>
      {children}
    </button>
  );
}

export function OperatingPictureScreen() {
  const gateway = useGateway<HeimdallGateway>();
  const [regionIds, setRegionIdsState] = React.useState<string[]>(readRegions);
  const setRegionIds = (ids: string[]) => { setRegionIdsState(ids); saveRegions(ids); };
  const [focusHover, setFocusHoverState] = React.useState<boolean>(readFocus);
  const setFocusHover = (on: boolean) => { setFocusHoverState(on); saveFocus(on); };
  const world = regionIds.includes(WORLD_ID);
  const enabledRegions = React.useMemo(() => REGION_LIST.filter((r) => regionIds.includes(r.id)), [regionIds]);
  const [pointerRegion, setPointerRegion] = React.useState<Region | null>(null);
  const [lookRegion, setLookRegion] = React.useState<Region | null>(null);
  // The gateway reads only the regions switched on.
  React.useEffect(() => {
    gateway.setRegions?.(world ? null : enabledRegions.map((r) => r.box));
  }, [gateway, world, enabledRegions]);
  const status = useStatus();
  const wall = status.wall;
  const watch = useWatchlist();
  // Canvas tools, all off until asked for. [FR-H-1]
  const [radarOn, setRadarOn] = React.useState(false);
  const radar = useRadar(radarOn);
  const [trafficOn, setTrafficOn] = React.useState(false);
  const [alertsOnMap, setAlertsOnMap] = React.useState(true);
  const traffic = useTraffic(trafficOn);
  const [measuring, setMeasuring] = React.useState(false);
  const [measurePts, setMeasurePts] = React.useState<LatLon[]>([]);
  const [ringsOn, setRingsOn] = React.useState(false);
  const [flyRequest, setFlyRequest] = React.useState<{ n: number; id?: string; lat?: number; lon?: number; preset?: PresetId } | null>(null);
  const [alertPin, setAlertPin] = React.useState<LatLon | null>(null);
  const [tourOn, setTourOn] = React.useState(false);
  const pausedUntil = React.useRef(0);
  const { data: picture, reload } = useAsync<HeimdallPicture, HeimdallGateway>((c) => c.getPicture(), []);
  // Redraw every 2 s; the live client fetches from the gateway at most every 15 s and
  // moves tracks along their heading in between. [FR-H-1]
  React.useEffect(() => {
    const timer = window.setInterval(reload, 2000);
    return () => window.clearInterval(timer);
  }, [reload]);

  // 2D map or 3D globe, remembered per browser. [FR-P3-7]
  const [view, setViewState] = React.useState<'2d' | '3d'>(readView);
  const setView = (v: '2d' | '3d') => {
    setViewState(v);
    try { window.localStorage.setItem(VIEW_KEY, v); } catch { /* storage blocked: still switches */ }
  };

  // The replay timeline and the history behind it, shared by both views. [FR-P3-7]
  const [windowSecs, setWindowSecs] = React.useState(3600);
  const [trailMins, setTrailMins] = React.useState(10);
  const trailSecs = trailMins ? trailMins * 60 : windowSecs;
  const clock = useReplayClock(windowSecs, view === '3d' ? 100 : 250);
  const history = useHistory(gateway, windowSecs, trailSecs);
  const replay = !clock.live && history.status === 'ready';
  const replayAt = replay ? clock.t : null;

  const layerDefs = picture?.layers ?? [];
  /** Every positioned entity worldwide (the globe's), live. */
  const geoLive = React.useMemo(() => picture?.geo ?? picture?.tracks ?? [], [picture]);
  /** Geographic picture (live, or any replay): a basemap, trails and altitude colours
   *  replace the reference layout's illustrative rings and sensor footprint. */
  const region: HeimdallRegion | undefined = replay ? (picture?.region ?? UK_AND_IRELAND) : picture?.region;
  const replayed = React.useMemo(
    () => (replay ? replayTracks(geoLive, history.store, clock.t, region, Math.max(60, 2 * stepFor(windowSecs))) : null),
    [replay, geoLive, history.store, clock.t, region, windowSecs],
  );
  const allTracks: HeimdallTrack[] = view === '3d'
    ? (replayed ?? geoLive)
    : (replayed ? replayed.filter(inBox) : (picture?.tracks ?? []));
  const geoView = view === '3d' || !!region;
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
  // The panels along the bottom (figures + timeline): the globe centres above them, and
  // on short canvases the figures fold to one line.
  const bottomRef = React.useRef<HTMLDivElement>(null);
  const [bottomH, setBottomH] = React.useState(0);
  React.useEffect(() => {
    const el = bottomRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setBottomH(el.offsetHeight + 10));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const [statsOpenChoice, setStatsOpen] = React.useState<boolean | null>(null);
  const statsOpen = statsOpenChoice ?? false;
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
      // Airspace (about 1,300 volumes) starts off, whatever the feed says. [FR-H-1]
      // Airspace is dense: off until asked for (the fixed utility sites show only close in).
      setLayers(layerDefs.reduce((acc, l) => ({ ...acc, [l.id]: l.id === 'airspace' ? false : l.active }), {} as LayerState));
    }
  }, [layerDefs, layers]);

  const layerOn = (id: HeimdallLayerId) => layers?.[id] !== false;
  // The Gulf watch switch: its symbols and its list together. [FR-H-4]
  const [gulfWatchOn, setGulfWatchOn] = React.useState(true);
  const setLayer = (id: HeimdallLayerId, v: boolean) => setLayers((l) => ({ ...(l ?? ({} as LayerState)), [id]: v }));

  const e = tracks.find((t) => t.id === sel);
  // Regions: only the ones switched on are read and drawn; with FOCUS, only the one under
  // the pointer (or the camera's centre). Satellites and space weather are global. [FR-P3-7]
  const focusRegion = focusHover && !world ? (pointerRegion ?? lookRegion) : null;
  const visible = React.useMemo(() => tracks.filter((t) => {
    if (layers?.[t.layer] === false) return false;
    if (!gulfWatchOn && isGulfWatch(t)) return false;
    if (world || t.layer === 'space') return true;
    const lat = t.lat ?? t.detail?.lat, lon = t.lon ?? t.detail?.lon;
    if (lat === undefined || lon === undefined) return true;
    if (focusRegion) return inRegionBox(focusRegion.box, lat, lon) || t.id === sel;
    return enabledRegions.some((r) => inRegionBox(r.box, lat, lon)) || t.id === sel;
  }), [tracks, layers, gulfWatchOn, world, focusRegion, enabledRegions, sel]);
  const options = e?.options ?? [];

  // Trails: the last `trailMins` of recorded history up to the time shown, joined to the
  // drawn position; without history, the positions this session has seen. [FR-P3-7]
  const tShown = clock.t;
  const trails = React.useMemo(() => {
    const out = new Map<string, HeimdallHistoryPoint[]>();
    if (!geoView) return out;
    for (const t of visible) {
      const h = history.store.get(keyOf(t));
      let pts: HeimdallHistoryPoint[] = h
        ? trailOf(h.points, tShown - trailSecs, tShown)
        : replay ? [] : (t.geoTrail ?? []).filter((p) => p[0] >= tShown - trailSecs);
      if (!replay && t.detail && pts.length) {
        pts = [...pts, [tShown, t.lat ?? t.detail.lat, t.lon ?? t.detail.lon, t.detail.altM ?? null]];
      }
      if (pts.length > 1) out.set(t.id, pts);
    }
    return out;
  }, [geoView, visible, history.store, tShown, trailSecs, replay]);
  // Trails only for moving vehicles inside the view and a margin just beyond it: the
  // radius follows the camera, so panning and zooming bring trails in and out. [FR-P3-7]
  const [viewCircle, setViewCircle] = React.useState<{ lat: number; lon: number; km: number } | null>(null);
  const globeTrails = React.useMemo<GlobeTrail[]>(
    // Satellites are left out: their orbits cut straight across the picture. A selected
    // satellite still shows its path.
    () => {
      if (view !== '3d') return [];
      // A little more than what is on screen (the view spans about the camera distance).
      const radiusKm = viewCircle ? Math.max(40, viewCircle.km * 1.15) : Infinity;
      const near = (t: HeimdallTrack) => {
        if (!viewCircle || t.id === sel) return true;
        const lat = t.lat ?? t.detail?.lat, lon = t.lon ?? t.detail?.lon;
        if (lat === undefined || lon === undefined) return false;
        const p1 = (viewCircle.lat * Math.PI) / 180, p2 = (lat * Math.PI) / 180, dl = ((lon - viewCircle.lon) * Math.PI) / 180;
        const h = Math.sin((p2 - p1) / 2) ** 2 + Math.cos(p1) * Math.cos(p2) * Math.sin(dl / 2) ** 2;
        return 2 * 6371 * Math.asin(Math.min(1, Math.sqrt(h))) <= radiusKm;
      };
      return visible.flatMap((t) => {
        const shape = shapeOf(t);
        if (shape !== 'aircraft' && shape !== 'vessel' && shape !== 'balloon' && shape !== 'train' && t.id !== sel) return [];
        if (!near(t)) return [];
        const p = trails.get(t.id);
        return p ? [{ colour: colourOf(t), points: p }] : [];
      });
    },
    [view, visible, trails, viewCircle, sel],
  );

  // Utility network lines for the 2D map (the globe builds its own), rebuilt only when the
  // lines change. [FR-H-1]
  const lineKey2d = view === '2d' ? utilityLineKey(visible) : '';
  const visibleRef = React.useRef(visible);
  visibleRef.current = visible;
  const utilityLines2d = React.useMemo(() => (lineKey2d ? utilityLineGroups(visibleRef.current) : []), [lineKey2d]);

  // Ask Mimir about what is on screen: in 3D the view circle (once close enough to mean
  // an area), else the regions shown. [FR-MI-3]
  const askFocus = view === '3d' && viewCircle && viewCircle.km < 1500
    ? { lat: viewCircle.lat, lon: viewCircle.lon, km: Math.max(20, viewCircle.km * 0.6) } : null;

  // Cameras on screen (the view spans about the camera distance), nearest the centre
  // first, for analysing an area. [FR-P3-5]
  const camerasInView = React.useMemo<HeimdallTrack[]>(() => {
    if (view !== '3d' || !viewCircle || viewCircle.km > 400) return [];
    const p1 = (viewCircle.lat * Math.PI) / 180;
    const out: [number, HeimdallTrack][] = [];
    for (const t of visible) {
      if (shapeOf(t) !== 'camera' || !t.detail?.camera || t.lat === undefined || t.lon === undefined) continue;
      const p2 = (t.lat * Math.PI) / 180, dl = ((t.lon - viewCircle.lon) * Math.PI) / 180;
      const h = Math.sin((p2 - p1) / 2) ** 2 + Math.cos(p1) * Math.cos(p2) * Math.sin(dl / 2) ** 2;
      const d = 2 * 6371 * Math.asin(Math.min(1, Math.sqrt(h)));
      if (d <= viewCircle.km * 0.6) out.push([d, t]);
    }
    return out.sort((a, b) => a[0] - b[0]).map(([, t]) => t);
  }, [view, visible, viewCircle]);

  // The selected entity's whole recorded path over the window.
  const selKey = e ? keyOf(e) : null;
  const entityPath = useEntityPath(gateway, history.status === 'ready' ? selKey : null, windowSecs, history.to);
  // The selected aircraft's flights over the whole retention window, and the one picked.
  const knownAirports = React.useMemo<KnownAirport[]>(
    () => geoLive.flatMap((t) => (t.detail?.kind === 'land-airport' ? [{ icao: t.detail.key, name: t.detail.name ?? t.detail.key, lat: t.detail.lat, lon: t.detail.lon }] : [])),
    [geoLive],
  );
  const aircraftKey = e?.detail?.kind === 'air-track' ? selKey : null;
  const flightState = useFlights(gateway, aircraftKey, knownAirports);
  const [flightPick, setFlightPick] = React.useState<number | null>(null);
  React.useEffect(() => setFlightPick(null), [aircraftKey]);
  const picked = flightPick !== null ? flightState.flights?.[flightPick] : undefined;
  const selPath = picked?.points ?? entityPath ?? (selKey ? history.store.get(selKey)?.points ?? null : null);
  // Pointing at a track shows its recorded flight straight away, before any click.
  const hoverTrack = hover && hover !== sel ? allTracks.find((t) => t.id === hover) : undefined;
  const hoverPath = hoverTrack ? history.store.get(keyOf(hoverTrack))?.points ?? null : null;
  const trailNote = (t: HeimdallTrack) => {
    const n = history.store.get(keyOf(t))?.points.length;
    if (history.status === 'ready') return n ? `${n.toLocaleString()} recorded positions over the window; trail shows ${trailMins ? `the last ${trailMins} min` : 'the whole window'}` : 'no recorded positions in this window';
    return `${t.geoTrail?.length ?? t.trail?.length ?? 0} reported positions since this page opened (history unavailable)`;
  };

  // ---- watchlist: live state, and a toast as a watched track comes or goes. [FR-H-1]
  const byKey = React.useMemo(() => {
    const m = new Map<string, HeimdallTrack>();
    for (const t of geoLive) m.set(keyOf(t), t);
    return m;
  }, [geoLive]);
  const watchKeys = watch.entries.map((w) => w.key).join('|');
  const prevPresent = React.useRef<Set<string> | null>(null);
  React.useEffect(() => {
    if (!picture) return;
    const present = new Map<string, string>();
    for (const w of watch.entries) { const t = byKey.get(w.key); if (t) present.set(w.key, t.id); }
    const now = Math.floor(Date.now() / 1000);
    const prev = prevPresent.current;
    if (prev) {
      for (const w of watch.entries) {
        const was = prev.has(w.key), is = present.has(w.key);
        if (!was && is) status.toast(`WATCHLIST · ${present.get(w.key)} IN THE PICTURE`, 'info');
        if (was && !is) status.toast(`WATCHLIST · ${w.label} LEFT THE PICTURE`, 'warn');
      }
    }
    prevPresent.current = new Set(present.keys());
    watch.seen(present, now);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [byKey, watchKeys]);

  // ---- areas (SIGMETs, warnings, wind farms, roads, airspace) for both views. [FR-H-1] [FR-P3-3]
  const areaTracks = React.useMemo(() => visible.filter((t) => t.detail?.area && t.detail.area.length >= 3), [visible]);
  const globeAreas = React.useMemo<GlobeArea[]>(() => (view !== '3d' ? [] : areaTracks.map((t) => {
    const d = t.detail!;
    const ft = (v: number | undefined) => (v === undefined ? undefined : v / 3.28084);
    // SIGMETs are volumes (their altitude band is the point); airspace lies flat on the
    // ground as an outline with a light tint, since 1,300 volumes bury the picture. Its
    // vertical limits are in the tooltip and inspector.
    if (d.kind === 'event-sigmet') {
      const base = Math.max(0, d.baseFt ?? 0);
      // A SIGMET without a top is drawn 10,000 ft deep.
      const top = Math.min(66000, d.topFt ?? base + 10000);
      return { id: `${t.id}:${d.kind}`, mode: 'volume' as const, ring: d.area!, colour: areaColour(d), fill: 0.11, baseM: ft(base), topM: ft(top) };
    }
    // A warning issued ahead is outlined only until it comes into force.
    const fill = d.kind === 'event-weather-warning' ? (inForce(d) ? 0.2 : 0.04) : d.kind === 'land-road-disruption' ? 0.4 : d.kind === 'airspace-volume' ? 0.05 : d.kind === 'event-gnss-interference' ? (d.level === 'high' ? 0.4 : d.level === 'medium' ? 0.28 : 0.16) : 0.14;
    return { id: `${t.id}:${d.kind}`, mode: 'drape' as const, ring: d.area!, colour: areaColour(d), fill };
  })).concat(view !== '3d' || world ? [] : enabledRegions.map((r) => {
    const [w, s, ea, n] = r.box;
    // The region's outline; the focused one brighter.
    return { id: `region:${r.id}`, mode: 'drape' as const, ring: [[n, w], [n, ea], [s, ea], [s, w]] as [number, number][], colour: focusRegion?.id === r.id ? '#ffd27a' : '#6f7f8f', fill: 0 };
  })), [areaTracks, view, world, enabledRegions, focusRegion]);

  // ---- measure: Escape clears. 2D clicks are read from the map position. [FR-H-1]
  React.useEffect(() => {
    if (!measuring && !measurePts.length) return;
    const onKey = (ev: KeyboardEvent) => { if (ev.key === 'Escape') { setMeasurePts([]); setMeasuring(false); } };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [measuring, measurePts.length]);
  const addMeasure = React.useCallback((at: LatLon) => setMeasurePts((p) => (p.length >= 2 ? [at] : [...p, at])), []);
  const selCentre: LatLon | null = e?.detail ? { lat: e.lat ?? e.detail.lat, lon: e.lon ?? e.detail.lon } : null;
  const rings = ringsOn && selCentre ? { centre: selCentre, nms: RING_NMS } : null;

  // ---- wall display: tour the presets every 45 s, paused for a while by any input. [FR-P3-7]
  // A wall display starts with nothing selected, so the inspector stays out of the way.
  React.useEffect(() => { if (wall) setSel(''); }, [wall]);
  React.useEffect(() => {
    if (!wall) { setTourOn(false); return; }
    const pause = () => { pausedUntil.current = Date.now() + TOUR_PAUSE_MS; };
    window.addEventListener('pointerdown', pause, true);
    window.addEventListener('keydown', pause, true);
    window.addEventListener('wheel', pause, { capture: true, passive: true });
    // Keep a wall screen awake where the browser allows it.
    let lock: { release: () => Promise<void> } | null = null;
    const nav = navigator as Navigator & { wakeLock?: { request: (t: 'screen') => Promise<{ release: () => Promise<void> }> } };
    nav.wakeLock?.request('screen').then((l) => { lock = l; }).catch(() => { /* not allowed: fine */ });
    return () => {
      window.removeEventListener('pointerdown', pause, true);
      window.removeEventListener('keydown', pause, true);
      window.removeEventListener('wheel', pause, true);
      void lock?.release().catch(() => undefined);
    };
  }, [wall]);
  React.useEffect(() => {
    if (!tourOn) return;
    let stop = 0;
    const next = () => {
      if (Date.now() < pausedUntil.current) return;
      setFlyRequest((r) => ({ n: (r?.n ?? 0) + 1, preset: TOUR[stop++ % TOUR.length] }));
    };
    pausedUntil.current = 0;
    next();
    const timer = window.setInterval(next, TOUR_MS);
    return () => window.clearInterval(timer);
  }, [tourOn]);

  // ---- alerts: pick one to select its entity and fly to it. [FR-H-1] [FR-H-5]
  const pickAlert = (a: HeimdallAlert) => {
    const t = byKey.get(a.entity) ?? geoLive.find((x) => x.id === a.entity);
    pausedUntil.current = Date.now() + TOUR_PAUSE_MS;
    if (t) {
      select(t.id);
      setAlertPin(null);
      setFlyRequest((r) => ({ n: (r?.n ?? 0) + 1, id: t.id }));
    } else if (a.lat !== null && a.lon !== null) {
      setAlertPin({ lat: a.lat, lon: a.lon });
      setFlyRequest((r) => ({ n: (r?.n ?? 0) + 1, lat: a.lat!, lon: a.lon! }));
    } else {
      status.toast(`${a.entity} IS NOT IN THE PICTURE`, 'info');
    }
  };

  // ---- open alerts on the map and in the strip over it, in the regions shown. [FR-H-4]
  const openAlerts = React.useMemo(() => (status.alerts ?? []).filter((a) => a.clearedAt === null), [status.alerts]);
  const alertMarks = React.useMemo<AlertMark[]>(() => {
    if (!alertsOnMap || replayAt !== null) return [];
    const rank = { critical: 0, warning: 1, info: 2 } as const;
    return openAlerts.flatMap((a) => {
      const t = byKey.get(a.entity);
      const lat = t?.lat ?? a.lat, lon = t?.lon ?? a.lon;
      if (lat === null || lon === null || lat === undefined || lon === undefined) return [];
      if (!world && !enabledRegions.some((r) => inRegionBox(r.box, lat, lon))) return [];
      // A weather warning says what and when (they are often issued days ahead).
      const warning = t?.detail?.kind === 'event-weather-warning' ? t.detail : undefined;
      const text = warning ? warningText(warning) : alertText(a.label);
      return [{ id: a.id, lat, lon, severity: a.severity, text, acked: !!a.acknowledgedBy, colour: WARNING_LEVEL_COLOURS[warning?.level ?? ''],
        title: `${a.severity.toUpperCase()} · ${text} · ${a.entity}${a.rationale ? ` · ${a.rationale}` : ''}` }];
    }).sort((x, y) => rank[x.severity] - rank[y.severity]);
  }, [alertsOnMap, replayAt, openAlerts, byKey, world, enabledRegions]);
  const pickAlertId = (id: string) => { const a = openAlerts.find((x) => x.id === id); if (a) pickAlert(a); };
  const alertName = React.useCallback((a: HeimdallAlert) => {
    const d = byKey.get(a.entity)?.detail;
    // A power cut by where it is and how many customers are off.
    if (d?.kind === 'utility-power-outage') {
      const f = (l: string) => d.facts?.find((x) => x.label === l)?.value;
      const where = f('AREA') ?? f('POSTCODES')?.split(/[;,]\s*/).slice(0, 2).join(', ') ?? f('OPERATOR');
      return where ? `${where}${f('CUSTOMERS') ? ` (${f('CUSTOMERS')})` : ''}` : undefined;
    }
    return d?.name;
  }, [byKey]);

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

  const select = (id: string) => { setSel(id); setGate(null); };
  // The Gulf watch list: its indicators wherever they are, while the Events layer is on.
  const gulfWatch = React.useMemo(() => (layerOn('events') ? geoLive.filter(isGulfWatch) : []), [geoLive, layers]); // eslint-disable-line react-hooks/exhaustive-deps
  const aside = geoView && picture ? (
    e?.detail ? (
      <LiveTrackPanel t={e} onClose={() => setSel('')} replayAt={replayAt} trailNote={trailNote(e)} path={selPath} flights={aircraftKey ? { ...flightState, pick: flightPick, setPick: setFlightPick } : undefined} watch={watch} rings={ringsOn} setRings={setRingsOn}
        advice={e.detail ? <AdviceCard gateway={gateway} subject={keyOf(e)} alertId={openAlerts.find((x) => x.entity === keyOf(e) && !x.acknowledgedBy)?.id ?? openAlerts.find((x) => x.entity === keyOf(e))?.id}
          pick={(key) => { const hit = byKey.get(key); if (hit) select(hit.id); }} /> : undefined} />
    ) : wall ? null : (
      <LiveSummaryPanel picture={picture} tracks={allTracks} filter={filter} setFilter={setFilter} select={select} replayAt={replayAt}
        watchCard={<WatchlistCard watch={watch} present={byKey} select={select} />}
        areaCard={replayAt === null && camerasInView.length > 0 ? <AreaCamerasCard gateway={gateway} cameras={camerasInView} select={select} /> : undefined}
        askCard={<AskCard gateway={gateway} focus={askFocus} pick={(key) => { const hit = byKey.get(key); if (hit) select(hit.id); }}
          areaLabel={askFocus ? `AROUND ${askFocus.lat.toFixed(2)}, ${askFocus.lon.toFixed(2)} · ${Math.round(askFocus.km)} KM` : world ? 'WHOLE WORLD' : `${enabledRegions.map((r) => r.label).join(' · ').toUpperCase() || 'REGIONS SHOWN'}`} />} />
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
  const project = (p: HeimdallHistoryPoint) => {
    const r = region ?? UK_AND_IRELAND;
    return `${((p[2] - r.west) / (r.east - r.west)) * 100},${((r.north - p[1]) / (r.north - r.south)) * 100}`;
  };
  // In replay the selected path ends at the replay time; live, at its newest point.
  const selPath2d = view === '2d' && region && selPath && selPath.length > 1
    ? (replay ? selPath.filter((p) => p[0] <= tShown) : selPath)
    : null;

  const viewToggle = (
    <span role="group" aria-label="Picture view" style={{ display: 'inline-flex', border: '1px solid var(--border)', borderRadius: 'var(--radius-sm)', overflow: 'hidden' }}>
      {(['2d', '3d'] as const).map((v) => (
        <button
          key={v}
          type="button"
          aria-pressed={view === v}
          onClick={() => setView(v)}
          style={{
            all: 'unset', cursor: 'pointer', minWidth: 44, height: 36, display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            ...mono, fontSize: 10, color: view === v ? 'var(--accent-hover)' : 'var(--text-muted)',
            background: view === v ? 'var(--surface-raised)' : 'transparent',
          }}
        >
          {v.toUpperCase()}
        </button>
      ))}
    </span>
  );

  return (
    <ConsolePage
      active="cop"
      context="COMMON OPERATING PICTURE / SECTOR NORTH"
      asideWidth={360}
      mainStyle={{ padding: 0, gap: 0, position: 'relative' }}
      toolbar={
        <>
          {viewToggle}
          {/* Wall display: no side nav, larger text and markers; ?wall=1 opens in it. [FR-P3-7] */}
          <Chip on={wall} onClick={() => status.setWall(!wall)} label={wall ? 'Leave wall display' : 'Wall display'}>WALL</Chip>
          {wall && <Chip on={tourOn} onClick={() => { if (!tourOn && view !== '3d') setView('3d'); setTourOn(!tourOn); }} label="Tour the camera presets every 45 seconds">TOUR</Chip>}
          <AlertsBell />
          <IconButton label="Settings">
            <Icon name="settings" size={17} />
          </IconButton>
        </>
      }
      navFooter={
        // Scrolls on its own when the regions and layers outgrow the window (the nav
        // above keeps its place).
        <div style={{ padding: '10px 10px 4px', display: 'flex', flexDirection: 'column', gap: 9, maxHeight: 'max(180px, calc(100vh - 430px))', overflowY: 'auto' }}>
          <div className="mg-label" style={{ color: 'var(--text-faint)' }}>REGIONS</div>
          <Switch checked={focusHover} onChange={setFocusHover} label="Focus on hover" />
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6, maxHeight: 160, overflowY: 'auto', paddingRight: 4, flex: 'none' }}>
            {REGION_LIST.map((r) => (
              <Switch key={r.id} checked={regionIds.includes(r.id)} label={r.label}
                onChange={(v) => setRegionIds(v ? [...regionIds.filter((x) => x !== WORLD_ID), r.id] : regionIds.filter((x) => x !== r.id))} />
            ))}
            <Switch checked={world} label="Whole world (heavy)" onChange={(v) => setRegionIds(v ? [...regionIds, WORLD_ID] : regionIds.filter((x) => x !== WORLD_ID))} />
          </div>
          <div className="mg-label" style={{ color: 'var(--text-faint)', marginTop: 6 }}>LAYERS</div>
          {/* Only layers with something on them. */}
          {layerDefs.filter((l) => geoLive.some((t) => t.layer === l.id)).map((l) => (
            <Switch key={l.id} checked={layerOn(l.id)} onChange={(v) => setLayer(l.id, v)} label={l.label} />
          ))}
          {geoLive.some(isGulfWatch) && <Switch checked={gulfWatchOn} onChange={setGulfWatchOn} label="Gulf watch" />}
        </div>
      }
      aside={aside ? (wall ? <div style={{ zoom: 1.2, display: 'flex', flexDirection: 'column', gap: 14 }}>{aside}</div> : aside) : undefined}
    >
      {/* The blueprint grid and glow stay in both views; the live view adds the map. */}
      <div ref={canvasRef} className="mg-grid-bg" style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }} />
      <div style={{ position: 'absolute', inset: 0, background: 'radial-gradient(ellipse at 55% 45%, var(--accent-tint), transparent 60%)', pointerEvents: 'none' }} />

      {view === '3d' && (
        <React.Suspense fallback={<div style={{ position: 'absolute', top: '40%', width: '100%', textAlign: 'center', ...mono, color: 'var(--text-faint)' }}>LOADING 3D VIEW…</div>}>
          <Globe3D tracks={visible} trails={globeTrails} selected={sel} path={selPath} hoverPath={hoverPath} pathsFirst={trailMins === 0 || trailMins >= 30} hover={hover} onSelect={select} onHover={setHover} bottomInset={bottomH}
            onPointerGround={(at) => { const r = at ? regionAt(enabledRegions, at.lat, at.lon) : null; setPointerRegion((p) => (p?.id === r?.id ? p : r)); }}
            onRegionPick={(preset) => {
              // Flying to a region switches its data on (sub-areas belong to a region).
              const id = REGION_FOR_PRESET[preset] ?? REGION_LIST.find((r) => r.preset === preset)?.id;
              if (id && !regionIds.includes(id) && !world) setRegionIds([...regionIds, id]);
            }}
            onLookAt={(at, km) => { const r = regionAt(enabledRegions, at.lat, at.lon); setLookRegion((p) => (p?.id === r?.id ? p : r)); setViewCircle({ lat: at.lat, lon: at.lon, km }); }}
            areas={globeAreas} radar={radar.status === 'ready' ? radar.frame.canvas : null} traffic={traffic.status === 'ready' ? traffic.lines : null} sunTime={replayAt !== null ? replayAt * 1000 : Date.now()}
            measuring={measuring} measure={measurePts} rings={rings} onSurface={addMeasure} markerScale={wall ? 1.6 : 1} flyRequest={flyRequest}
            onInteract={() => { pausedUntil.current = Date.now() + TOUR_PAUSE_MS; }}
            alerts={alertMarks} onAlertPick={pickAlertId} />
        </React.Suspense>
      )}

      {view === '2d' && region && (
        <>
          <Basemap region={region} radarUrl={radar.status === 'ready' ? radar.frame.url : undefined} traffic={traffic.status === 'ready' ? traffic.lines : undefined} utilityLines={utilityLines2d} />
          {areaTracks.length > 0 && <AreaLayer region={region} areas={areaTracks} selected={sel} />}
          <GraticuleLabels region={region} />
          <GeoRings region={region} height={canvasH} />
        </>
      )}

      {view === '2d' && region && (
        <svg aria-hidden viewBox="0 0 100 100" preserveAspectRatio="none" style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none' }}>
          {visible.map((t) => {
            // Recorded history when there is some, else the positions this session has seen.
            const hist = trails.get(t.id);
            const pts = hist
              ? hist.map(project).join(' ')
              : !replay && t.trail?.length ? [...t.trail, { x: t.x, y: t.y }].map((p) => `${p.x},${p.y}`).join(' ') : null;
            if (!pts) return null;
            const on = t.id === sel || t.id === hover;
            return (
              <polyline key={t.id} points={pts} fill="none" stroke={colourOf(t)}
                strokeOpacity={on ? 0.9 : 0.3} strokeWidth={on ? 2 : 1} vectorEffect="non-scaling-stroke" />
            );
          })}
          {selPath2d && (
            <polyline points={selPath2d.map(project).join(' ')} fill="none" stroke="var(--grey-100)" strokeOpacity={0.7} strokeWidth={1.5} strokeDasharray="4 3" vectorEffect="non-scaling-stroke" />
          )}
        </svg>
      )}

      {/* range rings + scale + north arrow (reference layout only) */}
      {view === '2d' && !region && <div style={{ position: 'absolute', inset: 0, overflow: 'hidden', pointerEvents: 'none' }}>
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

      {view === '2d' && !region && layerOn('feeds') && (
        <>
          <div style={{ position: 'absolute', left: '12%', top: '8%', width: '55%', height: '62%', border: '1px dashed var(--accent-glow)', borderRadius: '50%', pointerEvents: 'none' }} />
          <span style={{ position: 'absolute', left: '39%', top: '8%', transform: 'translate(-50%, -50%)', fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.12em', color: 'var(--accent-hover)', background: 'var(--bg-sunken)', padding: '1px 6px', pointerEvents: 'none' }}>NORTH-2 · RADAR · 4 HZ · R 60 NM</span>
        </>
      )}

      {view === '2d' && visible.map((en) => {
        const anomalous = en.track === 'anomalous';
        const selected = en.id === sel;
        const color = region ? colourOf(en) : anomalous ? 'var(--warn)' : 'var(--accent)';
        const shape = shapeOf(en);
        // Network lines are drawn by the basemap; the thousands of fixed sites (substations,
        // works, exchanges) only on the globe, close in.
        if (shape === 'line' || en.detail?.kind.startsWith('infra-')) return null;
        const rad = (en.hdg * Math.PI) / 180;
        // Live: the vector is where the track will be in 3 minutes (speed in NM/min × 3).
        // Satellites move too fast for a vector to mean anything at this scale; events do not move.
        const moves = shape === 'aircraft' || shape === 'vessel' || shape === 'balloon' || shape === 'point';
        const vlen = region ? (en.detail?.onGround || !moves ? 0 : (en.spd / 20) * pxPerNm) : Math.min(46, 12 + en.spd / 14);
        // Gulf watch: larger diamonds, ringed, always named. [FR-H-4]
        const gw = isGulfWatch(en);
        const base = gw ? (en.detail?.kind === 'event-fire-at-sea' ? 18 : 15) : region ? (shape === 'quake' ? 6 + 2 * (en.detail?.magnitude ?? 2) : shape === 'flood' ? 10 : shape === 'fire' ? 4 + Math.min(8, Math.sqrt(en.detail?.frp ?? 1) * 1.2) : shape === 'airspace' ? 4 : shape === 'windfarm' ? 11 : 7) : 14;
        const size = wall ? Math.round(base * 1.5) : base;
        // With hundreds of live tracks, label only the ones that matter right now.
        const labelled = !region || gw || selected || anomalous || en.id === hover || visible.length <= 40;
        const square = gw || en.layer === 'maritime' || shape === 'satellite' || shape === 'platform' || shape === 'road';
        return (
          <button
            key={en.id}
            onClick={() => select(en.id)}
            onMouseEnter={() => setHover(en.id)}
            onMouseLeave={() => setHover(null)}
            onFocus={() => setHover(en.id)}
            onBlur={() => setHover(null)}
            aria-label={`${en.id} · ${en.cls} · heading ${en.hdg} degrees`}
            style={{ all: 'unset', position: 'absolute', left: en.x + '%', top: en.y + '%', transform: 'translate(-50%,-50%)', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 5, cursor: 'pointer', zIndex: selected ? 3 : gw ? 2 : undefined, transition: region && !replay ? 'left 2s linear, top 2s linear' : undefined }}
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
                  position: 'absolute', inset: 0, borderRadius: square ? 2 : '50%',
                  background: shape === 'quake' || shape === 'windfarm' || shape === 'airspace' ? 'transparent' : color,
                  border: shape === 'quake' || shape === 'airspace' ? `${shape === 'airspace' ? 1 : 2}px solid ${color}` : undefined,
                  opacity: shape === 'flood' ? 0.7 : shape === 'airspace' ? 0.7 : 1,
                  transform: gw || shape === 'satellite' || shape === 'road' ? 'rotate(45deg)' : undefined,
                  // A wind farm reads as a small turbine at its centre.
                  ...(shape === 'windfarm' ? { borderRadius: 0, color, fontSize: size + 3, lineHeight: `${size}px`, textAlign: 'center' as const } : {}),
                  boxShadow: selected
                    ? `0 0 0 5px ${anomalous ? 'var(--warn-tint)' : 'var(--accent-tint)'}, 0 0 18px -2px ${anomalous ? 'var(--warn)' : 'var(--accent-glow)'}`
                    : gw ? `0 0 0 2px var(--bg-sunken), 0 0 0 4px ${color}, 0 0 16px 2px ${color}`
                    : region && shape !== 'windfarm' && shape !== 'airspace' ? `0 0 8px -1px ${color}` : 'none',
                  outline: selected ? `1px solid ${color}` : 'none', outlineOffset: 3,
                }}
              >{shape === 'windfarm' ? '✢' : null}</span>
            </span>
            {labelled && gw && (
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: wall ? 13 : 10, fontWeight: 700, letterSpacing: '0.06em', color, background: 'color-mix(in srgb, var(--bg-sunken) 85%, transparent)', padding: '1px 5px', borderRadius: 2, whiteSpace: 'nowrap', position: 'absolute', top: size + 6 }}>
                {gulfWatchName(en).toUpperCase()}
              </span>
            )}
            {labelled && !gw && (
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: wall ? 12 : 9, letterSpacing: '0.08em', color: selected ? 'var(--grey-100)' : 'var(--text-faint)', whiteSpace: 'nowrap', position: region ? 'absolute' : undefined, top: region ? size + 3 : undefined }}>
                {en.id}{anomalous ? ' ⚠' : ''}{region ? ` · ${en.alt}` : ` · ${en.hdg}°`}
              </span>
            )}
          </button>
        );
      })}

      {view === '2d' && (() => {
        const t = tracks.find((x) => x.id === hover);
        if (!t || !layerOn(t.layer)) return null;
        const rng = Math.round(Math.hypot(t.x - 50, t.y - 50) * 1.3);
        const flipY = t.y < 36;
        const d = t.detail;
        const rows: [string, string][] = region && d?.area ? [
          ['TYPE', d.airspaceType ? `${d.airspaceType.toUpperCase()}${d.icaoClass ? ` · CLASS ${d.icaoClass}` : ''}` : t.cls],
          ['LIMITS', t.alt],
          ['NAME', d.name ?? '—'],
        ] : region && d ? [
          ['TRK / SPD', String(Math.round(t.hdg)).padStart(3, '0') + '° · ' + t.spd + ' KT'],
          ['ALT', t.alt],
          ['SQUAWK', d.squawk ?? '—'],
          ['POSITION', `${(t.lat ?? d.lat).toFixed(2)}, ${(t.lon ?? d.lon).toFixed(2)}`],
          replay ? ['REPLAY', `${utc(tShown)} UTC · recorded`] : ['REPORTED', `${fmtAgo(secsAgo(d.lastSeen))} ago${d.estimatedSecs > 0 ? ' · moved on (est.)' : ''}`],
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

      {view === '2d' && focus && !region && (
        <div style={{ position: 'absolute', top: 12, right: 14, pointerEvents: 'none', textAlign: 'right', fontFamily: 'var(--font-mono)', display: 'flex', flexDirection: 'column', gap: 3 }}>
          <span style={{ fontSize: 8, letterSpacing: '0.14em', color: 'var(--accent-hover)' }}>FOCUS · {focus.id}</span>
          <span style={{ fontSize: 9, color: 'var(--text-muted)' }}>{String(focus.hdg).padStart(3, '0')}° · {focus.spd} KT · {focus.alt}</span>
          <span style={{ fontSize: 9, color: 'var(--text-faint)' }}>{focusRng} NM · {focus.feeds} FEEDS · {focus.track.toUpperCase()}</span>
        </div>
      )}

      {/* MEASURE line, range rings and an alert's place (2D). [FR-H-1] */}
      {view === '2d' && region && (measurePts.length > 0 || rings) && <ToolLayer region={region} measure={measurePts} rings={rings} />}
      {view === '2d' && region && alertMarks.map((m, i) => {
        if (m.lat < region.south || m.lat > region.north || m.lon < region.west || m.lon > region.east) return null;
        const s = markStyle(m);
        const sev = m.severity === 'critical' ? 'CRITICAL' : m.severity === 'warning' ? 'WARNING' : 'INFO';
        // Tags for critical alerts always, the rest while there are few enough to read;
        // one tag per wording (a warning covering several regions is named once).
        const tagged = m.severity === 'critical' || (alertMarks.length <= 12 && alertMarks.findIndex((x) => x.text === m.text) === i);
        return (
          <button key={m.id} type="button" title={m.title} aria-label={`Alert: ${m.title}`} onClick={() => pickAlertId(m.id)}
            style={{ all: 'unset', cursor: 'pointer', position: 'absolute', zIndex: m.severity === 'critical' ? 5 : 4, display: 'flex', alignItems: 'center', gap: 4,
              left: `${((m.lon - region.west) / (region.east - region.west)) * 100}%`, top: `${((region.north - m.lat) / (region.north - region.south)) * 100}%`,
              transform: `translate(${-s.size / 2}px, ${-s.size * 0.6}px)`, opacity: s.opacity,
              ['--mg-alert-c' as string]: s.colour, ['--mg-alert-s' as string]: `${s.pingS}s` } as React.CSSProperties}>
            <span className={`mg-alert-icon${s.pulse ? ' mg-ping' : ''}${s.flash ? ' mg-flash' : ''}`} style={{ width: s.size, height: s.size }}
              dangerouslySetInnerHTML={{ __html: alertIconSvg(s.colour, s.size) }} />
            {tagged && <span className="mg-alert-tag"><b>{sev}</b>{m.text}</span>}
          </button>
        );
      })}
      {/* The alerts on the map, listed top right (hidden while the alerts drawer is open). */}
      {replayAt === null && alertsOnMap && !status.alertsOpen && (
        <AlertsPanel marks={alertMarks} alerts={openAlerts} nameOf={alertName} onPick={pickAlert} onOpenAll={() => status.setAlertsOpen(true)}
          style={{ position: 'absolute', zIndex: 5, maxHeight: '55%', width: 'min(340px, calc(100% - 120px))', minWidth: 240,
            zoom: wall ? 1.15 : undefined, ...(view === '2d' ? { top: 56, right: 14 } : { top: 12, right: 66 }) }} />
      )}
      {replayAt === null && gulfWatchOn && gulfWatch.length > 0 && (
        <GulfWatchPanel items={gulfWatch} onPick={(t) => { select(t.id); setFlyRequest((r) => ({ n: (r?.n ?? 0) + 1, id: t.id })); }}
          style={{ position: 'absolute', zIndex: 5, maxHeight: '45%', width: 'min(330px, calc(100% - 120px))', minWidth: 230,
            zoom: wall ? 1.15 : undefined, ...(view === '2d' ? { top: 56, left: 14 } : { top: 128, left: 12 }) }} />
      )}
      <style>{ALERT_PULSE_CSS}</style>
      {/* Credits the utility sources ask for, while their layers are on. [FR-H-1] */}
      {(layerOn('utilities') || layerOn('infrastructure')) && visible.some((t) => t.layer === 'utilities' || t.layer === 'infrastructure') && (
        <div style={{ position: 'absolute', right: 10, bottom: bottomH + 6, zIndex: 4, maxWidth: 'min(560px, 70%)', textAlign: 'right',
          fontFamily: 'var(--font-mono)', fontSize: 7.5, letterSpacing: '0.04em', lineHeight: 1.5, color: 'var(--text-faint)', textShadow: '0 0 3px #000' }}>
          UTILITIES: Contains BMRS data © Elexon Limited · NESO Carbon Intensity (CC BY 4.0) · UK Power Networks (CC BY 4.0) ·{' '}
          <a href="https://northernpowergrid.opendatasoft.com/p/opendatalicence/" target="_blank" rel="noreferrer" style={{ color: 'inherit' }}>Supported by Northern Powergrid Open Data</a>{' '}
          · NGED · storm overflows via Stream (CC BY 4.0) · Environment Agency (OGL) · National Gas Transmission Data Portal · Source: GIE AGSI+ / ALSI · IODA, Georgia Tech · Cloudflare Radar (CC BY-NC 4.0) · © OpenStreetMap contributors
        </div>
      )}
      {view === '2d' && region && alertPin && (
        <span aria-hidden style={{ position: 'absolute', left: `${((alertPin.lon - region.west) / (region.east - region.west)) * 100}%`, top: `${((region.north - alertPin.lat) / (region.north - region.south)) * 100}%`, width: 26, height: 26, transform: 'translate(-50%,-50%)', border: '2px solid var(--danger)', borderRadius: '50%', boxShadow: '0 0 14px var(--danger)', pointerEvents: 'none', zIndex: 3 }} />
      )}
      {view === '2d' && region && measuring && (
        <div aria-label="Measure: click two points on the map" role="application"
          onClick={(ev) => {
            const r = ev.currentTarget.getBoundingClientRect();
            const fx = (ev.clientX - r.left) / r.width, fy = (ev.clientY - r.top) / r.height;
            addMeasure({ lat: region.north - fy * (region.north - region.south), lon: region.west + fx * (region.east - region.west) });
          }}
          style={{ position: 'absolute', inset: 0, zIndex: 3, cursor: 'crosshair' }} />
      )}

      {/* Canvas chips: radar and measure (2D top right; 3D under the MAP chip). */}
      {(view === '3d' || region) && <div style={{ position: 'absolute', zIndex: 4, display: 'flex', gap: 6, flexWrap: 'wrap', ...(view === '3d' ? { top: 92, left: 12 } : { top: 12, right: 14 }) }}>
        <Chip on={alertsOnMap} onClick={() => setAlertsOnMap(!alertsOnMap)} tone={alertsOnMap && openAlerts.some((a) => a.severity === 'critical' && !a.acknowledgedBy) ? 'var(--danger)' : undefined}
          label={alertsOnMap ? 'Hide alerts on the map' : 'Show open alerts on the map'}>ALERTS {alertMarks.length || openAlerts.length}</Chip>
        {gulfWatch.length > 0 && <Chip on={gulfWatchOn} onClick={() => setGulfWatchOn(!gulfWatchOn)} label={gulfWatchOn ? 'Hide the Gulf watch list' : 'Show the Gulf watch list'}>GULF WATCH</Chip>}
        {/* RADAR and TRAFFIC need the internet: not in the offline build (app/offline.ts). */}
        {!OFFLINE && <Chip on={radarOn} onClick={() => setRadarOn(!radarOn)} tone={radar.status === 'unavailable' ? 'var(--warn)' : undefined}
          label={radarOn ? 'Hide rain radar' : 'Show rain radar (RainViewer, needs the internet)'}>{radarLabel(radar)}</Chip>}
        {!OFFLINE && <Chip on={trafficOn} onClick={() => setTrafficOn(!trafficOn)} tone={traffic.status === 'unavailable' ? 'var(--warn)' : undefined}
          label={trafficOn ? 'Hide live traffic' : 'Show live traffic (TomTom, needs the internet): green light, amber moderate, red heavy, dark red closed'}>{trafficLabel(traffic)}</Chip>}
        {(
          <Chip on={measuring} onClick={() => { if (measuring) { setMeasuring(false); setMeasurePts([]); } else setMeasuring(true); }} label="Measure: click two points; Escape clears">
            {measuring ? (measurePts.length < 2 ? `MEASURE · CLICK POINT ${measurePts.length + 1}` : 'MEASURE · ESC CLEARS') : 'MEASURE'}
          </Chip>
        )}
      </div>}

      {status.alertsOpen && (
        <AlertsDrawer onPick={pickAlert} onClose={() => status.setAlertsOpen(false)}
          style={{ position: 'absolute', top: view === '3d' ? 12 : 54, right: view === '3d' ? 66 : 14, width: 'min(380px, calc(100% - 90px))', maxHeight: '62%', zIndex: 6, zoom: wall ? 1.15 : undefined }} />
      )}

      {/* Picture figures and the replay timeline, stacked along the bottom. [FR-P3-7] */}
      <div ref={bottomRef} style={{ position: 'absolute', left: 12, right: 12, bottom: 10, display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: 6, zIndex: 4, pointerEvents: 'none', zoom: wall ? 1.15 : undefined }}>
        {!statsOpen && (
          <button type="button" onClick={() => setStatsOpen(true)} aria-expanded={false} style={{ all: 'unset', pointerEvents: 'auto', cursor: 'pointer', minHeight: 32, display: 'inline-flex', alignItems: 'center', gap: 12, padding: '0 12px', ...mono, color: 'var(--text-muted)', background: 'color-mix(in srgb, var(--bg-sunken) 80%, transparent)', border: '1px solid var(--divider)', borderRadius: 'var(--radius-sm)' }}>
            <span>{(picture?.entities.length ?? 0).toLocaleString()} TRACKED</span>
            <span>{visible.length.toLocaleString()} {view === '3d' ? 'ON GLOBE' : 'ON CANVAS'}</span>
            <span style={{ color: 'var(--accent-hover)' }}>{tracks.filter((t) => t.track === 'anomalous').length} ANOMALIES</span>
            <span aria-hidden>▴</span>
          </button>
        )}
        {statsOpen && <div style={{ pointerEvents: 'auto', display: 'flex', alignItems: 'flex-end', gap: '8px 18px', flexWrap: 'wrap', background: 'color-mix(in srgb, var(--bg-sunken) 80%, transparent)', backdropFilter: 'var(--blur-panel)', WebkitBackdropFilter: 'var(--blur-panel)', border: '1px solid var(--divider)', borderRadius: 'var(--radius-sm)', padding: '8px 12px', width: 'fit-content', maxWidth: '100%', boxSizing: 'border-box' }}>
          {/* Counted from the picture the gateway serves, not fixed figures. [FR-H-1] [FR-H-2] */}
          <StatBlock value={(picture?.entities.length ?? 0).toLocaleString()} label="TRACKED ENTITIES" size="sm" />
          <StatBlock value={visible.length.toLocaleString()} label={view === '3d' ? 'ON GLOBE' : 'ON CANVAS'} size="sm" />
          <StatBlock value={String(tracks.filter((t) => t.track === 'anomalous').length)} label="ANOMALIES" size="sm" accent />
          {region && view === '2d' && <StatBlock value={String(allTracks.filter((t) => t.detail?.label === 'fast-mover').length)} label="FAST MOVERS" size="sm" />}
          {region && view === '2d' && <StatBlock value={String(allTracks.filter((t) => t.detail?.label === 'rotorcraft').length)} label="ROTORCRAFT" size="sm" />}
          {geoView && (
            <span style={{ display: 'flex', gap: 10, flexWrap: 'wrap', alignItems: 'center' }}>
              {[...BANDS.map((b) => ({ label: b.label, color: b.color })), ...KIND_LEGEND, ...AREA_LEGEND.filter((a) => visible.some((t) => t.detail?.kind === a.kind))].map((b) => (
                <span key={b.label} style={{ display: 'inline-flex', alignItems: 'center', gap: 5, ...mono, fontSize: 8, color: 'var(--text-faint)' }}>
                  <span style={{ width: 7, height: 7, borderRadius: '50%', background: b.color }} />{b.label}
                </span>
              ))}
            </span>
          )}
          {filter && (
            <button onClick={() => setFilter(null)} style={{ all: 'unset', cursor: 'pointer', ...mono, color: 'var(--accent-hover)' }}>FILTER: {filter} ✕</button>
          )}
          <span style={{ marginLeft: 'auto', fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.14em', color: 'var(--text-faint)', whiteSpace: 'nowrap' }}>
            {view === '3d' ? 'GLOBE: NATURAL EARTH 1:110M + 1:50M EUROPE · TERRAIN: MAPZEN TERRAIN TILES (SRTM, GMTED2010, ETOPO1)' : region ? 'MAP: NATURAL EARTH 1:50M · LINEAR LAT/LON' : 'REFERENCE LAYOUT · POSITIONS NOT TO MAP SCALE'}
          </span>
          <button type="button" onClick={() => setStatsOpen(false)} aria-label="Fold the figures" aria-expanded style={{ all: 'unset', cursor: 'pointer', ...mono, color: 'var(--text-faint)', padding: '4px 6px' }}>▾</button>
        </div>}
        <div style={{ pointerEvents: 'auto', width: '100%' }}>
          <Timeline clock={clock} history={history} windowSecs={windowSecs} setWindow={setWindowSecs} trailMins={trailMins} setTrailMins={setTrailMins} />
        </div>
      </div>
    </ConsolePage>
  );
}
