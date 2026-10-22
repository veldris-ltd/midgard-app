/* How a track is coloured and shaped, shared by the 2D canvas and the 3D globe so both
 * views read the same. Aircraft are coloured by altitude band; the other kinds each have
 * their own colour (flood warnings by severity, earthquakes by magnitude). Colours are
 * design-system tokens; the globe resolves them to RGB at run time. [FR-H-1] [FR-P3-3]
 * Area kinds (SIGMETs by hazard, weather warnings by level, wind farms, road
 * disruptions) and offshore platforms have their own shapes and colours too. */
import type { HeimdallCameraAnalysis, HeimdallTrack } from '@midgard/sdk';

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
  | 'sigmet' | 'warning' | 'windfarm' | 'platform' | 'road' | 'airspace' | 'fire'
  // utilities: power cut, water (overflow, reservoir), plant or gas site, fixed site, a
  // whole system's status, and a network line (drawn as a line, not a marker)
  | 'outage' | 'drop' | 'hexagon' | 'site' | 'hub' | 'line';

/** Utility networks' colours. [FR-H-1] */
export const NETWORK_COLOURS: Record<string, string> = { power: '#ffd23f', water: '#3fa9ff', gas: '#ff8c1a', telecom: '#b48cff' };
/** Power stations by fuel (OpenStreetMap plant:source). */
const PLANT_COLOURS: Record<string, string> = {
  wind: '#7fd3ff', solar: '#ffe066', gas: '#ff8c1a', nuclear: '#c77dff', hydro: '#3fa9ff', coal: '#9a9a9a', oil: '#9a9a9a',
  biomass: '#8bc34a', biofuel: '#8bc34a', waste: '#a1887f', battery: '#2fd16b', tidal: '#4dd0e1',
};
/** Interconnector flow direction (into GB, out of GB, idle). */
export const FLOW_COLOURS: Record<string, string> = { import: '#2fd16b', export: '#ff9f1a', idle: '#6f7f8f' };

/** A utility entity's colour from its kind and what it reports. */
export function utilityColour(d: { kind: string; network?: string; planned?: boolean; status?: string; flowing?: boolean; direction?: string; fuel?: string; voltageKv?: number; level?: string }): string {
  switch (d.kind) {
    case 'utility-power-outage': return d.planned ? '#ffd23f' : '#ff4d2e';
    case 'utility-water-overflow': return d.status === 'discharging' ? '#d9a441' : '#7f8c99';
    case 'utility-gas-terminal': case 'utility-gas-storage': case 'utility-gas-lng': return d.flowing ? '#ff8c1a' : '#8a6a4a';
    case 'utility-power-interconnector': case 'utility-gas-interconnector': return FLOW_COLOURS[d.direction ?? 'idle'] ?? FLOW_COLOURS.idle;
    case 'utility-telecom-outage': return '#ff4dd2';
    // OONI censorship: violet, magenta while it is rising above the country's previous week.
    case 'utility-telecom-censorship': return d.level === 'rising' ? '#ff4dd2' : '#9b7bff';
    case 'infra-power-plant': return PLANT_COLOURS[(d.fuel ?? '').split(';')[0].toLowerCase()] ?? NETWORK_COLOURS.power;
    default: return NETWORK_COLOURS[d.network ?? ''] ?? 'var(--text-muted)';
  }
}

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
    // EASA conflict-zone bulletins and FAA prohibitions: a warning to airlines about the airspace.
    case 'airspace-advisory': return 'var(--warn)';
    // Tsunami bulletins by category: a warning red, an advisory, watch or threat amber.
    case 'event-tsunami': return d.level === 'warning' ? 'var(--danger)' : d.level === 'advisory' || d.level === 'watch' || d.level === 'threat' ? 'var(--warn)' : 'var(--info)';
    // News topics: amber while coverage is surging.
    case 'event-news': return d.level === 'surge' ? 'var(--warn)' : 'var(--heimdall-300)';
    case 'land-road-disruption': return ROAD_COLOURS[(d.level ?? '').toLowerCase()] ?? 'var(--heimdall-300)';
    // GPS interference cells: magenta, deeper with the share of aircraft affected.
    case 'event-gnss-interference': return d.level === 'high' ? '#ff2bd6' : d.level === 'medium' ? '#c04dff' : '#8a6bff';
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
    case 'airspace-advisory': case 'event-tsunami': case 'event-news': return 'warning';
    case 'event-fire': return 'fire';
    case 'utility-power-outage': return 'outage';
    case 'utility-water-overflow': case 'utility-water-reservoir': return 'drop';
    case 'utility-gas-terminal': case 'utility-gas-storage': case 'utility-gas-lng': case 'infra-power-plant': return 'hexagon';
    case 'utility-power-grid': case 'utility-gas-system': case 'utility-telecom-outage': case 'utility-telecom-censorship': return 'hub';
    case 'utility-power-interconnector': case 'utility-gas-interconnector':
    case 'infra-power-substation': case 'infra-water-works': case 'infra-water-treatment':
    case 'infra-telecom-exchange': case 'infra-telecom-datacentre': return 'site';
    default:
      // Network lines (power lines, pipelines, cables) are drawn as lines.
      if (kind.startsWith('infra-') && t.detail?.lines) return 'line';
      return 'point';
  }
}

