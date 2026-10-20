/* Utility network lines (power lines by voltage, gas pipelines, submarine cables, power
 * and gas interconnectors by flow direction) grouped by look, for the globe and the 2D
 * map alike. Tracks on a layer that is switched off are simply absent. [FR-H-1] */
import type { HeimdallTrack } from '@midgard/sdk';
import { FLOW_COLOURS, NETWORK_COLOURS } from './trackStyle';
import type { InfraLineGroup } from './globe/GlobeView';

export function utilityLineGroups(tracks: HeimdallTrack[]): InfraLineGroup[] {
  const groups = new Map<string, InfraLineGroup>();
  const add = (key: string, look: Omit<InfraLineGroup, 'key' | 'lines'>, lines: [number, number][][]) => {
    const g = groups.get(key) ?? { key, ...look, lines: [] };
    g.lines.push(...lines);
    groups.set(key, g);
  };
  for (const t of tracks) {
    const d = t.detail;
    if (!d?.lines) continue;
    if (d.kind === 'infra-power-line') {
      const kv = Number(/(\d+) kV/.exec(d.name ?? '')?.[1] ?? 132);
      add(`power-${kv}`, kv >= 400 ? { colour: NETWORK_COLOURS.power, px: 1.8, opacity: 0.85 } : kv >= 275 ? { colour: '#ffb020', px: 1.5, opacity: 0.8 } : { colour: '#b8962a', px: 1.1, opacity: 0.65 }, d.lines);
    } else if (d.kind === 'infra-gas-pipeline') add('gas', { colour: NETWORK_COLOURS.gas, px: 1.4, opacity: 0.75, dashed: true }, d.lines);
    else if (d.kind === 'infra-telecom-cable') add('telecom', { colour: NETWORK_COLOURS.telecom, px: 1.2, opacity: 0.7 }, d.lines);
    else if (d.kind.endsWith('-interconnector')) {
      const dir = d.direction ?? 'idle';
      add(`link-${dir}`, { colour: FLOW_COLOURS[dir] ?? FLOW_COLOURS.idle, px: 2.8, opacity: 0.95 }, d.lines);
    }
  }
  // Thin lines under thick ones.
  return [...groups.values()].sort((a, b) => a.px - b.px);
}

/** A key that changes only when the set of lines (or a link's direction) does. */
export const utilityLineKey = (tracks: HeimdallTrack[]): string =>
  tracks.filter((t) => t.detail?.lines).map((t) => `${t.id}:${t.detail?.direction ?? ''}:${t.detail?.lines?.length}`).join('|');
