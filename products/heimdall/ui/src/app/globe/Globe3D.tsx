/* The 3D view of the operating picture: the GlobeView scene plus its React overlays
 * (camera controls, the vertical-exaggeration slider, the selected track's label and
 * the hover tooltip). Selection is the screen's, shared with the 2D canvas.
 * [FR-P3-3] [FR-P3-7]
 * Also hands the scene its overlays (areas, radar, day/night time, the MEASURE line and
 * range rings) and takes camera requests (fly to an alert, the wall display's tour).
 * [FR-H-1] */
import * as React from 'react';
import type { HeimdallHistoryPoint, HeimdallTrack } from '@midgard/sdk';
import { CAMERA_LEVEL_COLOURS, cameraLevel, colourOf, shapeOf } from '../trackStyle';
import type { TrafficLines } from '../traffic';
import { PLACES } from '../places';
import { LABEL_MAX_KM, markStyle, type AlertMark } from '../alertMarks';
import { GlobeView, REGIONS, type GlobeArea, type GlobeItem, type GlobeTrail, type MapStyle, type PresetId } from './GlobeView';
import { fmtMeasure, type LatLon } from '../geoMath';
import { STRETCH_KM } from './geo';

const mono: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em' };
const EXAGGERATION_KEY = 'heimdall.cop.exaggeration';

function readNumber(key: string, fallback: number): number {
  try {
    const v = Number(window.localStorage.getItem(key));
    return Number.isFinite(v) && v > 0 ? v : fallback;
  } catch {
    return fallback;
  }
}

/** A large (44 px) round control on the canvas. */
function Control({ label, onClick, active, children }: { label: string; onClick: () => void; active?: boolean; children: React.ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={active}
      onClick={onClick}
      style={{
        all: 'unset', boxSizing: 'border-box', width: 44, height: 44, display: 'flex', alignItems: 'center', justifyContent: 'center',
        cursor: 'pointer', borderRadius: 'var(--radius-sm)', touchAction: 'manipulation',
        background: active ? 'var(--surface-raised)' : 'color-mix(in srgb, var(--bg-sunken) 85%, transparent)',
        border: `1px solid ${active ? 'var(--accent)' : 'var(--border)'}`,
        color: active ? 'var(--accent-hover)' : 'var(--text-body)', ...mono, fontSize: 11,
      }}
    >
      {children}
    </button>
  );
}

const IMAGERY_KEY = 'heimdall.globe.imagery';

/** Place names over the globe: the largest places first, as many as fit without
 *  overlapping, with smaller places admitted as the camera comes closer. Spans are
 *  reused between frames. [FR-P3-3] */