/** Whether a warning (or SIGMET) applies now; one issued ahead is not yet in force. */
export const inForce = (d: { validFrom?: number; validTo?: number }, now = Date.now() / 1000): boolean =>
  (d.validFrom === undefined || d.validFrom <= now) && (d.validTo === undefined || d.validTo > now);

/** "THU 06:00Z" */
export const dayTime = (s: number): string =>
  `${new Date(s * 1000).toUTCString().slice(0, 3).toUpperCase()} ${new Date(s * 1000).toISOString().slice(11, 16)}Z`;

/** A weather warning in words, with when it applies: "YELLOW WIND WARNING · FROM THU 06:00Z". */
export function warningText(d: { level?: string; weather?: string; validFrom?: number; validTo?: number }, now = Date.now() / 1000): string {
  const what = [d.level, d.weather].filter(Boolean).join(' ').toUpperCase();
  const when = d.validFrom !== undefined && d.validFrom > now ? `FROM ${dayTime(d.validFrom)}`
    : d.validTo !== undefined ? `IN FORCE TO ${dayTime(d.validTo)}` : 'IN FORCE';
  return `${what ? `${what} ` : ''}WARNING · ${when}`;
}

export type CameraLevel = 'light' | 'moderate' | 'heavy';

/** Colours by camera level, matching the road traffic colours. */
export const CAMERA_LEVEL_COLOURS: Record<CameraLevel, string> = { light: '#2fd16b', moderate: '#ffb020', heavy: '#ff3b30' };

/** An analysis older than this no longer colours its camera. */
const CAMERA_FRESH_S = 30 * 60;

/** How busy a camera's last analysis saw the road: heavy for a queue or most vehicles
 *  stopped, moderate for many vehicles or many standing, light otherwise. A still image
 *  (no movement) is judged on its count alone. Null when not analysed or stale. [FR-P3-5] */
export function cameraLevel(ai: HeimdallCameraAnalysis | undefined, now = Date.now() / 1000): CameraLevel | null {
  if (!ai || now - ai.at > CAMERA_FRESH_S) return null;
  const v = ai.vehicles ?? 0;
  if (ai.moving === undefined) return v >= 15 ? 'heavy' : v >= 7 ? 'moderate' : 'light';
  const standing = ai.stationary ?? 0;
  if (ai.queue || (v >= 5 && (ai.stopped ?? 0) >= 0.5 * v)) return 'heavy';
  if (v >= 12 || (v >= 4 && standing >= 0.4 * v)) return 'moderate';
  return 'light';
}

