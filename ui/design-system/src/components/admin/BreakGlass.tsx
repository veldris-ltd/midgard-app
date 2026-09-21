import * as React from 'react';

/** Break-glass emergency-elevation strip (FR-AD-8). idle = sealed chip; active = pulsing red with countdown; review = amber post-hoc review demand. */
export interface BreakGlassProps extends React.HTMLAttributes<HTMLDivElement> {
  state?: 'idle' | 'active' | 'review';
  holder?: string;
  /** What the elevation covers — never more than the emergency requires */
  scope?: string;
  /** Countdown label while active */
  remaining?: string;
  justification?: string;
  style?: React.CSSProperties;
}

/** BreakGlass — the emergency-elevation state strip (FR-AD-8): time-boxed, justified, alerted, reviewed. */
export function BreakGlass({
  state = 'idle',
  holder,
  scope,
  remaining = '00:41:12',
  justification,
  style,
  ...rest
}: BreakGlassProps) {
  if (state === 'idle') {
    return (
      <div style={{ display: 'inline-flex', alignItems: 'center', gap: 8, padding: '5px 11px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.08em', color: 'var(--text-faint)', background: 'var(--surface-inset)', border: '1px dashed var(--border-strong)', borderRadius: 'var(--radius-sm)', ...style }} {...rest}>
        <span style={{ width: 7, height: 7, borderRadius: '50%', background: 'var(--grey-500)' }} />
        BREAK-GLASS · SEALED · DUAL-CONTROL TO OPEN
      </div>
    );
  }
  const active = state === 'active';
  const c = active ? 'var(--danger)' : 'var(--warn)';
  return (
    <div style={{ background: active ? 'var(--danger-tint)' : 'var(--warn-tint)', border: '1px solid ' + c, borderRadius: 'var(--radius-md)', padding: '11px 14px', display: 'flex', flexDirection: 'column', gap: 7, ...style }} {...rest}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <span style={{ position: 'relative', width: 9, height: 9, flex: 'none' }}>
          <span style={{ position: 'absolute', inset: 0, borderRadius: '50%', background: c }} />
          {active && <span style={{ position: 'absolute', inset: 0, borderRadius: '50%', background: c, animation: 'mgPulse 1.2s var(--ease-out) infinite' }} />}
        </span>
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', letterSpacing: '0.12em', fontWeight: 'var(--fw-semibold)' as React.CSSProperties['fontWeight'], color: c }}>
          {active ? 'BREAK-GLASS ACTIVE' : 'POST-HOC REVIEW REQUIRED'}
        </span>
        {active && <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--text-heading)', marginLeft: 'auto' }}>{remaining} REMAINING</span>}
      </div>
      <div style={{ display: 'flex', gap: 14, flexWrap: 'wrap', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)' }}>
        {holder && <span><span style={{ color: 'var(--text-faint)' }}>HOLDER </span><span style={{ color: 'var(--text-body)' }}>{holder}</span></span>}
        {scope && <span><span style={{ color: 'var(--text-faint)' }}>SCOPE </span><span style={{ color: 'var(--text-body)' }}>{scope}</span></span>}
        <span style={{ color: 'var(--text-faint)' }}>SECURITY OFFICERS ALERTED · REAL-TIME</span>
      </div>
      {justification && <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.5 }}>{justification}</div>}
      <style>{'@keyframes mgPulse{0%{transform:scale(1);opacity:.65}70%{transform:scale(2.4);opacity:0}100%{opacity:0}}'}</style>
    </div>
  );
}
