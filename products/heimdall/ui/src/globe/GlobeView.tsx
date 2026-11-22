import * as React from 'react';
import type { HeimdallLayerId, HeimdallTrack } from '@midgard/sdk';
import { useResolvedMode } from '@midgard/ui-shared';

import { TrackTooltip } from '../app/TrackTooltip';
import { MIN_ALTITUDE_KM, cameraFloorKm } from './camera.ts';
import { readGlobeColours } from './colours.ts';
import { DEFAULT_EXAGGERATION, MAX_EXAGGERATION, MIN_EXAGGERATION, clampExaggeration, tracksToGlobeItems } from './items.ts';
import { loadOutlines } from './outlineAssets.ts';
import { type Region, bearingText, buildRegions, describeCamera, regionInView } from './presets.ts';
import { type ViewState, viewLookingAt } from './view.ts';
import { type GlobeOutlines, GlobeRenderer, webglAvailable } from './renderer.ts';
import './globe.css';

/* The operating picture as a 3D globe. Loaded lazily, with three.js and the bundled
 * outlines, only when 3D is opened. It shares the 2D view's layer switches and
 * inspector selection: `layerOn` and `selectedId` come in, `onSelect` goes out.
 * [FR-H-1] [AC-H-1] */

export interface GlobeViewProps {
  tracks: readonly HeimdallTrack[];
  layerOn: (layer: HeimdallLayerId) => boolean;
  selectedId: string | null;
  onSelect: (id: string) => void;
}

const reducedMotion = () =>
  typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

const KEY_STEP_PX = 60;

