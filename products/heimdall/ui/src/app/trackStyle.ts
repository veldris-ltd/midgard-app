/* How a track is coloured and shaped, shared by the 2D canvas and the 3D globe so both
 * views read the same. Aircraft are coloured by altitude band; the other kinds each have
 * their own colour (flood warnings by severity, earthquakes by magnitude). Colours are
 * design-system tokens; the globe resolves them to RGB at run time. [FR-H-1] [FR-P3-3] */
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
export type TrackShape = 'aircraft' | 'balloon' | 'satellite' | 'quake' | 'flood' | 'point';

export function shapeOf(t: HeimdallTrack): TrackShape {
  const kind = t.detail?.kind ?? (t.layer === 'air' ? 'air-track' : '');
  switch (kind) {
    case 'air-track': return 'aircraft';
    case 'air-balloon': return 'balloon';
    case 'space-satellite': return 'satellite';
    case 'event-earthquake': return 'quake';
    case 'event-flood': return 'flood';
    default: return 'point';
  }
}

/** The colour (a CSS value) a track is drawn in, in either view. */
export function colourOf(t: HeimdallTrack): string {
  if (t.track === 'anomalous') return 'var(--warn)';
  switch (shapeOf(t)) {
    case 'aircraft': return bandOf(t).color;
    case 'balloon': return 'var(--mimir-300)';
    case 'satellite': return t.detail?.station ? 'var(--heimdall-300)' : 'var(--bifrost-300)';
    case 'quake': return (t.detail?.magnitude ?? 0) >= 5 ? 'var(--danger)' : 'var(--warn)';
    case 'flood': return FLOOD_COLOURS[t.detail?.severityLevel ?? 4] ?? 'var(--info)';
    default: return 'var(--text-muted)';
  }
}

/** The legend for the non-aircraft kinds, in the order the views list them. */
export const KIND_LEGEND: { label: string; color: string }[] = [
  { label: 'BALLOON', color: 'var(--mimir-300)' },
  { label: 'SATELLITE', color: 'var(--bifrost-300)' },
  { label: 'STATION', color: 'var(--heimdall-300)' },
  { label: 'EARTHQUAKE', color: 'var(--warn)' },
  { label: 'FLOOD', color: 'var(--forge-500)' },
];
