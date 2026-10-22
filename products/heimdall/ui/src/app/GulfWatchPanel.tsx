/* The Gulf watch, listed over the map's top left: the summary line, then one row per
 * indicator (coloured by status) and per new fire at sea; clicking a row selects it and
 * flies there. Derived by the gulf-watch feed from the other feeds. The panel folds to
 * its header. [FR-H-1] [FR-H-4] */
import * as React from 'react';
import type { HeimdallTrack } from '@midgard/sdk';
import { colourOf } from './trackStyle';

const mono: React.CSSProperties = { fontFamily: 'var(--font-mono)', letterSpacing: '0.06em' };
const FOLD_KEY = 'heimdall.gulfWatch.folded';

/** Gulf watch entities: the indicators and any new fire at sea. */
export const isGulfWatch = (t: HeimdallTrack) => t.detail?.kind === 'event-indicator' || t.detail?.kind === 'event-fire-at-sea';

/** "Gulf watch · Civil aircraft over Iran (few receivers…)" → "Civil aircraft over Iran". */
export const gulfWatchName = (t: HeimdallTrack) =>
  (t.detail?.name ?? t.id).replace(/^Gulf watch · /, '').replace(/\s*\(.*\)$/, '');

/** The summary without its "Not yet judged: …" tail (the rows show which are warming up). */
const summaryText = (text: string | undefined) => {
  const head = (text ?? '').split('. Not yet judged')[0].trim();
  return !head || head === 'nothing unusual' ? 'Nothing unusual.' : head.replace(/; /g, ' · ');
};

const fact = (t: HeimdallTrack, label: string) => t.detail?.facts?.find((f) => f.label === label)?.value;

export function GulfWatchPanel({ items, onPick, style }: {
  items: HeimdallTrack[];
  onPick: (t: HeimdallTrack) => void;
  style?: React.CSSProperties;
}) {
  const [folded, setFolded] = React.useState(() => { try { return localStorage.getItem(FOLD_KEY) === '1'; } catch { return false; } });
  const fold = (v: boolean) => { setFolded(v); try { localStorage.setItem(FOLD_KEY, v ? '1' : '0'); } catch { /* private window */ } };
  const summary = items.find((t) => t.detail?.kind === 'event-indicator' && gulfWatchName(t) === 'summary');
  const rows = items.filter((t) => t !== summary).sort((a, b) =>
    Number(b.detail?.kind === 'event-fire-at-sea') - Number(a.detail?.kind === 'event-fire-at-sea')
    || Number(b.detail?.level === 'elevated') - Number(a.detail?.level === 'elevated'));
  const unusual = rows.filter((t) => t.detail?.kind === 'event-fire-at-sea' || t.detail?.level === 'elevated').length;
  const judged = rows.some((t) => t.detail?.kind === 'event-fire-at-sea' || t.detail?.level === 'normal' || t.detail?.level === 'elevated');
  const tone = unusual ? 'var(--warn)' : judged ? 'var(--secure)' : 'var(--info)';
  return (
    <section aria-label="Gulf watch" style={{
      display: 'flex', flexDirection: 'column', background: 'color-mix(in srgb, var(--bg-sunken) 92%, transparent)',
      backdropFilter: 'var(--blur-panel)', WebkitBackdropFilter: 'var(--blur-panel)', border: `1px solid ${tone}`,
      borderRadius: 'var(--radius-sm)', boxShadow: 'var(--shadow-lg)', overflow: 'hidden', ...style,
    }}>
      <button type="button" onClick={() => fold(!folded)} aria-expanded={!folded}
        style={{ all: 'unset', cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 8, padding: '6px 10px', ...mono, fontSize: 9.5, fontWeight: 700, color: 'var(--text-body)', borderBottom: folded ? 'none' : '1px solid var(--divider)' }}>
        <span style={{ width: 10, height: 10, flex: 'none', background: tone, transform: 'rotate(45deg)' }} />
        <span>GULF WATCH</span>
        <span style={{ marginLeft: 'auto', display: 'flex', gap: 8, fontWeight: 600 }}>
          <span style={{ color: tone }}>{unusual ? `${unusual} UNUSUAL` : judged ? 'ALL NORMAL' : 'WARMING UP'}</span>
          <span style={{ color: 'var(--text-faint)' }}>{folded ? '▸' : '▾'}</span>
        </span>
      </button>
      {!folded && (
        <div style={{ display: 'flex', flexDirection: 'column', overflowY: 'auto' }}>
          {summary && (
            <button type="button" onClick={() => onPick(summary)}
              style={{ all: 'unset', cursor: 'pointer', ...mono, fontSize: 9, lineHeight: 1.45, color: 'var(--text-body)', padding: '6px 10px', borderBottom: '1px solid var(--divider)', overflowWrap: 'anywhere' }}>
              {summaryText(fact(summary, 'SUMMARY'))}
            </button>
          )}
          {rows.map((t) => {
            const c = colourOf(t);
            const fire = t.detail?.kind === 'event-fire-at-sea';
            const now = fact(t, 'NOW (60 MIN)'), normal = fact(t, 'NORMAL');
            const status = fire ? 'NEW FIRE' : (t.detail?.level ?? '').toUpperCase();
            return (
              <button key={t.id} type="button" onClick={() => onPick(t)} title={fact(t, 'BASED ON') ?? fact(t, 'WHY FLAGGED')}
                style={{ all: 'unset', cursor: 'pointer', display: 'flex', alignItems: 'flex-start', gap: 8, padding: '6px 10px 6px 0', borderBottom: '1px solid var(--divider)' }}>
                <span style={{ alignSelf: 'stretch', width: 3, background: c, flex: 'none' }} />
                <span style={{ width: 9, height: 9, flex: 'none', marginTop: 2, background: c, transform: 'rotate(45deg)' }} />
                <span style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', gap: 2 }}>
                  <span style={{ ...mono, fontSize: 9.5, fontWeight: 700, color: 'var(--text-body)', overflowWrap: 'anywhere' }}>{gulfWatchName(t)}</span>
                  <span style={{ ...mono, fontSize: 8.5, color: 'var(--text-muted)' }}>
                    {fire ? (fact(t, 'NEAREST SHIP') ?? '') : [now && `NOW ${now}`, normal && `NORMAL ${normal}`].filter(Boolean).join(' · ') || fact(t, 'BASED ON')}
                  </span>
                </span>
                <span style={{ ...mono, fontSize: 8.5, fontWeight: 700, color: c, whiteSpace: 'nowrap' }}>{status}</span>
              </button>
            );
          })}
        </div>
      )}
    </section>
  );
}