function drawPlaceLabels(host: HTMLDivElement, view: GlobeView): void {
  const km = view.state.dist * 6371;
  const minPop = km > 2500 ? 1_000_000 : km > 1400 ? 250_000 : km > 700 ? 80_000 : km > 350 ? 30_000 : km > 150 ? 12_000 : 5_000;
  const boxes: [number, number, number, number][] = [];
  const shown: { name: string; x: number; y: number; big: boolean; capital: boolean }[] = [];
  for (const [name, lat, lon, pop, capital] of PLACES) {
    if (pop < minPop || shown.length >= 140) break;
    const at = view.screenOfPoint(lat, lon);
    if (!at) continue;
    const big = pop >= 400_000 || capital === 1;
    const w = name.length * (big ? 7.4 : 6.4) + 10, h = big ? 16 : 14;
    const b: [number, number, number, number] = [at.x - w / 2, at.y - h - 4, at.x + w / 2, at.y + 4];
    if (boxes.some((o) => b[0] < o[2] && b[2] > o[0] && b[1] < o[3] && b[3] > o[1])) continue;
    boxes.push(b);
    shown.push({ name, x: at.x, y: at.y, big, capital: capital === 1 });
  }
  while (host.childElementCount < shown.length) {
    const s = document.createElement('span');
    s.style.cssText = 'position:absolute;left:0;top:0;white-space:nowrap;font-family:var(--font-mono);letter-spacing:0.06em;text-shadow:0 0 3px #000,0 0 6px #000;';
    host.appendChild(s);
  }
  for (let i = 0; i < host.childElementCount; i++) {
    const s = host.children[i] as HTMLSpanElement;
    const p = shown[i];
    if (!p) { s.style.display = 'none'; continue; }
    s.style.display = 'block';
    const text = p.name.toUpperCase();
    if (s.textContent !== text) s.textContent = text;
    s.style.fontSize = p.big ? '11px' : '9.5px';
    s.style.fontWeight = p.big ? '600' : '400';
    s.style.color = p.capital ? '#f4e2b8' : p.big ? '#e6dcc8' : '#b9b2a4';
    s.style.transform = `translate(${Math.round(p.x)}px, ${Math.round(p.y)}px) translate(-50%, -125%)`;
  }
}
/** Close in, each analysed camera's counts beside it in its level colour. [FR-P3-5] */
function drawCameraLabels(host: HTMLDivElement, view: GlobeView, tracks: HeimdallTrack[]): void {
  const km = view.state.dist * 6371;
  const shown: { text: string; x: number; y: number; colour: string }[] = [];
  if (km < 250) {
    for (const t of tracks) {
      if (shown.length >= 80) break;
      const ai = t.detail?.camera?.analysis;
      const level = cameraLevel(ai);
      if (!ai || !level || t.lat === undefined || t.lon === undefined) continue;
      const at = view.screenOfPoint(t.lat, t.lon);
      if (!at) continue;
      const text = `${ai.vehicles ?? 0}V${ai.people ? ` ${ai.people}P` : ''}${ai.queue ? ' Q' : ''}`;
      shown.push({ text, x: at.x, y: at.y, colour: CAMERA_LEVEL_COLOURS[level] });
    }
  }
  while (host.childElementCount < shown.length) {
    const s = document.createElement('span');
    s.style.cssText = 'position:absolute;left:0;top:0;white-space:nowrap;font-family:var(--font-mono);font-size:9px;font-weight:600;letter-spacing:0.04em;padding:0 3px;border-radius:2px;background:rgba(6,10,14,0.72);';
    host.appendChild(s);
  }
  for (let i = 0; i < host.childElementCount; i++) {
    const s = host.children[i] as HTMLSpanElement;
    const p = shown[i];
    if (!p) { s.style.display = 'none'; continue; }
    s.style.display = 'block';
    if (s.textContent !== p.text) s.textContent = p.text;
    s.style.color = p.colour;
    s.style.border = `1px solid ${p.colour}`;
    s.style.transform = `translate(${Math.round(p.x)}px, ${Math.round(p.y)}px) translate(8px, -50%)`;
  }
}

/** Open alerts as rings on the globe, critical and warning everywhere, info close in;
 *  the label shows beside a ring close in, or always for a critical one. Clicking a ring
 *  hands its alert id to `pick`. [FR-H-1] [FR-H-4] */
function drawAlertMarks(host: HTMLDivElement, view: GlobeView, marks: AlertMark[], pick: (id: string) => void): void {
  const km = view.state.dist * 6371;
  const shown: { m: AlertMark; x: number; y: number; label: boolean }[] = [];
  // Labels that would overlap one already placed are left off (most severe first).
  const boxes: [number, number, number, number][] = [];
  for (const m of marks) {
    if (shown.length >= 150) break;
    const at = view.screenOfPoint(m.lat, m.lon);
    if (!at) continue;
    let label = km < LABEL_MAX_KM || m.severity === 'critical';
    if (label) {
      const b: [number, number, number, number] = [at.x + 8, at.y - 8, at.x + 16 + m.text.length * 6.2, at.y + 8];
      label = !boxes.some((o) => b[0] < o[2] && b[2] > o[0] && b[1] < o[3] && b[3] > o[1]);
      if (label) boxes.push(b);
    }
    shown.push({ m, x: at.x, y: at.y, label });
  }
  while (host.childElementCount < shown.length) {
    const el = document.createElement('button');
    el.type = 'button';
    el.style.cssText = 'all:unset;position:absolute;left:0;top:0;cursor:pointer;pointer-events:auto;display:flex;align-items:center;gap:5px;';
    const ring = document.createElement('span');
    ring.style.cssText = 'display:block;border-radius:50%;border:2px solid;box-sizing:border-box;flex:none;';
    const label = document.createElement('span');
    label.style.cssText = 'font-family:var(--font-mono);font-size:9px;font-weight:600;letter-spacing:0.06em;white-space:nowrap;text-shadow:0 0 3px #000,0 0 6px #000;';
    el.append(ring, label);
    el.addEventListener('click', (ev) => { ev.stopPropagation(); if (el.dataset.id) pick(el.dataset.id); });
    host.appendChild(el);
  }
  for (let i = 0; i < host.childElementCount; i++) {
    const el = host.children[i] as HTMLButtonElement;
    const p = shown[i];
    if (!p) { el.style.display = 'none'; continue; }
    const s = markStyle(p.m);
    const ring = el.children[0] as HTMLSpanElement, label = el.children[1] as HTMLSpanElement;
    el.style.display = 'flex';
    el.dataset.id = p.m.id;
    el.title = p.m.title;
    el.setAttribute('aria-label', `Alert: ${p.m.title}`);
    el.style.opacity = String(s.opacity);
    ring.style.width = ring.style.height = `${s.size}px`;
    ring.style.borderColor = s.colour;
    ring.style.background = `color-mix(in srgb, ${s.colour} 18%, transparent)`;
    ring.style.setProperty('--mg-alert-c', s.colour);
    ring.className = s.pulse ? 'mg-alert-pulse' : '';
    const text = p.label ? p.m.text : '';
    if (label.textContent !== text) label.textContent = text;
    label.style.color = s.colour;
    el.style.transform = `translate(${Math.round(p.x - s.size / 2)}px, ${Math.round(p.y - s.size / 2)}px)`;
  }
}

