/* The 2D map's overlays, in the same linear lat/lon projection as the tracks: area
 * entities (SIGMETs outlined and hatched in their hazard colour, weather warnings
 * filled by level, wind farms and airspace outlined, road disruptions), the MEASURE
 * line and the range rings round the selected track. [FR-H-1] [FR-P3-3] */
import * as React from 'react';
import type { HeimdallRegion, HeimdallTrack } from '@midgard/sdk';
import { areaColour, inForce } from './trackStyle';
import { circle, fmtMeasure, greatCircle, type LatLon } from './geoMath';

const mono: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em' };

export function projector(region: HeimdallRegion) {
  return (lat: number, lon: number): [number, number] => [
    ((lon - region.west) / (region.east - region.west)) * 100,
    ((region.north - lat) / (region.north - region.south)) * 100,
  ];
}

/** Area polygons, under the tracks. */
export function AreaLayer({ region, areas, selected }: { region: HeimdallRegion; areas: HeimdallTrack[]; selected: string }) {
  const p = projector(region);
  const hazards = [...new Set(areas.filter((a) => a.detail?.kind === 'event-sigmet').map((a) => areaColour(a.detail!)))];
  return (
    <svg aria-hidden viewBox="0 0 100 100" preserveAspectRatio="none" style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none' }}>
      <defs>
        {hazards.map((c, i) => (
          <pattern key={c} id={`hatch-${i}`} patternUnits="userSpaceOnUse" width={0.9} height={0.9} patternTransform="rotate(45)">
            <line x1={0} y1={0} x2={0} y2={0.9} stroke={c} strokeWidth={0.18} strokeOpacity={0.55} />
          </pattern>
        ))}
      </defs>
      {areas.map((a) => {
        const d = a.detail!;
        const pts = d.area!.map(([la, lo]) => p(la, lo).map((v) => v.toFixed(2)).join(',')).join(' ');
        const c = areaColour(d);
        const on = a.id === selected;
        const kind = d.kind;
        const fill = kind === 'event-sigmet' ? `url(#hatch-${hazards.indexOf(c)})`
          : kind === 'event-weather-warning' ? c
          : kind === 'land-road-disruption' ? c
          : kind === 'sea-windfarm' ? c : 'none';
        // A warning issued ahead is outlined only until it comes into force.
        const fillOpacity = kind === 'event-weather-warning' ? (inForce(d) ? 0.18 : 0.03) : kind === 'sea-windfarm' ? 0.12 : kind === 'land-road-disruption' ? 0.35 : kind === 'airspace-volume' ? 0 : 1;
        return (
          <polygon key={a.id} points={pts} fill={fill} fillOpacity={fillOpacity} stroke={c}
            strokeOpacity={on ? 1 : kind === 'airspace-volume' ? 0.45 : 0.8} strokeWidth={on ? 2 : 1}
            strokeDasharray={kind === 'event-sigmet' || (kind === 'event-weather-warning' && !inForce(d)) ? '5 3' : undefined} vectorEffect="non-scaling-stroke" />
        );
      })}
    </svg>
  );
}

/** The MEASURE line and the range rings, over the tracks. */
export function ToolLayer({ region, measure, rings }: { region: HeimdallRegion; measure: LatLon[]; rings: { centre: LatLon; nms: number[] } | null }) {
  const p = projector(region);
  const poly = (pts: LatLon[]) => pts.map((q) => p(q.lat, q.lon).map((v) => v.toFixed(3)).join(',')).join(' ');
  const f = measure.length === 2 ? fmtMeasure(measure[0], measure[1]) : null;
  const end = measure.length ? p(measure[measure.length - 1].lat, measure[measure.length - 1].lon) : null;
  return (
    <>
      <svg aria-hidden viewBox="0 0 100 100" preserveAspectRatio="none" style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none', zIndex: 2 }}>
        {rings?.nms.map((nm) => (
          <polyline key={nm} points={poly(circle(rings.centre, nm))} fill="none" stroke="var(--accent-hover)" strokeOpacity={0.7} strokeWidth={1} strokeDasharray="3 3" vectorEffect="non-scaling-stroke" />
        ))}
        {measure.length === 2 && <polyline points={poly(greatCircle(measure[0], measure[1]))} fill="none" stroke="var(--grey-100)" strokeWidth={1.5} vectorEffect="non-scaling-stroke" />}
      </svg>
      {measure.map((q, i) => {
        const [x, y] = p(q.lat, q.lon);
        return <span key={i} aria-hidden style={{ position: 'absolute', left: `${x}%`, top: `${y}%`, width: 9, height: 9, transform: 'translate(-50%,-50%)', border: '1.5px solid var(--grey-100)', borderRadius: '50%', pointerEvents: 'none', zIndex: 2 }} />;
      })}
      {rings?.nms.map((nm) => {
        const [x, y] = p(rings.centre.lat + nm / 60, rings.centre.lon);
        return <span key={nm} aria-hidden style={{ ...mono, fontSize: 8, position: 'absolute', left: `${x}%`, top: `${y}%`, transform: 'translate(-50%,-50%)', color: 'var(--accent-hover)', background: 'var(--bg-sunken)', padding: '0 4px', pointerEvents: 'none', zIndex: 2 }}>{nm} NM</span>;
      })}
      {f && end && (
        <span aria-live="polite" style={{ ...mono, fontSize: 10, position: 'absolute', left: `${end[0]}%`, top: `${end[1]}%`, transform: 'translate(10px, -130%)', color: 'var(--grey-100)', background: 'color-mix(in srgb, var(--bg-sunken) 85%, transparent)', border: '1px solid var(--border-strong)', borderRadius: 3, padding: '3px 7px', whiteSpace: 'nowrap', pointerEvents: 'none', zIndex: 5 }}>
          {f.nm} · {f.km} · {f.brg}
        </span>
      )}
    </>
  );
}
