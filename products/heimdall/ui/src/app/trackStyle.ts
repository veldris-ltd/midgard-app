/* How a track is coloured and shaped, shared by the 2D canvas and the 3D globe so both
 * views read the same. Aircraft are coloured by altitude band; the other kinds each have
 * their own colour (flood warnings by severity, earthquakes by magnitude). Colours are
 * design-system tokens; the globe resolves them to RGB at run time. [FR-H-1] [FR-P3-3]
 * Area kinds (SIGMETs by hazard, weather warnings by level, wind farms, road
 * disruptions) and offshore platforms have their own shapes and colours too. */
import type { HeimdallTrack } from '@midgard/sdk';

/** Altitude bands that colour aircraft. */
export const BANDS = [
  { id: 'gnd', label: 'ON GROUND', color: 'var(--text-faint)', test: (t: HeimdallTrack) => !!t.detail?.onGround },
  { id: 'low', label: 'BELOW 10,000 FT', color: 'var(--secure)', test: (t: HeimdallTrack) => (t.altFt ?? 0) < 10000 },
  { id: 'mid', label: '10,000–25,000 FT', color: 'var(--info)', test: (t: HeimdallTrack) => (t.altFt ?? 0) < 25000 },
  { id: 'high', label: 'ABOVE 25,000 FT', color: 'var(--accent)', test: () => true },
] as const;
export const bandOf = (t: HeimdallTrack) => BANDS.find((b) => b.test(t))!;

/** Flood warning severity 1 (most severe) to 4. */
export const FLOOD_COLOURS: Record<number, string> = {
  1: 'var(--danger)',
  2: 'var(--forge-500)',
  3: 'var(--warn)',
  4: 'var(--info)',
};

/** The entity kind a track is drawn as. Reference-layout tracks carry no detail, so
 *  their layer stands in. */
export type TrackShape = 'aircraft' | 'vessel' | 'train' | 'camera' | 'airport' | 'balloon' | 'satellite' | 'quake' | 'flood' | 'point'
  | 'sigmet' | 'warning' | 'windfarm' | 'platform' | 'road' | 'airspace' | 'fire';

/** openAIP airspace by type: restrictions warm, controlled airspace cool, the rest faint. */
export function airspaceColour(type: string | undefined): string {
  switch ((type ?? '').toLowerCase()) {
    case 'prohibited': return 'var(--danger)';
    case 'restricted': return 'var(--forge-300)';
    case 'danger': return 'var(--warn)';
    case 'ctr': case 'tma': case 'cta': return 'var(--info)';
    case 'atz': case 'matz': case 'tmz': case 'rmz': case 'airway': return 'var(--mimir-500)';
    default: return 'var(--text-faint)';
  }
}

/** Fire hotspots, flame-coloured. */
export const FIRE_COLOUR = 'var(--forge-500)';

/** SIGMET hazards, in colours that read as their danger. */
export const HAZARD_COLOURS: Record<string, string> = {
  TS: 'var(--danger)',
  TC: 'var(--danger)',
  TURB: 'var(--forge-300)',
  ICE: 'var(--info)',
  MTW: 'var(--mimir-300)',
  VA: 'var(--bifrost-300)',
};

/** Met Office warning levels. */
export const WARNING_COLOURS: Record<string, string> = {
  yellow: 'var(--heimdall-300)',
  amber: 'var(--warn)',
  red: 'var(--danger)',
};

/** TfL disruption severity. */
const ROAD_COLOURS: Record<string, string> = { severe: 'var(--danger)', serious: 'var(--warn)', moderate: 'var(--forge-300)' };

/** The colour an area entity (by its detail) is drawn in. */
export function areaColour(d: { kind: string; hazard?: string; level?: string; airspaceType?: string }): string {
  switch (d.kind) {
    case 'event-sigmet': return HAZARD_COLOURS[(d.hazard ?? '').toUpperCase()] ?? 'var(--heimdall-300)';
    case 'event-weather-warning': return WARNING_COLOURS[(d.level ?? '').toLowerCase()] ?? 'var(--heimdall-300)';
    case 'sea-windfarm': return 'var(--mimir-300)';
    case 'airspace-volume': return airspaceColour(d.airspaceType);
    case 'land-road-disruption': return ROAD_COLOURS[(d.level ?? '').toLowerCase()] ?? 'var(--heimdall-300)';
    default: return 'var(--text-muted)';
  }
}

export function shapeOf(t: HeimdallTrack): TrackShape {
  const kind = t.detail?.kind ?? (t.layer === 'air' ? 'air-track' : '');
  switch (kind) {
    case 'air-track': return 'aircraft';
    case 'air-balloon': return 'balloon';
    case 'sea-vessel': return 'vessel';
    case 'land-train': return 'train';
    case 'land-camera': return 'camera';
    case 'land-airport': return 'airport';
    case 'space-satellite': return 'satellite';
    case 'event-earthquake': return 'quake';
    case 'event-flood': return 'flood';
    case 'event-sigmet': return 'sigmet';
    case 'event-weather-warning': return 'warning';
    case 'sea-windfarm': return 'windfarm';
    case 'sea-platform': return 'platform';
    case 'land-road-disruption': return 'road';
    case 'airspace-volume': return 'airspace';
    case 'event-fire': return 'fire';
    default: return 'point';
  }
}