/** The stored map style; tactical unless the browser chose another. */
export const readMapStyle = (): MapStyle => {
  try {
    const v = window.localStorage.getItem(IMAGERY_KEY);
    return v === 'satellite' || v === 'dark' ? v : 'tactical';
  } catch {
    return 'tactical';
  }
};

export interface Globe3DProps {
  tracks: HeimdallTrack[];
  trails: GlobeTrail[];
  selected: string;
  /** The selected track's whole recorded path, if any. */
  path: HeimdallHistoryPoint[] | null;
  /** The recorded path of the track under the pointer, shown before any click. */
  hoverPath?: HeimdallHistoryPoint[] | null;
  /** Long trails: dim the markers so the paths lead. */
  pathsFirst?: boolean;
  hover: string | null;
  onSelect: (id: string) => void;
  onHover: (id: string | null) => void;
  /** Height (px) of the panels laid over the bottom of the canvas. */
  bottomInset?: number;
  /** Areas to draw (SIGMET and airspace volumes, draped fills). Rebuilt only when the set
   *  of ids changes. */
  areas?: GlobeArea[];
  /** The radar image over the terrain box, or null. */
  radar?: HTMLCanvasElement | null;
  /** Live traffic lines for the UK overview, or null. */
  traffic?: TrafficLines | null;
  /** The time the night side is shaded for (unix ms): now, or the replay time. */
  sunTime?: number;
  /** The ground under the mouse (null off the globe), and the point the camera looks at. */
  onPointerGround?: (at: LatLon | null) => void;
  /** `km`: the camera's distance from that point, a measure of how much is in view. */
  onLookAt?: (at: LatLon, km: number) => void;
  /** A region picked from the REGION menu (so its data can be switched on). */
  onRegionPick?: (preset: PresetId) => void;
  /** Open alerts to ring on the globe, and what a click on one does. [FR-H-4] */
  alerts?: AlertMark[];
  onAlertPick?: (id: string) => void;
  /** MEASURE: surface taps go to `onSurface` instead of selecting. */
  measuring?: boolean;
  measure?: LatLon[];
  rings?: { centre: LatLon; nms: number[] } | null;
  onSurface?: (at: LatLon) => void;
  /** Wall display: larger markers. */
  markerScale?: number;
  /** A one-off camera request; `n` changes for each new request. */
  flyRequest?: { n: number; id?: string; lat?: number; lon?: number; preset?: PresetId } | null;
  /** Any direct input on the globe (pauses the tour). */
  onInteract?: () => void;
}