export default function GlobeView({ tracks, layerOn, selectedId, onSelect }: GlobeViewProps) {
  const hostRef = React.useRef<HTMLDivElement>(null);
  const labelsRef = React.useRef<HTMLDivElement>(null);
  const tooltipRef = React.useRef<HTMLDivElement>(null);
  const rendererRef = React.useRef<GlobeRenderer | null>(null);
  const [failed, setFailed] = React.useState(() => !webglAvailable());
  const [outlines, setOutlines] = React.useState<GlobeOutlines | null>(null);
  const [loadFailed, setLoadFailed] = React.useState(false);
  const paintedMode = React.useRef<string | null>(null);
  const [hover, setHover] = React.useState<string | null>(null);
  const [heights, setHeights] = React.useState(DEFAULT_EXAGGERATION);
  const [camera, setCamera] = React.useState<ViewState | null>(null);
  const mode = useResolvedMode();
  const summaryId = React.useId();

  const regions = React.useMemo(() => buildRegions(tracks), [tracks]);
  const sector = regions[regions.length - 1];
  const items = React.useMemo(() => tracksToGlobeItems(tracks, layerOn, heights), [tracks, layerOn, heights]);
  const listed = tracks.filter((t) => layerOn(t.layer));
  const onSelectRef = React.useRef(onSelect);
  onSelectRef.current = onSelect;

  // The outlines, from the console's own files, once per mount.
  React.useEffect(() => {
    if (failed) return;
    let live = true;
    loadOutlines().then(
      (o) => { if (live) setOutlines(o); },
      () => { if (live) setLoadFailed(true); },
    );
    return () => { live = false; };
  }, [failed]);

  // Start-up: one renderer per mount, opening on SECTOR NORTH.
  React.useEffect(() => {
    if (failed || !outlines || !hostRef.current || !labelsRef.current) return;
    let r: GlobeRenderer;
    try {
      paintedMode.current = mode;
      r = new GlobeRenderer(hostRef.current, labelsRef.current, readGlobeColours(hostRef.current), outlines, {
        onHover: setHover,
        onSelect: (id) => onSelectRef.current(id),
        onCamera: (c, settled) => { if (settled) setCamera(c); },
        onContextLost: () => setFailed(true),
      });
    } catch {
      setFailed(true);
      return;
    }
    rendererRef.current = r;
    r.flyTo(sector.camera, true);
    return () => {
      rendererRef.current = null;
      r.dispose();
    };
    // Mount once per WebGL attempt; the effects below push every later change in.
  }, [failed, outlines]);

  React.useEffect(() => {
    const r = rendererRef.current;
    if (!r) return;
    r.setItems(items);
    const byId = new Map(tracks.map((t) => [t.id, t]));
    r.setLabelText((id) => {
      const t = byId.get(id);
      return t ? `${t.id}${t.track === 'anomalous' ? ' ⚠' : ''} · ${t.hdg}°` : id;
    });
    // No marker may stand above the camera or inside its near plane.
    const highest = items.reduce((m, i) => Math.max(m, i.altitudeKm), 0) * clampExaggeration(heights);
    r.setCameraFloor(cameraFloorKm(highest));
  }, [items, tracks, heights, failed, outlines]);

  React.useEffect(() => { rendererRef.current?.setSelected(selectedId); }, [selectedId, items, failed, outlines]);
  React.useEffect(() => { rendererRef.current?.setTooltipAnchor(tooltipRef.current); }, [hover, failed, outlines]);
  // Repaint only when the mode changes after start-up; the renderer paints once on its own.
  React.useEffect(() => {
    const r = rendererRef.current;
    if (!r || !hostRef.current || paintedMode.current === mode) return;
    paintedMode.current = mode;
    r.setColours(readGlobeColours(hostRef.current));
  }, [mode]);

  const flyTo = (c: ViewState) => rendererRef.current?.flyTo(c, reducedMotion());
  // Keeps the current tilt and heading, and looks at the track from behind it.
  const flyToTrack = (t: HeimdallTrack) => {
    if (typeof t.lat !== 'number' || typeof t.lon !== 'number') return;
    const now = rendererRef.current?.cameraState;
    // High enough that the marker, lifted by HEIGHTS, stays in the frame with its stem.
    const item = items.find((i) => i.id === t.id);
    const markerKm = (item?.altitudeKm ?? 0) * clampExaggeration(heights);
    const alt = Math.max(MIN_ALTITUDE_KM, markerKm * 3, Math.min(now?.altKm ?? 800, 800));
    flyTo(viewLookingAt({ lat: t.lat, lon: t.lon }, alt, now?.pitch ?? 0, now?.heading ?? 0));
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const r = rendererRef.current;
    if (!r) return;
    const k = e.key;
    // Shift and the arrows tilt (up and down) and turn (left and right).
    const orbits: Record<string, [number, number]> = {
      ArrowUp: [5, 0], ArrowDown: [-5, 0], ArrowLeft: [0, -10], ArrowRight: [0, 10],
    };
    if (e.shiftKey && orbits[k]) {
      e.preventDefault();
      r.orbit(orbits[k][0], orbits[k][1]);
      return;
    }
    const moves: Record<string, [number, number, number]> = {
      ArrowLeft: [KEY_STEP_PX, 0, 1], ArrowRight: [-KEY_STEP_PX, 0, 1],
      ArrowUp: [0, KEY_STEP_PX, 1], ArrowDown: [0, -KEY_STEP_PX, 1],
      '+': [0, 0, 0.7], '=': [0, 0, 0.7], '-': [0, 0, 1 / 0.7],
    };
    const m = moves[k];
    if (!m) return;
    e.preventDefault();
    r.nudge(m[0], m[1], m[2]);
  };

  const current: Region | null = camera ? regionInView(camera, regions) : null;
  const summary = camera ? describeCamera(camera, regions) : '';
  const hovered = hover ? tracks.find((t) => t.id === hover) : undefined;

  if (loadFailed) {
    return (
      <div className="hg-globe" role="status">
        <div className="hg-globe-unavailable">
          <strong>3D VIEW UNAVAILABLE · RELOAD TO RETRY</strong>
          <span>THE GLOBE'S OUTLINES DID NOT LOAD · THE 2D VIEW STILL WORKS</span>
        </div>
      </div>
    );
  }

  if (failed) {
    return (
      <div className="hg-globe" role="status">
        <div className="hg-globe-unavailable">
          <strong>3D VIEW UNAVAILABLE · WEBGL</strong>
          <span>THIS BROWSER GAVE THE GLOBE NO WEBGL CONTEXT · THE 2D VIEW STILL WORKS</span>
        </div>
      </div>
    );
  }

  return (
    <div className="hg-globe">
      <div
        ref={hostRef}
        className="hg-globe-surface"
        tabIndex={0}
        role="application"
        aria-roledescription="3D globe"
        aria-label="Globe. Arrow keys pan, Shift and arrows tilt and turn, plus and minus zoom. The track list reaches each track."
        aria-describedby={summaryId}
        onKeyDown={onKeyDown}
      />
      <div ref={labelsRef} className="hg-globe-labels" aria-hidden="true" />
      {!outlines && <div className="hg-globe-loading" role="status">LOADING 3D VIEW…</div>}
      {hovered && (
        <div ref={tooltipRef} style={{ position: 'absolute', left: 0, top: 0, zIndex: 5, pointerEvents: 'none', display: 'none' }}>
          <TrackTooltip track={hovered} selected={hovered.id === selectedId} style={{ position: 'relative' }} />
        </div>
      )}

      <nav className="hg-globe-panel hg-globe-nav" aria-label="Globe regions and tracks">
        <div className="hg-globe-section">
          <div className="hg-globe-heading" id={`${summaryId}-regions`}>REGION</div>
          <ul className="hg-globe-list" aria-labelledby={`${summaryId}-regions`}>
            {regions.map((r) => (
              <li key={r.id}>
                <button type="button" aria-current={current?.id === r.id ? 'true' : undefined} onClick={() => flyTo(r.camera)}>
                  {r.label}
                </button>
              </li>
            ))}
          </ul>
        </div>
        <div className="hg-globe-section grow">
          <div className="hg-globe-heading" id={`${summaryId}-tracks`}>
            <span>TRACKS</span>
            <span>{items.length} ON GLOBE</span>
          </div>
          <ul className="hg-globe-list" aria-labelledby={`${summaryId}-tracks`}>
            {listed.map((t) => {
              const placed = typeof t.lat === 'number' && typeof t.lon === 'number';
              return (
                <li key={t.id}>
                  <button
                    type="button"
                    disabled={!placed}
                    aria-current={t.id === selectedId ? 'true' : undefined}
                    aria-label={`${t.id} · ${t.cls} · ${t.alt}${t.track === 'anomalous' ? ' · anomalous' : ''}${placed ? '' : ' · no position'}`}
                    onClick={() => { onSelect(t.id); flyToTrack(t); }}
                    onMouseEnter={() => rendererRef.current?.setHovered(t.id)}
                    onMouseLeave={() => rendererRef.current?.setHovered(null)}
                    onFocus={() => rendererRef.current?.setHovered(t.id)}
                    onBlur={() => rendererRef.current?.setHovered(null)}
                  >
                    <span className="hg-globe-track">
                      <span className="tone" data-tone={t.track === 'anomalous' ? 'warn' : 'accent'} />
                      <span>{t.id}</span>
                      <span className="sub">{placed ? t.alt : 'NO POSITION'}</span>
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      </nav>

      <div className="hg-globe-panel hg-globe-side">
        <div className="hg-globe-section">
          <div className="hg-globe-heading">CAMERA</div>
          <div className="hg-globe-readout" aria-hidden="true">
            {camera ? (
              <>
                <div>ALT {Math.round(camera.altKm).toLocaleString('en-GB')} KM</div>
                <div>{Math.abs(camera.lat).toFixed(2)}° {camera.lat < 0 ? 'S' : 'N'} · {Math.abs(camera.lon).toFixed(2)}° {camera.lon < 0 ? 'W' : 'E'}</div>
                <div>TILT {Math.round(camera.pitch)}° · HDG {bearingText(camera.heading)}</div>
                <div>{camera.pitch >= 1 ? 'TOWARDS' : 'OVER'} {current?.label ?? 'OPEN GLOBE'}</div>
              </>
            ) : '—'}
          </div>
          <p id={summaryId} className="hg-sr-only" aria-live="polite">{summary}</p>
          <div className="hg-globe-zoom">
            <button type="button" aria-label="Zoom in" onClick={() => rendererRef.current?.nudge(0, 0, 0.6)}>+</button>
            <button type="button" aria-label="Zoom out" onClick={() => rendererRef.current?.nudge(0, 0, 1 / 0.6)}>−</button>
          </div>
          <div className="hg-globe-zoom hg-globe-tilt">
            <button type="button" aria-label="Tilt towards the horizon" onClick={() => rendererRef.current?.orbit(10)}>TILT +</button>
            <button type="button" aria-label="Tilt towards straight down" onClick={() => rendererRef.current?.orbit(-10)}>TILT −</button>
            <button
              type="button"
              className="hg-globe-compass"
              aria-label={`Heading ${bearingText(camera?.heading ?? 0)}. Reset to north up, looking straight down`}
              title="North up, straight down"
              onClick={() => rendererRef.current?.resetOrientation(reducedMotion())}
            >
              <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" style={{ transform: `rotate(${-(camera?.heading ?? 0)}deg)` }}>
                <path d="M12 2 L16 12 L12 10 L8 12 Z" fill="var(--warn)" />
                <path d="M12 22 L8 12 L12 14 L16 12 Z" fill="currentColor" opacity="0.6" />
              </svg>
            </button>
          </div>
        </div>
        <label className="hg-globe-section hg-globe-heights">
          <span className="hg-globe-heading"><span>HEIGHTS</span><span>×{heights}</span></span>
          <input
            type="range"
            min={MIN_EXAGGERATION}
            max={MAX_EXAGGERATION}
            step={1}
            value={heights}
            aria-valuetext={`Heights exaggerated ${heights} times`}
            onChange={(e) => setHeights(Number(e.target.value))}
          />
        </label>
      </div>
    </div>
  );
}
