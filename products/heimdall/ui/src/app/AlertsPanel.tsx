/* The open alerts on the map, listed over its top right: one row per alert, or per
 * warning where one covers several regions, most severe first, then newest. Each row
 * has the alert's triangle, what it is and where, and its age; clicking it selects its
 * subject and flies there. The panel folds to its header. [FR-H-1] [FR-H-4] */
import * as React from 'react';
import type { HeimdallAlert } from '@midgard/sdk';
import { alertIconSvg, markStyle, type AlertMark } from './alertMarks';
import { agoText } from './alerts';

const mono: React.CSSProperties = { fontFamily: 'var(--font-mono)', letterSpacing: '0.06em' };
const RANK = { critical: 0, warning: 1, info: 2 } as const;
const SHOW = 6;
const FOLD_KEY = 'heimdall.alertsPanel.folded';

interface Row {
  mark: AlertMark;
  alert: HeimdallAlert;
  /** Where: the subjects of the alerts in the row ("South West England", "Wales"…). */
  places: string[];
  newest: number;
  count: number;
}

const ll = (lat: number, lon: number) =>
  `${Math.abs(lat).toFixed(1)}°${lat >= 0 ? 'N' : 'S'} ${Math.abs(lon).toFixed(1)}°${lon >= 0 ? 'E' : 'W'}`;

/** "Yellow warning of wind affecting South West England" → "South West England"; an
 *  unnamed subject (a GPS interference cell) by where it is. */
const placeOf = (name: string | undefined, m: AlertMark) =>
  (name?.match(/affecting (.+)$/i)?.[1] ?? name ?? ll(m.lat, m.lon)).trim();

export function AlertsPanel({ marks, alerts, nameOf, onPick, onOpenAll, style }: {
  marks: AlertMark[];
  alerts: HeimdallAlert[];
  /** The alerted entity's name, where it is in the picture. */
  nameOf: (a: HeimdallAlert) => string | undefined;
  onPick: (a: HeimdallAlert) => void;
  onOpenAll: () => void;
  style?: React.CSSProperties;
}) {
  const [folded, setFolded] = React.useState(() => { try { return window.localStorage.getItem(FOLD_KEY) === '1'; } catch { return false; } });
  const fold = (v: boolean) => { setFolded(v); try { window.localStorage.setItem(FOLD_KEY, v ? '1' : '0'); } catch { /* still folds */ } };

  const rows = React.useMemo(() => {
    const byId = new Map(alerts.map((a) => [a.id, a]));
    const groups = new Map<string, Row>();
    for (const m of marks) {
      const a = byId.get(m.id);
      if (!a) continue;
      const key = `${m.severity}|${m.text}`;
      const place = placeOf(nameOf(a), m);
      const g = groups.get(key);
      if (!g) groups.set(key, { mark: m, alert: a, places: [place], newest: a.raisedAt, count: 1 });
      else {
        g.count++;
        if (!g.places.includes(place)) g.places.push(place);
        if (a.raisedAt > g.newest) { g.newest = a.raisedAt; g.alert = a; }
        if (!m.acked) g.mark = { ...g.mark, acked: false };
      }
    }
    return [...groups.values()].sort((x, y) => RANK[x.mark.severity] - RANK[y.mark.severity] || y.newest - x.newest);
  }, [marks, alerts, nameOf]);

  const counts = { critical: 0, warning: 0, info: 0 };
  for (const r of rows) counts[r.mark.severity] += r.count;
  const worst = rows[0]?.mark;

  return (
    <section aria-label="Alerts on the map" style={{
      display: 'flex', flexDirection: 'column', background: 'color-mix(in srgb, var(--bg-sunken) 90%, transparent)',
      backdropFilter: 'var(--blur-panel)', WebkitBackdropFilter: 'var(--blur-panel)', border: `1px solid ${worst ? markStyle(worst).colour : 'var(--divider)'}`,
      borderRadius: 'var(--radius-sm)', boxShadow: 'var(--shadow-lg)', overflow: 'hidden', ...style,
    }}>
      <button type="button" onClick={() => fold(!folded)} aria-expanded={!folded}
        style={{ all: 'unset', cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 8, padding: '6px 10px', ...mono, fontSize: 9.5, fontWeight: 700, color: 'var(--text-body)', borderBottom: folded ? 'none' : '1px solid var(--divider)' }}>
        {worst && <span style={{ width: 14, height: 14, flex: 'none' }} dangerouslySetInnerHTML={{ __html: alertIconSvg(markStyle(worst).colour, 14) }} />}
        <span>ALERTS IN VIEW</span>
        <span style={{ marginLeft: 'auto', display: 'flex', gap: 8, fontWeight: 600 }}>
          {counts.critical > 0 && <span style={{ color: '#ff3b30' }}>{counts.critical} CRIT</span>}
          {counts.warning > 0 && <span style={{ color: '#ffb020' }}>{counts.warning} WARN</span>}
          {counts.info > 0 && <span style={{ color: '#5ab8ff' }}>{counts.info} INFO</span>}
          {rows.length === 0 && <span style={{ color: 'var(--text-faint)' }}>NONE</span>}
          <span style={{ color: 'var(--text-faint)' }}>{folded ? '▸' : '▾'}</span>
        </span>
      </button>
      {!folded && (
        <div style={{ display: 'flex', flexDirection: 'column', overflowY: 'auto' }}>
          {rows.length === 0 && <div style={{ ...mono, fontSize: 9, color: 'var(--text-faint)', padding: '8px 10px' }}>NO OPEN ALERTS IN THE REGIONS SHOWN</div>}
          {rows.slice(0, SHOW).map((r) => {
            const s = markStyle(r.mark);
            return (
              <button key={`${r.mark.severity}|${r.mark.text}`} type="button" onClick={() => onPick(r.alert)} title={r.alert.rationale}
                style={{ all: 'unset', cursor: 'pointer', display: 'flex', alignItems: 'flex-start', gap: 8, padding: '6px 10px 6px 0', borderBottom: '1px solid var(--divider)', opacity: r.mark.acked ? 0.6 : 1 }}>
                <span style={{ alignSelf: 'stretch', width: 3, background: s.colour, flex: 'none' }} />
                <span style={{ width: 16, height: 16, flex: 'none', marginTop: 1 }} dangerouslySetInnerHTML={{ __html: alertIconSvg(s.colour, 16) }} />
                <span style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', gap: 2 }}>
                  <span style={{ ...mono, fontSize: 9.5, fontWeight: 700, color: 'var(--text-body)', overflowWrap: 'anywhere' }}>{r.mark.text}</span>
                  <span style={{ ...mono, fontSize: 8.5, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {r.places.slice(0, 3).join(' · ')}{r.places.length > 3 ? ` +${r.places.length - 3}` : ''}
                  </span>
                </span>
                <span style={{ ...mono, fontSize: 8.5, color: 'var(--text-faint)', whiteSpace: 'nowrap', textAlign: 'right' }}>
                  {agoText(r.newest).toUpperCase()}{r.mark.acked ? <><br />ACK</> : null}
                </span>
              </button>
            );
          })}
          <button type="button" onClick={onOpenAll}
            style={{ all: 'unset', cursor: 'pointer', ...mono, fontSize: 8.5, color: 'var(--accent-hover)', padding: '5px 10px', textAlign: 'center' }}>
            {rows.length > SHOW ? `+${rows.length - SHOW} MORE · ` : ''}OPEN ALL ALERTS
          </button>
        </div>
      )}
    </section>
  );
}