export function Globe3D({ tracks, trails, selected, path, hoverPath = null, pathsFirst = false, hover, onSelect, onHover, bottomInset = 0, areas = [], radar = null, traffic = null, sunTime, measuring = false, measure = [], rings = null, onSurface, markerScale = 1, flyRequest = null, onInteract, onPointerGround, onLookAt, onRegionPick, alerts, onAlertPick }: Globe3DProps) {
  const measureRef = React.useRef<HTMLDivElement>(null);
  const ringLabelRefs = React.useRef<(HTMLSpanElement | null)[]>([]);
  const hostRef = React.useRef<HTMLDivElement>(null);
  const viewRef = React.useRef<GlobeView | null>(null);
  const labelRef = React.useRef<HTMLDivElement>(null);
  const placesRef = React.useRef<HTMLDivElement>(null);
  const cameraLabelsRef = React.useRef<HTMLDivElement>(null);
  const alertMarksRef = React.useRef<HTMLDivElement>(null);
  const lookAtRef = React.useRef<(LatLon & { km: number }) | null>(null);
  const tipRef = React.useRef<HTMLDivElement>(null);
  const compassRef = React.useRef<HTMLSpanElement>(null);
  const [exaggeration, setExaggeration] = React.useState(() => readNumber(EXAGGERATION_KEY, 4));
  const [follow, setFollow] = React.useState(false);
  const [tilted, setTilted] = React.useState(true);
  const [heightsOpen, setHeightsOpen] = React.useState(false);
  const [regionsOpen, setRegionsOpen] = React.useState(false);
  // Map style (tactical by default, satellite or dark), remembered per browser.
  const [mapStyle, setMapStyleState] = React.useState<MapStyle>(readMapStyle);
  const nextStyle = () => {
    const next: MapStyle = mapStyle === 'tactical' ? 'satellite' : mapStyle === 'satellite' ? 'dark' : 'tactical';
    setMapStyleState(next);
    try { window.localStorage.setItem(IMAGERY_KEY, next); } catch { /* storage blocked: still switches */ }
  };
  const imageryRef = React.useRef(mapStyle);
  imageryRef.current = mapStyle;
  React.useEffect(() => { viewRef.current?.setMapStyle(mapStyle); }, [mapStyle]);
  const [failed, setFailed] = React.useState<string | null>(null);
  // Callbacks read through a ref so the scene is built once.
  const live = React.useRef({ onSelect, onHover, selected, hover, onSurface, onInteract, measure, rings, onPointerGround, onLookAt, tracks, alerts, onAlertPick });
  live.current = { onSelect, onHover, selected, hover, onSurface, onInteract, measure, rings, onPointerGround, onLookAt, tracks, alerts, onAlertPick };

  /** Move the label, tooltip and compass to follow the camera (after every frame). */
  const overlays = React.useCallback(() => {
    const view = viewRef.current;
    if (!view) return;
    const place = (el: HTMLElement | null, id: string | null) => {
      if (!el) return;
      const at = id ? view.screenOf(id) : null;
      el.style.display = at ? 'block' : 'none';
      if (at) el.style.transform = `translate(${Math.round(at.x)}px, ${Math.round(at.y)}px)`;
    };
    place(labelRef.current, live.current.selected || null);
    place(tipRef.current, live.current.hover && live.current.hover !== live.current.selected ? live.current.hover : null);
    if (compassRef.current) compassRef.current.style.transform = `rotate(${-view.state.heading}deg)`;
    // The point the camera looks at, reported when it moves by more than ~0.2° or the
    // zoom changes by more than 15 %.
    const c = view.state;
    const km = c.dist * 6371;
    const last = lookAtRef.current;
    if (!last || Math.abs(last.lat - c.lat) + Math.abs(last.lon - c.lon) > 0.2 || Math.abs(Math.log(km / last.km)) > 0.15) {
      lookAtRef.current = { lat: c.lat, lon: c.lon, km };
      live.current.onLookAt?.({ lat: c.lat, lon: c.lon }, km);
    }
    if (placesRef.current) drawPlaceLabels(placesRef.current, view);
    if (cameraLabelsRef.current) drawCameraLabels(cameraLabelsRef.current, view, live.current.tracks);
    if (alertMarksRef.current) drawAlertMarks(alertMarksRef.current, view, live.current.alerts ?? [], (id) => live.current.onAlertPick?.(id));
    // The measured distance sits at the line's far end; ring labels at each ring's north.
    const m = live.current.measure;
    if (measureRef.current) {
      const at = m.length === 2 ? view.screenOfPoint(m[1].lat, m[1].lon) : null;
      measureRef.current.style.display = at ? 'block' : 'none';
      if (at) measureRef.current.style.transform = `translate(${Math.round(at.x)}px, ${Math.round(at.y)}px)`;
    }
    const r = live.current.rings;
    ringLabelRefs.current.forEach((el, i) => {
      if (!el) return;
      const nm = r?.nms[i];
      const at = r && nm !== undefined ? view.screenOfPoint(r.centre.lat + nm / 60, r.centre.lon) : null;
      el.style.display = at ? 'block' : 'none';
      if (at) el.style.transform = `translate(${Math.round(at.x)}px, ${Math.round(at.y)}px) translate(-50%, -50%)`;
    });
    setTilted(view.state.tilt > 5);
  }, []);

  React.useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let view: GlobeView;
    try {
      view = new GlobeView(host, {
        onSelect: (id) => live.current.onSelect(id),
        onHover: (id) => live.current.onHover(id),
        onFrame: overlays,
        onFollowEnd: () => setFollow(false),
        onSurface: (at) => live.current.onSurface?.(at),
        onPointerGround: (at) => live.current.onPointerGround?.(at),
        onInteract: () => live.current.onInteract?.(),
      });
    } catch (err) {
      setFailed(err instanceof Error ? err.message : String(err));
      return;
    }
    viewRef.current = view;
    view.setMapStyle(imageryRef.current);
    return () => {
      view.dispose();
      viewRef.current = null;
    };
  }, [overlays]);

  const items = React.useMemo<GlobeItem[]>(() => tracks.flatMap((t) => (t.detail ? [{
    id: t.id,
    shape: shapeOf(t),
    lat: t.lat ?? t.detail.lat,
    lon: t.lon ?? t.detail.lon,
    altM: t.detail.onGround ? 0 : (t.detail.altM ?? (t.altFt !== undefined ? t.altFt / 3.28084 : null)),
    hdg: t.hdg,
    colour: colourOf(t),
    station: t.detail.station,
    magnitude: t.detail.kind === 'event-fire' ? t.detail.frp : t.detail.magnitude,
    runways: t.detail.runways,
  }] : [])).map((it) => {
    // A SIGMET's marker floats mid-volume, where it can be picked.
    const t = tracks.find((x) => x.id === it.id);
    const d = t?.detail;
    if (it.shape === 'sigmet' && d) return { ...it, altM: (((d.baseFt ?? 0) + (d.topFt ?? (d.baseFt ?? 0) + 10000)) / 2) / 3.28084 };
    return it;
  }), [tracks]);

  const itemsRef = React.useRef(items);
  itemsRef.current = items;
  React.useEffect(() => { viewRef.current?.setItems(items); }, [items]);
  // Choosing an airport (on the globe or with Find) flies in over its runways.
  React.useEffect(() => {
    if (selected && itemsRef.current.find((it) => it.id === selected)?.shape === 'airport') viewRef.current?.flyClose(selected);
  }, [selected]);
  React.useEffect(() => { viewRef.current?.setInsets(bottomInset); }, [bottomInset]);
  // Areas rebuild only when the set changes (1,300 airspaces are not rebuilt every redraw).
  const areasRef = React.useRef(areas);
  areasRef.current = areas;
  const areaSig = areas.map((a) => a.id).join('|');
  React.useEffect(() => { viewRef.current?.setAreas(areasRef.current); }, [areaSig]);
  React.useEffect(() => { viewRef.current?.setRadar(radar); }, [radar]);
  React.useEffect(() => { viewRef.current?.setTraffic(traffic); }, [traffic]);
  // Shade for the minute (a frame per minute is plenty for the terminator).
  const sunMinute = Math.floor((sunTime ?? Date.now()) / 60000);
  React.useEffect(() => { viewRef.current?.setSunTime(sunMinute * 60000); }, [sunMinute]);
  React.useEffect(() => { viewRef.current?.setPickSurface(measuring); }, [measuring]);
  const ringSig = rings ? `${rings.centre.lat.toFixed(4)},${rings.centre.lon.toFixed(4)},${rings.nms.join(',')}` : '';
  const measureSig = measure.map((p) => `${p.lat.toFixed(5)},${p.lon.toFixed(5)}`).join(';');
  React.useEffect(() => { viewRef.current?.setMeasure(live.current.measure, live.current.rings); overlays(); }, [measureSig, ringSig, overlays]);
  React.useEffect(() => { viewRef.current?.setMarkerScale(markerScale); }, [markerScale]);
  React.useEffect(() => {
    const v = viewRef.current;
    if (!v || !flyRequest) return;
    if (flyRequest.preset) v.flyToPreset(flyRequest.preset);
    else if (flyRequest.id && itemsRef.current.some((it) => it.id === flyRequest.id)) v.flyToItem(flyRequest.id);
    else if (flyRequest.lat !== undefined && flyRequest.lon !== undefined) v.flyToPoint(flyRequest.lat, flyRequest.lon);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [flyRequest?.n]);
  React.useEffect(() => { viewRef.current?.setTrails(trails); }, [trails]);
  const sel = tracks.find((t) => t.id === selected);
  const selColour = sel ? colourOf(sel) : undefined;
  React.useEffect(() => { viewRef.current?.setSelected(selected || null, path, selColour); }, [selected, path, selColour]);
  const hoverColour = React.useMemo(() => { const t = tracks.find((x) => x.id === hover); return t ? colourOf(t) : undefined; }, [tracks, hover]);
  React.useEffect(() => { viewRef.current?.setHoverPath(hoverPath, hoverColour); }, [hoverPath, hoverColour]);
  React.useEffect(() => { viewRef.current?.setPathsFirst(pathsFirst); }, [pathsFirst]);
  React.useEffect(() => {
    viewRef.current?.setExaggeration(exaggeration);
    try { window.localStorage.setItem(EXAGGERATION_KEY, String(exaggeration)); } catch { /* storage blocked */ }
  }, [exaggeration]);
  React.useEffect(() => { viewRef.current?.setFollow(follow && !!selected); }, [follow, selected]);
  React.useEffect(() => { if (!selected) setFollow(false); }, [selected]);
  // A new hover or selection moves its overlay before the next frame.
  React.useLayoutEffect(() => { overlays(); }, [hover, selected, overlays]);

  const tip = hover && hover !== selected ? tracks.find((t) => t.id === hover) : undefined;
  const view = () => viewRef.current;

  return (
    <div style={{ position: 'absolute', inset: 0 }}>
      <div ref={hostRef} style={{ position: 'absolute', inset: 0, touchAction: 'none', cursor: measuring ? 'crosshair' : hover ? 'pointer' : 'grab' }} />
      {failed && (
        <div role="alert" style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', ...mono, color: 'var(--warn)' }}>
          3D VIEW UNAVAILABLE · WEBGL: {failed}
        </div>
      )}

      {/* Selected track label and hover tooltip, positioned each frame by `overlays`. */}
      {/* Town and city names, thinned by zoom so they never overlap. */}
      <div ref={placesRef} aria-hidden style={{ position: 'absolute', inset: 0, pointerEvents: 'none', zIndex: 2, overflow: 'hidden' }} />
      <div ref={cameraLabelsRef} aria-hidden style={{ position: 'absolute', inset: 0, pointerEvents: 'none', zIndex: 2, overflow: 'hidden' }} />
      <div ref={alertMarksRef} style={{ position: 'absolute', inset: 0, pointerEvents: 'none', zIndex: 3, overflow: 'hidden' }} />
      <div ref={labelRef} aria-hidden style={{ position: 'absolute', left: 0, top: 0, display: 'none', pointerEvents: 'none', zIndex: 3 }}>
        {sel && (
          <span style={{ position: 'absolute', left: 14, top: -9, whiteSpace: 'nowrap', ...mono, fontSize: 10, color: 'var(--grey-100)', background: 'color-mix(in srgb, var(--bg-sunken) 75%, transparent)', padding: '2px 6px', borderRadius: 3, border: `1px solid ${selColour}` }}>
            {sel.id}{sel.track === 'anomalous' ? ' ⚠' : ''} · {sel.alt}
          </span>
        )}
      </div>
      <div ref={tipRef} role="tooltip" style={{ position: 'absolute', left: 0, top: 0, display: 'none', pointerEvents: 'none', zIndex: 5 }}>
        {tip && (
          <div style={{ position: 'absolute', left: 14, top: 10, minWidth: 180, background: 'var(--surface-card)', border: `1px solid ${tip.track === 'anomalous' ? 'var(--warn)' : 'var(--border-strong)'}`, borderRadius: 'var(--radius-sm)', padding: '8px 10px', boxShadow: 'var(--shadow-md)' }}>
            <div style={{ display: 'flex', gap: 8, alignItems: 'baseline', marginBottom: 4 }}>
              <span style={{ ...mono, fontSize: 11, fontWeight: 600, color: 'var(--text-heading)' }}>{tip.detail?.name && tip.detail.kind !== 'air-track' ? tip.detail.name : tip.id}</span>
              <span style={{ ...mono, color: 'var(--text-faint)' }}>{tip.cls}</span>
            </div>
            {(tip.detail?.area ? [
              // Areas: what it is and its vertical limits. [FR-H-1]
              ['TYPE', tip.detail.airspaceType ? `${tip.detail.airspaceType.toUpperCase()}${tip.detail.icaoClass ? ` · CLASS ${tip.detail.icaoClass}` : ''}` : tip.cls],
              ['LIMITS', tip.alt],
            ] : [
              ['ALT', tip.alt],
              ['TRK / SPD', tip.spd ? `${String(Math.round(tip.hdg)).padStart(3, '0')}° · ${tip.spd.toLocaleString()} KT` : '—'],
              ['POSITION', tip.detail ? `${tip.detail.lat.toFixed(2)}, ${tip.detail.lon.toFixed(2)}` : '—'],
            ] as [string, string][]).map(([k, v]) => (
              <div key={k} style={{ display: 'flex', gap: 8, ...mono, padding: '1px 0' }}>
                <span style={{ color: 'var(--text-faint)', width: 64 }}>{k}</span>
                <span style={{ color: 'var(--text-body)' }}>{v}</span>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* The MEASURE result and range ring labels, placed each frame by `overlays`. */}
      <div ref={measureRef} aria-live="polite" style={{ position: 'absolute', left: 0, top: 0, display: 'none', pointerEvents: 'none', zIndex: 4 }}>
        {measure.length === 2 && (() => { const f = fmtMeasure(measure[0], measure[1]); return (
          <span style={{ position: 'absolute', left: 10, top: -26, whiteSpace: 'nowrap', ...mono, fontSize: 10, color: 'var(--grey-100)', background: 'color-mix(in srgb, var(--bg-sunken) 85%, transparent)', border: '1px solid var(--border-strong)', borderRadius: 3, padding: '3px 7px' }}>
            {f.nm} · {f.km} · {f.brg}
          </span>
        ); })()}
      </div>
      {(rings?.nms ?? []).map((nm, i) => (
        <span key={nm} ref={(el) => { ringLabelRefs.current[i] = el; }} aria-hidden style={{ position: 'absolute', left: 0, top: 0, display: 'none', pointerEvents: 'none', zIndex: 3, ...mono, fontSize: 8, color: 'var(--accent-hover)', background: 'color-mix(in srgb, var(--bg-sunken) 75%, transparent)', padding: '0 4px', whiteSpace: 'nowrap' }}>{nm} NM</span>
      ))}

      {/* Camera controls. */}
      <div style={{ position: 'absolute', top: 12, right: 12, display: 'flex', flexDirection: 'column', gap: 6, zIndex: 4 }}>
        <Control label="Zoom in" onClick={() => view()?.zoomBy(0.6)}>＋</Control>
        <Control label="Zoom out" onClick={() => view()?.zoomBy(1 / 0.6)}>－</Control>
        <Control label="North up" onClick={() => view()?.northUp()}>
          <span ref={compassRef} style={{ display: 'inline-flex', flexDirection: 'column', alignItems: 'center', lineHeight: 1 }}>
            <span style={{ color: 'var(--danger)', fontSize: 12 }}>▲</span>
            <span style={{ fontSize: 8 }}>N</span>
          </span>
        </Control>
        <Control label="Frame the UK and Ireland" onClick={() => { setFollow(false); view()?.home(); }}>UK</Control>
        <Control label="Fly to a region" onClick={() => setRegionsOpen((o) => !o)} active={regionsOpen}><span style={{ fontSize: 9 }}>REGION</span></Control>
        {regionsOpen && (
          <div role="menu" style={{ position: 'absolute', right: 52, top: 150, display: 'flex', flexDirection: 'column', gap: 2, padding: 6, minWidth: 190, background: 'color-mix(in srgb, var(--bg-sunken) 92%, transparent)', border: '1px solid var(--divider)', borderRadius: 'var(--radius-sm)' }}>
            {REGIONS.map(([label, id]) => (
              <button key={id} role="menuitem" type="button" onClick={() => { setFollow(false); setRegionsOpen(false); onRegionPick?.(id); view()?.flyToPreset(id, 1800); }}
                style={{ all: 'unset', cursor: 'pointer', ...mono, fontSize: 10, color: 'var(--text-body)', padding: '9px 10px', borderRadius: 'var(--radius-sm)' }}>{label}</button>
            ))}
          </div>
        )}
        <Control label={tilted ? 'Look straight down' : 'Oblique view'} onClick={() => view()?.toggleTilt()} active={tilted}>
          {tilted ? '3D' : '2D'}
        </Control>
        <Control label="Follow the selected track" onClick={() => setFollow((f) => !f)} active={follow && !!selected}>
          <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden><circle cx="9" cy="9" r="5" fill="none" stroke="currentColor" strokeWidth="1.5" /><circle cx="9" cy="9" r="1.6" fill="currentColor" /><path d="M9 0v3M9 15v3M0 9h3M15 9h3" stroke="currentColor" strokeWidth="1.5" /></svg>
        </Control>
      </div>

      {/* Vertical exaggeration, labelled so the heights are never read as true scale:
          folded to one chip until asked for. */}
      <button type="button" onClick={nextStyle} aria-label={`Map style: ${mapStyle}; switch style`}
        style={{ all: 'unset', cursor: 'pointer', position: 'absolute', top: 52, left: 12, zIndex: 4, minHeight: 32, padding: '0 10px', display: 'inline-flex', alignItems: 'center', ...mono, color: 'var(--text-muted)', background: 'color-mix(in srgb, var(--bg-sunken) 85%, transparent)', border: '1px solid var(--divider)', borderRadius: 'var(--radius-sm)' }}>
        MAP · {mapStyle.toUpperCase()}
      </button>
      {!heightsOpen && (
        <button type="button" onClick={() => setHeightsOpen(true)} aria-expanded={false} aria-label={`Heights stretched ×${exaggeration}; change`}
          style={{ all: 'unset', cursor: 'pointer', position: 'absolute', top: 12, left: 12, zIndex: 4, minHeight: 32, padding: '0 10px', display: 'inline-flex', alignItems: 'center', ...mono, color: 'var(--warn)', background: 'color-mix(in srgb, var(--bg-sunken) 85%, transparent)', border: '1px solid var(--divider)', borderRadius: 'var(--radius-sm)' }}>
          HEIGHTS ×{exaggeration}
        </button>
      )}
      {heightsOpen && <label style={{ position: 'absolute', top: 12, left: 12, zIndex: 4, display: 'flex', flexDirection: 'column', gap: 4, padding: '8px 10px', background: 'color-mix(in srgb, var(--bg-sunken) 85%, transparent)', border: '1px solid var(--warn)', borderRadius: 'var(--radius-sm)', ...mono }}>
        <span style={{ color: 'var(--warn)' }}>VERTICAL ×{exaggeration} · HEIGHTS STRETCHED NEAR THE SURFACE</span>
        <input
          type="range" min={1} max={50} step={1} value={exaggeration}
          aria-label="Vertical exaggeration below 100 km"
          onChange={(e) => setExaggeration(Number(e.target.value))}
          style={{ width: 220, height: 28, margin: 0, accentColor: 'var(--warn)', touchAction: 'none' }}
        />
        <span style={{ color: 'var(--text-faint)', fontSize: 8 }}>ORDER KEPT · SATELLITES RAISED BY UP TO {(exaggeration - 1) * STRETCH_KM} KM</span>
        <button type="button" onClick={() => setHeightsOpen(false)} style={{ all: 'unset', cursor: 'pointer', alignSelf: 'flex-end', color: 'var(--text-faint)', padding: '4px 0' }}>DONE</button>
      </label>}
    </div>
  );
}
