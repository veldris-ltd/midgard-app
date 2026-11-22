import * as React from 'react';
import type { HeimdallLayerId, HeimdallTrack } from '@midgard/sdk';
import { useResolvedMode } from '@midgard/ui-shared';

import { TrackTooltip } from '../app/TrackTooltip';
import { loadOutlines } from '../globe/outlineAssets.ts';
import { type Region, buildRegions, regionAt } from '../globe/presets.ts';
import type { LoaderState } from '../tiles/loader.ts';
import { type MapCamera, MAP_MAX_ZOOM, MAP_MIN_ZOOM, zoomForAltitude } from './mapCamera.ts';
import { MapRenderer } from './mapRenderer.ts';
import '../globe/globe.css';
import './map.css';

/* The operating picture on a real map: the second 2D mode, beside the reference
 * layout. VersaTiles vector tiles (OpenStreetMap data) over the bundled Natural Earth
 * land, with the tracks at their positions. It shares the layer switches and the
 * inspector selection with the other views. Loaded lazily, and only in a build with
 * the tiles switch on. [FR-H-1] [AC-H-1] */

export interface MapViewProps {
  tracks: readonly HeimdallTrack[];
  layerOn: (layer: HeimdallLayerId) => boolean;
  selectedId: string | null;
  onSelect: (id: string) => void;
}

const reducedMotion = () =>
  typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

const KEY_STEP_PX = 80;

function regionCamera(r: Region, viewportPx: number): MapCamera {
  const zoom = zoomForAltitude(r.camera.altKm, r.camera.lat, Math.max(200, viewportPx));
  return { lat: r.camera.lat, lon: r.camera.lon, zoom: Math.min(MAP_MAX_ZOOM, Math.max(MAP_MIN_ZOOM, zoom)) };
}

const fmt = (lat: number, lon: number) =>
  `${Math.abs(lat).toFixed(2)}° ${lat < 0 ? 'S' : 'N'} · ${Math.abs(lon).toFixed(2)}° ${lon < 0 ? 'W' : 'E'}`;