/** The colour (a CSS value) a track is drawn in, in either view. */
export function colourOf(t: HeimdallTrack): string {
  if (t.track === 'anomalous') return 'var(--warn)';
  switch (shapeOf(t)) {
    case 'aircraft': return bandOf(t).color;
    case 'balloon': return 'var(--mimir-300)';
    case 'airport': return FLIGHT_CATEGORY_COLOURS[t.detail?.flightCategory ?? ''] ?? 'var(--text-muted)';
    // Cameras: by how busy the analytics last saw them; pale blue when not analysed.
    case 'camera': {
      const level = cameraLevel(t.detail?.camera?.analysis);
      return level ? CAMERA_LEVEL_COLOURS[level] : '#7fd3ff';
    }
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
    case 'outage': case 'drop': case 'hexagon': case 'site': case 'hub': case 'line':
      return t.detail ? utilityColour(t.detail) : 'var(--text-muted)';
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
  // utilities [FR-H-1]
  { label: 'POWER CUT', color: '#ff4d2e', kind: 'utility-power-outage' },
  { label: 'PLANNED WORK', color: '#ffd23f', kind: 'utility-power-outage' },
  { label: 'INTERCONNECTOR IMPORT / EXPORT', color: '#2fd16b', kind: 'utility-power-interconnector' },
  { label: 'STORM OVERFLOW (NOW / 48 H)', color: '#d9a441', kind: 'utility-water-overflow' },
  { label: 'RESERVOIR', color: '#3fa9ff', kind: 'utility-water-reservoir' },
  { label: 'GAS TERMINAL / STORAGE', color: '#ff8c1a', kind: 'utility-gas-terminal' },
  { label: 'LNG TERMINAL (EUROPE)', color: '#ff8c1a', kind: 'utility-gas-lng' },
  { label: 'INTERNET DISRUPTION', color: '#ff4dd2', kind: 'utility-telecom-outage' },
  { label: 'CENSORSHIP (RISING)', color: '#9b7bff', kind: 'utility-telecom-censorship' },
  { label: 'POWER LINE 400 / 275 / 132 KV', color: '#ffd23f', kind: 'infra-power-line' },
  { label: 'SUBSTATION', color: '#ffd23f', kind: 'infra-power-substation' },
  { label: 'POWER STATION (BY FUEL)', color: '#7fd3ff', kind: 'infra-power-plant' },
  { label: 'GAS PIPELINE', color: '#ff8c1a', kind: 'infra-gas-pipeline' },
  { label: 'WATER / WASTEWATER WORKS', color: '#3fa9ff', kind: 'infra-water-works' },
  { label: 'SUBMARINE CABLE / EXCHANGE', color: '#b48cff', kind: 'infra-telecom-cable' },
  { label: 'AIRSPACE R/D/P · CTR/TMA', color: 'var(--info)', kind: 'airspace-volume' },
  { label: 'GPS INTERFERENCE', color: '#ff2bd6', kind: 'event-gnss-interference' },
  { label: 'AIRSPACE WARNING (EASA · FAA)', color: 'var(--warn)', kind: 'airspace-advisory' },
  { label: 'TSUNAMI BULLETIN', color: 'var(--danger)', kind: 'event-tsunami' },
  { label: 'NEWS TOPIC (SURGE)', color: 'var(--heimdall-300)', kind: 'event-news' },
];

/** Kinds that never move: no speed, track, trail or flight history in the inspector. */
export const isFixedKind = (kind: string) =>
  kind === 'event-gnss-interference' ||
  kind === 'land-camera' ||
  kind.startsWith('event-') || kind === 'airspace-volume' || kind === 'airspace-advisory' || kind.startsWith('utility-telecom-') || kind === 'land-airport' || kind === 'sea-windfarm' || kind === 'sea-platform' || kind === 'land-road-disruption';
