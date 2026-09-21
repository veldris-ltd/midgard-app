import * as React from 'react';

/** Admin action risk level 1–5 (KDD-2). Risk ≥4 implies dual-control unless overridden. */
export interface RiskBadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  level?: 1 | 2 | 3 | 4 | 5;
  /** Force the DUAL-CONTROL flag on/off; null = inferred from level */
  dualControl?: boolean | null;
  style?: React.CSSProperties;
}

/** RiskBadge — admin action risk level 1–5 (KDD-2): control strength scales with danger. */
export function RiskBadge({ level = 1, dualControl = null, style, ...rest }: RiskBadgeProps) {
  const n = Math.max(1, Math.min(5, level));
  const color = n >= 4 ? 'var(--danger)' : n === 3 ? 'var(--warn)' : 'var(--secure)';
  const needsDual = dualControl === null ? n >= 4 : dualControl;
  return (
    <span style={{
      display: 'inline-flex', alignItems: 'center', gap: 7,
      padding: '3px 9px',
      fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.07em',
      background: 'var(--surface-inset)', border: '1px solid var(--border)',
      borderRadius: 'var(--radius-xs)', whiteSpace: 'nowrap',
      ...style,
    }} {...rest}>
      <span style={{ display: 'inline-flex', gap: 2, alignItems: 'flex-end' }}>
        {[1, 2, 3, 4, 5].map(i => (
          <span key={i} style={{ width: 3, height: 3 + i * 1.6, borderRadius: 1, background: i <= n ? color : 'var(--surface-raised)' }} />
        ))}
      </span>
      <span style={{ color, fontWeight: 'var(--fw-medium)' as React.CSSProperties['fontWeight'] }}>RISK {n}</span>
      {needsDual && <span style={{ color: 'var(--text-faint)' }}>DUAL-CONTROL</span>}
    </span>
  );
}
