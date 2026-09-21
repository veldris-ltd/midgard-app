import * as React from 'react';

/** Live state pill with leading (optionally pulsing) dot. */
export interface StatusPillProps extends React.HTMLAttributes<HTMLSpanElement> {
  status?: 'secure' | 'online' | 'healthy' | 'offline' | 'warn' | 'danger' | 'live' | 'gated';
  /** Override the default status text */
  label?: string;
  /** Pulse animation on positive/live states */
  pulse?: boolean;
  style?: React.CSSProperties;
}

type StatusKey = NonNullable<StatusPillProps['status']>;

/** StatusPill, live state indicator with a leading dot (sealed / online / degraded / gated). */
export function StatusPill({ status = 'secure', label, pulse = true, style, ...rest }: StatusPillProps) {
  const map: Record<StatusKey, { color: string; text: string }> = {
    secure:   { color: 'var(--secure)', text: label || 'Sealed' },
    online:   { color: 'var(--secure)', text: label || 'Online' },
    healthy:  { color: 'var(--secure)', text: label || 'Healthy' },
    offline:  { color: 'var(--grey-500)', text: label || 'Offline' },
    warn:     { color: 'var(--warn)', text: label || 'Degraded' },
    danger:   { color: 'var(--midgard-300)', text: label || 'Failed' },
    live:     { color: 'var(--accent-hover)', text: label || 'Live' },
    gated:    { color: 'var(--warn)', text: label || 'Human gate' },
  };
  const s = map[status] || map.secure;
  const doPulse = pulse && (status === 'online' || status === 'secure' || status === 'healthy' || status === 'live');
  const kf = '@keyframes mgPulse{0%{transform:scale(1);opacity:.65}70%{transform:scale(2.6);opacity:0}100%{opacity:0}}@media (prefers-reduced-motion: reduce){[style*="mgPulse"]{animation:none!important}}';
  return (
    <span
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 7,
        padding: '3px 10px 3px 8px',
        fontFamily: 'var(--font-mono)',
        fontSize: 'var(--fs-xs)',
        fontWeight: 'var(--fw-medium)' as React.CSSProperties['fontWeight'],
        letterSpacing: '0.04em',
        borderRadius: 'var(--radius-pill)',
        background: 'var(--surface-inset)',
        border: '1px solid var(--border)',
        color: 'var(--grey-300)',
        whiteSpace: 'nowrap',
        ...style,
      }}
      {...rest}
    >
      <span style={{ position: 'relative', width: 8, height: 8, flex: 'none' }}>
        <span style={{ position: 'absolute', inset: 0, borderRadius: '50%', background: s.color }} />
        {doPulse && (
          <span style={{ position: 'absolute', inset: 0, borderRadius: '50%', background: s.color, animation: 'mgPulse 1.8s var(--ease-out) infinite' }} />
        )}
      </span>
      {s.text}
      <style>{kf}</style>
    </span>
  );
}
