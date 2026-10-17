/* The alert bell and the alerts drawer. [FR-H-1] [FR-H-5]
 *
 * The bell carries the number of open alerts (in the danger colour while a critical one
 * is unacknowledged). The drawer lists them newest first with severity, label, entity,
 * rationale and age; Acknowledge records who and when through the gateway (audited
 * there). Clicking an alert hands it to the screen, which selects its entity and flies
 * to it. Open / all filters by cleared state. */
import * as React from 'react';
import { Badge, Icon, IconButton } from '@midgard/design-system';
import { useGateway } from '@midgard/ui-shared';
import type { HeimdallAlert, HeimdallGateway } from '@midgard/sdk';
import { alertCounts, useStatus } from './status';

const mono: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em' };

/** "12 min ago" */
export function agoText(unix: number): string {
  const s = Math.max(0, Math.round(Date.now() / 1000 - unix));
  if (s < 60) return `${s} s ago`;
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ${Math.round((s % 3600) / 60)} min ago`;
  return `${Math.round(s / 86400)} d ago`;
}

const TONE: Record<HeimdallAlert['severity'], 'danger' | 'warn' | 'neutral'> = { critical: 'danger', warning: 'warn', info: 'neutral' };
const EDGE: Record<HeimdallAlert['severity'], string> = { critical: 'var(--danger)', warning: 'var(--warn)', info: 'var(--divider)' };

/** The bell with the open-alert count. `onClick` defaults to toggling the drawer. */
export function AlertsBell({ onClick }: { onClick?: () => void }) {
  const st = useStatus();
  const { open, critical } = alertCounts(st.alerts);
  const label = st.alertsError ? 'Alerts unavailable' : `${open} open alert${open === 1 ? '' : 's'}${critical ? `, ${critical} critical` : ''}`;
  return (
    <span style={{ position: 'relative', display: 'inline-flex' }}>
      <IconButton label={label} active={st.alertsOpen} onClick={onClick ?? (() => st.setAlertsOpen(!st.alertsOpen))}>
        <Icon name="bell" size={17} color={critical ? 'var(--danger)' : undefined} />
      </IconButton>
      {open > 0 && (
        <span aria-hidden style={{ position: 'absolute', top: 1, right: 0, minWidth: 15, height: 15, padding: '0 4px', boxSizing: 'border-box', borderRadius: 8, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', ...mono, fontSize: 9, letterSpacing: 0, color: '#fff', background: critical ? 'var(--danger)' : 'var(--grey-700, #444)', pointerEvents: 'none' }}>
          {open}
        </span>
      )}
    </span>
  );
}

export function AlertsDrawer({ onPick, onClose, style }: { onPick: (a: HeimdallAlert) => void; onClose: () => void; style?: React.CSSProperties }) {
  const st = useStatus();
  const gateway = useGateway<HeimdallGateway>();
  const [filter, setFilter] = React.useState<'open' | 'all'>('open');
  const [all, setAll] = React.useState<HeimdallAlert[] | null>(null);
  const [busy, setBusy] = React.useState<string | null>(null);
  // "All" is read on demand (open alerts are polled by the status provider).
  React.useEffect(() => {
    if (filter !== 'all') return;
    let live = true;
    gateway.getAlerts('all').then((l) => { if (live) setAll(l); }).catch(() => { if (live) setAll([]); });
    return () => { live = false; };
  }, [filter, gateway, st.alerts]);
  const list = filter === 'open' ? st.alerts : all;
  const ack = async (a: HeimdallAlert) => {
    setBusy(a.id);
    const done = await st.ackAlert(a.id);
    if (done && all) setAll(all.map((x) => (x.id === done.id ? done : x)));
    setBusy(null);
  };
  return (
    <section aria-label="Alerts" style={{ display: 'flex', flexDirection: 'column', gap: 8, padding: 12, background: 'color-mix(in srgb, var(--bg-sunken) 94%, transparent)', backdropFilter: 'var(--blur-panel)', WebkitBackdropFilter: 'var(--blur-panel)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', boxShadow: 'var(--shadow-lg)', ...style }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <Icon name="bell" size={15} color="var(--accent)" />
        <span style={{ ...mono, fontSize: 11, color: 'var(--text-heading)' }}>ALERTS</span>
        <span role="group" aria-label="Which alerts" style={{ display: 'inline-flex', marginLeft: 6, border: '1px solid var(--border)', borderRadius: 'var(--radius-sm)', overflow: 'hidden' }}>
          {(['open', 'all'] as const).map((f) => (
            <button key={f} type="button" aria-pressed={filter === f} onClick={() => setFilter(f)}
              style={{ all: 'unset', cursor: 'pointer', padding: '5px 10px', ...mono, color: filter === f ? 'var(--accent-hover)' : 'var(--text-muted)', background: filter === f ? 'var(--surface-raised)' : 'transparent' }}>
              {f.toUpperCase()}
            </button>
          ))}
        </span>
        <button type="button" onClick={onClose} aria-label="Close alerts" style={{ all: 'unset', cursor: 'pointer', marginLeft: 'auto', ...mono, color: 'var(--text-faint)', padding: '4px 6px' }}>✕</button>
      </div>
      {st.alertsError && <div style={{ ...mono, color: 'var(--text-muted)', padding: '6px 0' }}>ALERTS UNAVAILABLE · THE GATEWAY DOES NOT SERVE THEM YET</div>}
      {!st.alertsError && list === null && <div style={{ ...mono, color: 'var(--text-faint)' }}>READING…</div>}
      {!st.alertsError && list?.length === 0 && <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>No {filter === 'open' ? 'open ' : ''}alerts.</div>}
      <div style={{ display: 'flex', flexDirection: 'column', gap: 6, overflowY: 'auto', minHeight: 0 }}>
        {!st.alertsError && list?.map((a) => (
          <div key={a.id} style={{ border: '1px solid var(--divider)', borderLeft: `3px solid ${EDGE[a.severity]}`, borderRadius: 'var(--radius-sm)', padding: '8px 10px', opacity: a.clearedAt !== null ? 0.6 : 1 }}>
            <button type="button" onClick={() => onPick(a)} style={{ all: 'unset', cursor: 'pointer', display: 'flex', flexDirection: 'column', gap: 4, width: '100%' }}>
              <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <Badge tone={TONE[a.severity]}>{a.severity.toUpperCase()}</Badge>
                <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-heading)', fontWeight: 500, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{a.label}</span>
                <span style={{ ...mono, color: 'var(--text-faint)', marginLeft: 'auto', whiteSpace: 'nowrap' }}>{agoText(a.raisedAt)}</span>
              </span>
              <span style={{ ...mono, color: 'var(--accent-hover)' }}>{a.entity} · {a.kind}{a.clearedAt !== null ? ' · CLEARED' : ''}</span>
              <span style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', lineHeight: 1.45 }}>{a.rationale}</span>
            </button>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 6 }}>
              {a.acknowledgedBy ? (
                <span style={{ ...mono, color: 'var(--secure)' }}>✓ ACK {a.acknowledgedBy}{a.acknowledgedAt ? ` · ${agoText(a.acknowledgedAt)}` : ''}</span>
              ) : (
                <button type="button" disabled={busy === a.id} onClick={() => void ack(a)}
                  style={{ all: 'unset', cursor: busy === a.id ? 'progress' : 'pointer', padding: '5px 10px', ...mono, color: 'var(--text-heading)', border: '1px solid var(--border-strong)', borderRadius: 'var(--radius-sm)' }}>
                  {busy === a.id ? 'ACKNOWLEDGING…' : 'ACKNOWLEDGE'}
                </button>
              )}
              <span style={{ ...mono, color: 'var(--text-faint)', marginLeft: 'auto' }}>{new Date(a.raisedAt * 1000).toISOString().slice(11, 16)}Z</span>
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}
