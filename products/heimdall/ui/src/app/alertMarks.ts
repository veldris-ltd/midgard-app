/* Open alerts drawn on the map, in both views: a ring in the severity colour at the
 * alerted entity (where it is now, if it is in the picture; else where the alert was
 * raised), pulsing while a critical or warning alert is unacknowledged. Info alerts show
 * only close in, so a wide view is not crowded with them. [FR-H-1] [FR-H-4] */
import type { HeimdallAlert } from '@midgard/sdk';

export interface AlertMark {
  id: string;
  lat: number;
  lon: number;
  severity: HeimdallAlert['severity'];
  /** The alert's label in words, e.g. "CAMERA STOPPED VEHICLE". */
  text: string;
  /** For the tooltip: label, entity and rationale. */
  title: string;
  acked: boolean;
}

export const ALERT_COLOURS: Record<HeimdallAlert['severity'], string> = {
  critical: '#ff3b30',
  warning: '#ffb020',
  info: '#5ab8ff',
};

/** Info alerts show within this many km of view only. */
export const INFO_MAX_KM = 700;

export const alertText = (label: string) => label.replace(/[-_]+/g, ' ').toUpperCase();

/** The ring's look for one mark: size, colour, pulse. */
export function markStyle(m: AlertMark): { size: number; colour: string; pulse: boolean; opacity: number } {
  return {
    size: m.severity === 'critical' ? 26 : m.severity === 'warning' ? 20 : 15,
    colour: ALERT_COLOURS[m.severity],
    pulse: !m.acked && m.severity !== 'info',
    opacity: m.acked ? 0.5 : 1,
  };
}

/** CSS for the pulse (added once to the page); still under reduced motion. */
export const ALERT_PULSE_CSS = `
@keyframes mgAlertPulse { 0% { box-shadow: 0 0 0 0 var(--mg-alert-c); } 70% { box-shadow: 0 0 0 10px transparent; } 100% { box-shadow: 0 0 0 0 transparent; } }
.mg-alert-pulse { animation: mgAlertPulse 1.6s ease-out infinite; }
@media (prefers-reduced-motion: reduce) { .mg-alert-pulse { animation: none; } }
`;