export default function MapView({ tracks, layerOn, selectedId, onSelect }: MapViewProps) {
  const hostRef = React.useRef<HTMLDivElement>(null);
  const markersRef = React.useRef<HTMLDivElement>(null);
  const rendererRef = React.useRef<MapRenderer | null>(null);
  const [camera, setCamera] = React.useState<MapCamera | null>(null);
  const [tileState, setTileState] = React.useState<LoaderState>('online');
  const [hover, setHover] = React.useState<string | null>(null);
  const [failed, setFailed] = React.useState(false);
  const mode = useResolvedMode();
  const summaryId = React.useId();

  const regions = React.useMemo(() => buildRegions(tracks), [tracks]);
  const sector = regions[regions.length - 1];
  const listed = tracks.filter((t) => layerOn(t.layer));
  const placed = listed.filter((t) => typeof t.lat === 'number' && typeof t.lon === 'number');
  const onSelectRef = React.useRef(onSelect);
  onSelectRef.current = onSelect;

  React.useEffect(() => {
    const host = hostRef.current;
    const markers = markersRef.current;
    if (!host || !markers) return;
    let r: MapRenderer;
    try {
      r = new MapRenderer(host, markers, null, {
        onCamera: (c, settled) => { if (settled) setCamera(c); },
        onState: setTileState,
      });
    } catch {
      setFailed(true);
      return;
    }
    rendererRef.current = r;
    r.flyTo(regionCamera(sector, r.viewportHeight), true);
    let live = true;
    // The bundled land, from the console's own files, under the tiles.
    loadOutlines().then((o) => { if (live) r.setLand(o.close); }, () => {});
    return () => {
      live = false;
      rendererRef.current = null;
      r.dispose();
    };
    // One renderer per mount; the effects below push every later change in.
  }, []);

  React.useEffect(() => { rendererRef.current?.refreshColours(); }, [mode]);
  // The markers change: place them before the next frame.
  React.useEffect(() => { rendererRef.current?.requestRender(); }, [placed.length, layerOn, selectedId]);

  const flyTo = (c: MapCamera) => rendererRef.current?.flyTo(c, reducedMotion());
  const flyToTrack = (t: HeimdallTrack) => {
    if (typeof t.lat !== 'number' || typeof t.lon !== 'number') return;
    const z = Math.max(rendererRef.current?.camera.zoom ?? 8, 8);
    flyTo({ lat: t.lat, lon: t.lon, zoom: z });
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const r = rendererRef.current;
    if (!r) return;
    const moves: Record<string, [number, number, number]> = {
      ArrowLeft: [KEY_STEP_PX, 0, 0], ArrowRight: [-KEY_STEP_PX, 0, 0],
      ArrowUp: [0, KEY_STEP_PX, 0], ArrowDown: [0, -KEY_STEP_PX, 0],
      '+': [0, 0, 0.5], '=': [0, 0, 0.5], '-': [0, 0, -0.5],
    };
    const m = moves[e.key];
    if (!m) return;
    e.preventDefault();
    r.nudge(m[0], m[1], m[2]);
  };

  const current = camera ? regionAt(camera.lat, camera.lon, regions) : null;
  const summary = camera ? `Map at zoom ${camera.zoom.toFixed(1)}, centred on ${fmt(camera.lat, camera.lon).replace(' · ', ', ')}, over ${current?.label ?? 'open map'}.` : '';
  const hovered = hover ? tracks.find((t) => t.id === hover) : undefined;
  const hoverAt = hovered && typeof hovered.lat === 'number' && typeof hovered.lon === 'number' ? rendererRef.current?.project(hovered.lat, hovered.lon) : undefined;

  if (failed) {
    return (
      <div className="hg-globe" role="status">
        <div className="hg-globe-unavailable">
          <strong>MAP VIEW UNAVAILABLE</strong>
          <span>THIS BROWSER GAVE THE MAP NO 2D CANVAS · THE REFERENCE LAYOUT STILL WORKS</span>
        </div>
      </div>
    );
  }

  return (
    <div className="hg-globe hg-map">
      <div
        ref={hostRef}
        className="hg-globe-surface"
        tabIndex={0}
        role="application"
        aria-roledescription="map"
        aria-label="Map. Arrow keys pan, plus and minus zoom. The track list reaches each track."
        aria-describedby={summaryId}
        onKeyDown={onKeyDown}
      />
      <div ref={markersRef} className="hg-map-markers">
        {placed.map((t) => {
          const warn = t.track === 'anomalous';
          return (
            // Pointer targets only: the track list gives keyboard and screen-reader access.
            <div
              key={t.id}
              aria-hidden="true"
              className="hg-map-marker"
              data-lat={t.lat}
              data-lon={t.lon}
              data-layer={t.layer}
              data-tone={warn ? 'warn' : 'accent'}
              data-selected={t.id === selectedId}
              onClick={() => onSelect(t.id)}
              onMouseEnter={() => setHover(t.id)}
              onMouseLeave={() => setHover(null)}
            >
              <span className="dot" style={{ transform: `translate(-50%, -50%) rotate(${t.layer === 'ground' ? 45 : 0}deg)` }} />
              <span className="vec" style={{ transform: `rotate(${t.hdg}deg)` }} />
              <span className="label">{t.id}{warn ? ' ⚠' : ''} · {t.hdg}°</span>
            </div>
          );
        })}
      </div>
      {tileState === 'offline' && <div className="hg-globe-notice" role="status">MAP TILES NEED A CONNECTION</div>}
      {hovered && hoverAt && (
        <TrackTooltip
          track={hovered}
          selected={hovered.id === selectedId}
          style={{ left: Math.max(110, hoverAt.x), top: hoverAt.y, transform: hoverAt.y < 150 ? 'translate(-50%, 22px)' : 'translate(-50%, calc(-100% - 18px))', zIndex: 5 }}
        />
      )}

      <nav className="hg-globe-panel hg-globe-nav" aria-label="Map regions and tracks">
        <div className="hg-globe-section">
          <div className="hg-globe-heading" id={`${summaryId}-regions`}>REGION</div>
          <ul className="hg-globe-list" aria-labelledby={`${summaryId}-regions`}>
            {regions.map((r) => (
              <li key={r.id}>
                <button
                  type="button"
                  aria-current={current?.id === r.id ? 'true' : undefined}
                  onClick={() => flyTo(regionCamera(r, rendererRef.current?.viewportHeight ?? 600))}
                >
                  {r.label}
                </button>
              </li>
            ))}
          </ul>
        </div>
        <div className="hg-globe-section grow">
          <div className="hg-globe-heading" id={`${summaryId}-tracks`}>
            <span>TRACKS</span>
            <span>{placed.length} ON MAP</span>
          </div>
          <ul className="hg-globe-list" aria-labelledby={`${summaryId}-tracks`}>
            {listed.map((t) => {
              const has = typeof t.lat === 'number' && typeof t.lon === 'number';
              return (
                <li key={t.id}>
                  <button
                    type="button"
                    disabled={!has}
                    aria-current={t.id === selectedId ? 'true' : undefined}
                    aria-label={`${t.id} · ${t.cls} · ${t.alt}${t.track === 'anomalous' ? ' · anomalous' : ''}${has ? '' : ' · no position'}`}
                    onClick={() => { onSelect(t.id); flyToTrack(t); }}
                    onMouseEnter={() => setHover(t.id)}
                    onMouseLeave={() => setHover(null)}
                    onFocus={() => setHover(t.id)}
                    onBlur={() => setHover(null)}
                  >
                    <span className="hg-globe-track">
                      <span className="tone" data-tone={t.track === 'anomalous' ? 'warn' : 'accent'} />
                      <span>{t.id}</span>
                      <span className="sub">{has ? t.alt : 'NO POSITION'}</span>
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
          <div className="hg-globe-heading">MAP</div>
          <div className="hg-globe-readout" aria-hidden="true">
            {camera ? (
              <>
                <div>ZOOM {camera.zoom.toFixed(1)}</div>
                <div>{fmt(camera.lat, camera.lon)}</div>
                <div>OVER {current?.label ?? 'OPEN MAP'}</div>
              </>
            ) : '—'}
          </div>
          <p id={summaryId} className="hg-sr-only" aria-live="polite">{summary}</p>
          <div className="hg-globe-zoom">
            <button type="button" aria-label="Zoom in" onClick={() => rendererRef.current?.nudge(0, 0, 1)}>+</button>
            <button type="button" aria-label="Zoom out" onClick={() => rendererRef.current?.nudge(0, 0, -1)}>−</button>
          </div>
        </div>
      </div>
    </div>
  );
}