/** The colour (a CSS value) a track is drawn in, in either view. */
export function colourOf(t: HeimdallTrack): string {
  if (t.track === 'anomalous') return 'var(--warn)';
  switch (shapeOf(t)) {
    case 'aircraft': return bandOf(t).color;
    case 'balloon': return 'var(--mimir-300)';
    case 'airport': return FLIGHT_CATEGORY_COLOURS[t.detail?.flightCategory ?? ''] ?? 'var(--text-muted)';
    case 'camera': return '#7fd3ff';
    case 'train': {
      // On time (or early) green, a few minutes late amber, badly late red.
      const late = t.detail?.lateMin ?? 0;
      return late > 10 ? 'var(--danger)' : late > 2 ? 'var(--warn)' : 'var(--secure)';
    }
    case 'vessel': return VESSEL_COLOURS[t.detail?.shipType ?? ''] ?? 'var(--midgard-300)';
    case 'satellite': return t.detail?.station ? 'var(--heimdall-300)' : 'var(--bifrost-300)';
    case 'quake': return (t.detail?.magnitude ?? 0) >= 5 ? 'var(--danger)' : 'var(--warn)';
    case 'flood': return FLOOD_COLOURS[t.detail?.severityLevel ?? 4] ?? 'var(--info)';
    case 'fire': return FIRE_COLOUR;
    case 'sigmet': case 'warning': case 'windfarm': case 'road': case 'airspace': return t.detail ? areaColour(t.detail) : 'var(--text-muted)';
    case 'platform': return 'var(--forge-300)';
    default: return 'var(--text-muted)';
  }
}

/** The legend for the non-aircraft kinds, in the order the views list them. */
/** Airports by METAR flight category, in the usual aviation colours. */
export const FLIGHT_CATEGORY_COLOURS: Record<string, string> = {
  VFR: 'var(--secure)',
  MVFR: 'var(--info)',
  IFR: 'var(--danger)',
  LIFR: 'var(--mimir-500)',
};

/** Ships by AIS type: the common ones get their own colour, the rest share one. */
export const VESSEL_COLOURS: Record<string, string> = {
  cargo: 'var(--midgard-300)',
  tanker: 'var(--forge-300)',
  passenger: 'var(--heimdall-500)',
};

export const KIND_LEGEND: { label: string; color: string }[] = [
  { label: 'AIRPORT VFR', color: 'var(--secure)' },
  { label: 'MVFR', color: 'var(--info)' },
  { label: 'IFR', color: 'var(--danger)' },
  { label: 'LIFR', color: 'var(--mimir-500)' },
  { label: 'CARGO SHIP', color: 'var(--midgard-300)' },
  { label: 'TANKER', color: 'var(--forge-300)' },
  { label: 'PASSENGER SHIP', color: 'var(--heimdall-500)' },
  { label: 'BALLOON', color: 'var(--mimir-300)' },
  { label: 'SATELLITE', color: 'var(--bifrost-300)' },
  { label: 'STATION', color: 'var(--heimdall-300)' },
  { label: 'EARTHQUAKE', color: 'var(--warn)' },
  { label: 'FLOOD', color: 'var(--forge-500)' },
];

/** Legend for the area kinds, shown only when the picture has them. */
export const AREA_LEGEND: { label: string; color: string; kind: string }[] = [
  { label: 'SIGMET (TS · TURB · ICE)', color: 'var(--danger)', kind: 'event-sigmet' },
  { label: 'WARNING Y/A/R', color: 'var(--warn)', kind: 'event-weather-warning' },
  { label: 'WIND FARM', color: 'var(--mimir-300)', kind: 'sea-windfarm' },
  { label: 'PLATFORM', color: 'var(--forge-300)', kind: 'sea-platform' },
  { label: 'ROAD', color: 'var(--warn)', kind: 'land-road-disruption' },
  { label: 'FIRE', color: 'var(--forge-500)', kind: 'event-fire' },
  { label: 'AIRSPACE R/D/P · CTR/TMA', color: 'var(--info)', kind: 'airspace-volume' },
];

/** Kinds that never move: no speed, track, trail or flight history in the inspector. */
export const isFixedKind = (kind: string) =>
  kind === 'land-camera' ||
  kind.startsWith('event-') || kind === 'airspace-volume' || kind === 'land-airport' || kind === 'sea-windfarm' || kind === 'sea-platform' || kind === 'land-road-disruption';
