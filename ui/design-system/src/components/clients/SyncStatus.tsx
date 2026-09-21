import * as React from 'react';

/** Offline-first sync chip (FR-C-4): offline (with queued op count), syncing (spinner), synced, merged. */
export interface SyncStatusProps extends React.HTMLAttributes<HTMLSpanElement> {
  state?: 'offline' | 'syncing' | 'synced' | 'merged';
  /** Locally queued CRDT ops while offline/draining */
  queued?: number;
  /** Override the default detail text */
  detail?: string;
  style?: React.CSSProperties;
}

type SyncState = NonNullable<SyncStatusProps['state']>;

/** SyncStatus — the offline-first sync state chip: offline (queued ops), syncing, synced, conflict-free merge receipt. */
export function SyncStatus({ state = 'synced', queued = 0, detail, style, ...rest }: SyncStatusProps) {
  const map: Record<SyncState, { c: string; label: string; d: string }> = {
    offline:  { c: 'var(--grey-500)', label: 'OFFLINE', d: queued > 0 ? queued + ' ops queued locally' : 'edits persist locally' },
    syncing:  { c: 'var(--info)', label: 'SYNCING', d: queued > 0 ? 'draining ' + queued + ' ops' : 'exchanging CRDT ops' },
    synced:   { c: 'var(--secure)', label: 'SYNCED', d: 'conflict-free · up to date' },
    merged:   { c: 'var(--secure)', label: 'MERGED', d: 'reconnected · zero conflicts' },
  };
  const s = map[state] || map.synced;
  const spin = state === 'syncing';
  return (
    <span style={{
      display: 'inline-flex', alignItems: 'center', gap: 8,
      padding: '4px 10px 4px 8px',
      fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.06em',
      background: 'var(--surface-inset)', border: '1px solid var(--border)',
      borderRadius: 'var(--radius-pill)', color: 'var(--grey-300)', whiteSpace: 'nowrap',
      ...style,
    }} {...rest}>
      <span style={{ position: 'relative', width: 10, height: 10, flex: 'none' }}>
        {spin ? (
          <svg viewBox="0 0 10 10" width="10" height="10" style={{ animation: 'mgSpin 1.1s linear infinite', display: 'block' }}>
            <circle cx="5" cy="5" r="4" fill="none" stroke="var(--surface-raised)" strokeWidth="1.6" />
            <path d="M5 1 a4 4 0 0 1 4 4" fill="none" stroke={s.c} strokeWidth="1.6" strokeLinecap="round" />
          </svg>
        ) : (
          <span style={{ position: 'absolute', inset: 1, borderRadius: '50%', background: s.c }} />
        )}
      </span>
      <span style={{ color: s.c, fontWeight: 'var(--fw-medium)' as React.CSSProperties['fontWeight'] }}>{s.label}</span>
      <span style={{ color: 'var(--text-faint)' }}>{detail || s.d}</span>
      <style>{'@keyframes mgSpin{to{transform:rotate(360deg)}}@media (prefers-reduced-motion: reduce){svg[style*="mgSpin"]{animation:none!important}}'}</style>
    </span>
  );
}
