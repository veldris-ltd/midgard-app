import * as React from 'react';
import type { HeimdallTrack } from '@midgard/sdk';

/* The track tooltip, shared by the 2D reference layout and the 3D globe so both show
 * the same rows. Each view places it; this only draws it. [FR-H-1] */

export const CAPABILITY: Record<string, string> = {
  AIRCRAFT: 'IFF · ADS-B · RADAR',
  VESSEL: 'AIS · RADAR',
  'GROUND UNIT': 'OBSERVER NET',
  UNKNOWN: 'RADAR ONLY · UNCORROBORATED',
};

/** Range from the site, from the reference layout's x/y, as the 2D view has always shown it. */
export function rangeNm(t: Pick<HeimdallTrack, 'x' | 'y'>): number {
  return Math.round(Math.hypot(t.x - 50, t.y - 50) * 1.3);
}

export function tooltipRows(t: HeimdallTrack): [string, string][] {
  return [
    ['HDG / SPD', String(t.hdg).padStart(3, '0') + '° · ' + t.spd + ' KT'],
    ['ALT', t.alt],
    ['RNG', rangeNm(t) + ' NM FROM SITE'],
    ['CAP', CAPABILITY[t.cls] ?? '—'],
    ['TRACK', t.track.toUpperCase() + ' · ' + t.feeds + ' FEEDS'],
  ];
}

export function TrackTooltip({ track: t, selected, style }: { track: HeimdallTrack; selected: boolean; style?: React.CSSProperties }) {
  return (
    <div
      role="tooltip"
      style={{
        position: 'absolute', zIndex: 5, pointerEvents: 'none', minWidth: 200,
        background: 'var(--surface-card)', border: '1px solid ' + (t.track === 'anomalous' ? 'var(--warn)' : 'var(--border-strong)'),
        borderRadius: 'var(--radius-sm)', padding: '9px 11px', boxShadow: 'var(--shadow-md)',
        ...style,
      }}
    >
      <div style={{ display: 'flex', gap: 8, alignItems: 'baseline', marginBottom: 6 }}>
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, fontWeight: 600, color: 'var(--text-heading)' }}>{t.id}</span>
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: 'var(--text-faint)' }}>{t.cls}</span>
        <span style={{ marginLeft: 'auto', fontFamily: 'var(--font-mono)', fontSize: 10, color: t.conf < 0.7 ? 'var(--warn)' : 'var(--secure)' }}>{Math.round(t.conf * 100)}%</span>
      </div>
      {tooltipRows(t).map(([k, v]) => (
        <div key={k} style={{ display: 'flex', gap: 10, padding: '2px 0', fontFamily: 'var(--font-mono)', fontSize: 9 }}>
          <span style={{ color: 'var(--text-faint)', width: 62, flex: 'none', letterSpacing: '0.08em' }}>{k}</span>
          <span style={{ color: 'var(--text-body)' }}>{v}</span>
        </div>
      ))}
      {selected && <div style={{ marginTop: 5, fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.14em', color: 'var(--accent-hover)' }}>FOCUSED · OPTIONS IN INSPECTOR →</div>}
    </div>
  );
}
