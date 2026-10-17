/* The 3D view of the operating picture: the GlobeView scene plus its React overlays
 * (camera controls, the vertical-exaggeration slider, the selected track's label and
 * the hover tooltip). Selection is the screen's, shared with the 2D canvas.
 * [FR-P3-3] [FR-P3-7] */
import * as React from 'react';
import type { HeimdallHistoryPoint, HeimdallTrack } from '@midgard/sdk';
import { colourOf, shapeOf } from '../trackStyle';
import { GlobeView, type GlobeItem, type GlobeTrail } from './GlobeView';

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

export interface Globe3DProps {
  tracks: HeimdallTrack[];
  trails: GlobeTrail[];
  selected: string;
  /** The selected track's whole recorded path, if any. */
  path: HeimdallHistoryPoint[] | null;
  hover: string | null;
  onSelect: (id: string) => void;
  onHover: (id: string | null) => void;
  /** Height (px) of the panels laid over the bottom of the canvas. */
  bottomInset?: number;
}

export function Globe3D({ tracks, trails, selected, path, hover, onSelect, onHover, bottomInset = 0 }: Globe3DProps) {
  const hostRef = React.useRef<HTMLDivElement>(null);
  const viewRef = React.useRef<GlobeView | null>(null);
  const labelRef = React.useRef<HTMLDivElement>(null);
  const tipRef = React.useRef<HTMLDivElement>(null);
  const compassRef = React.useRef<HTMLSpanElement>(null);
  const [exaggeration, setExaggeration] = React.useState(() => readNumber(EXAGGERATION_KEY, 15));
  const [follow, setFollow] = React.useState(false);
  const [tilted, setTilted] = React.useState(true);
  const [failed, setFailed] = React.useState<string | null>(null);
  // Callbacks read through a ref so the scene is built once.
  const live = React.useRef({ onSelect, onHover, selected, hover });
  live.current = { onSelect, onHover, selected, hover };

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
      });
    } catch (err) {
      setFailed(err instanceof Error ? err.message : String(err));
      return;
    }
    viewRef.current = view;
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
    magnitude: t.detail.magnitude,
  }] : [])), [tracks]);

  React.useEffect(() => { viewRef.current?.setItems(items); }, [items]);
  React.useEffect(() => { viewRef.current?.setInsets(bottomInset); }, [bottomInset]);
  React.useEffect(() => { viewRef.current?.setTrails(trails); }, [trails]);
  const sel = tracks.find((t) => t.id === selected);
  const selColour = sel ? colourOf(sel) : undefined;
  React.useEffect(() => { viewRef.current?.setSelected(selected || null, path, selColour); }, [selected, path, selColour]);
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
      <div ref={hostRef} style={{ position: 'absolute', inset: 0, touchAction: 'none', cursor: hover ? 'pointer' : 'grab' }} />
      {failed && (
        <div role="alert" style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', ...mono, color: 'var(--warn)' }}>
          3D VIEW UNAVAILABLE · WEBGL: {failed}
        </div>
      )}

      {/* Selected track label and hover tooltip, positioned each frame by `overlays`. */}
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
            {([
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
        <Control label={tilted ? 'Look straight down' : 'Oblique view'} onClick={() => view()?.toggleTilt()} active={tilted}>
          {tilted ? '3D' : '2D'}
        </Control>
        <Control label="Follow the selected track" onClick={() => setFollow((f) => !f)} active={follow && !!selected}>
          <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden><circle cx="9" cy="9" r="5" fill="none" stroke="currentColor" strokeWidth="1.5" /><circle cx="9" cy="9" r="1.6" fill="currentColor" /><path d="M9 0v3M9 15v3M0 9h3M15 9h3" stroke="currentColor" strokeWidth="1.5" /></svg>
        </Control>
      </div>

      {/* Vertical exaggeration, labelled so the heights are never read as true scale. */}
      <label style={{ position: 'absolute', top: 12, left: 12, zIndex: 4, display: 'flex', flexDirection: 'column', gap: 4, padding: '8px 10px', background: 'color-mix(in srgb, var(--bg-sunken) 85%, transparent)', border: '1px solid var(--warn)', borderRadius: 'var(--radius-sm)', ...mono }}>
        <span style={{ color: 'var(--warn)' }}>VERTICAL ×{exaggeration} · EXAGGERATED BELOW 100 KM</span>
        <input
          type="range" min={1} max={50} step={1} value={exaggeration}
          aria-label="Vertical exaggeration below 100 km"
          onChange={(e) => setExaggeration(Number(e.target.value))}
          style={{ width: 220, height: 28, margin: 0, accentColor: 'var(--warn)', touchAction: 'none' }}
        />
        <span style={{ color: 'var(--text-faint)', fontSize: 8 }}>SATELLITES AND ABOVE 100 KM AT TRUE SCALE</span>
      </label>
    </div>
  );
}
