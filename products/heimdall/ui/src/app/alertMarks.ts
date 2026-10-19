/* Open alerts drawn on the map, in both views: a ring in the severity colour at the
 * alerted entity (where it is now, if it is in the picture; else where the alert was
 * raised), pulsing while a critical or warning alert is unacknowledged; info alerts are
 * smaller and steady. [FR-H-1] [FR-H-4] */
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
  /** A colour of its own (a weather warning's level), instead of the severity's. */
  colour?: string;
}

/** Weather warning levels, as the Met Office and Meteoalarm colour them. */
export const WARNING_LEVEL_COLOURS: Record<string, string> = { yellow: '#ffd23f', amber: '#ff9f1a', orange: '#ff8c1a', red: '#ff3b30' };

export const ALERT_COLOURS: Record<HeimdallAlert['severity'], string> = {
  critical: '#ff3b30',
  warning: '#ffb020',
  info: '#5ab8ff',
};

/** Alert names show beside their rings within this many km of view (critical always). */
export const LABEL_MAX_KM = 4500;

export const alertText = (label: string) => label.replace(/[-_]+/g, ' ').toUpperCase();

/** The look of one mark: icon size, colour, whether it pings (and how fast), whether it
 *  flashes (an unacknowledged critical alert), and how strongly it shows. */
export function markStyle(m: AlertMark): { size: number; colour: string; pulse: boolean; pingS: number; flash: boolean; opacity: number } {
  return {
    size: m.severity === 'critical' ? 28 : m.severity === 'warning' ? 24 : 19,
    colour: m.colour ?? ALERT_COLOURS[m.severity],
    pulse: !m.acked,
    pingS: m.severity === 'critical' ? 1.1 : m.severity === 'warning' ? 1.6 : 2.8,
    flash: !m.acked && m.severity === 'critical',
    opacity: m.acked ? 0.55 : 1,
  };
}

/** The alert icon: a hazard triangle with "!", in the severity colour on a dark fill. */
export function alertIconSvg(colour: string, size: number): string {
  return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" aria-hidden="true" style="display:block;overflow:visible;filter:drop-shadow(0 0 3px ${colour}) drop-shadow(0 0 1px #000)">`
    + `<path d="M12 2.2 L22.6 20.6 Q23 21.6 21.9 21.6 L2.1 21.6 Q1 21.6 1.4 20.6 Z" fill="rgba(10,12,14,0.88)" stroke="${colour}" stroke-width="2" stroke-linejoin="round"/>`
    + `<rect x="10.8" y="8" width="2.4" height="7.4" rx="1.2" fill="${colour}"/>`
    + `<circle cx="12" cy="18.2" r="1.45" fill="${colour}"/></svg>`;
}

/** CSS for the marks (added once to the page): the ping ring, the critical flash and
 *  the label tag; still under reduced motion. */
export const ALERT_PULSE_CSS = `
.mg-alert-icon { position: relative; display: block; flex: none; }
.mg-alert-icon.mg-ping::before { content: ''; position: absolute; left: 50%; top: 58%; width: 100%; height: 100%; border-radius: 50%;
  border: 2px solid var(--mg-alert-c); transform: translate(-50%, -50%) scale(0.6); opacity: 0;
  animation: mgAlertPing var(--mg-alert-s, 1.6s) ease-out infinite; pointer-events: none; }
@keyframes mgAlertPing { 0% { transform: translate(-50%, -50%) scale(0.6); opacity: 0.95; } 100% { transform: translate(-50%, -50%) scale(2.6); opacity: 0; } }
.mg-alert-icon.mg-flash svg { animation: mgAlertFlash 0.9s steps(2, jump-none) infinite; }
@keyframes mgAlertFlash { 0% { opacity: 1; } 100% { opacity: 0.35; } }
.mg-alert-tag { display: inline-flex; align-items: center; gap: 6px; white-space: nowrap; font-family: var(--font-mono); font-size: 9px;
  font-weight: 700; letter-spacing: 0.08em; padding: 2px 7px 2px 0; border-radius: 2px; background: rgba(8, 10, 12, 0.86);
  border: 1px solid var(--mg-alert-c); color: #f2f2f2; box-shadow: 0 0 8px rgba(0,0,0,0.6); }
.mg-alert-tag::before { content: ''; align-self: stretch; width: 4px; background: var(--mg-alert-c); }
.mg-alert-tag b { color: var(--mg-alert-c); font-weight: 800; }
@media (prefers-reduced-motion: reduce) { .mg-alert-icon.mg-ping::before, .mg-alert-icon.mg-flash svg { animation: none; } }
`;
