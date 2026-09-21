import React from 'react';

/** MeterBar — labelled utilisation/pass-rate meter with threshold colouring. */
export function MeterBar({ value = 0, label, showValue = true, warnAt = null, dangerAt = null, invert = false, width = 70, format, style, ...rest }) {
  const v = Math.max(0, Math.min(100, value));
  let color = 'var(--accent)';
  if (dangerAt != null && (invert ? v <= dangerAt : v >= dangerAt)) color = 'var(--danger)';
  else if (warnAt != null && (invert ? v <= warnAt : v >= warnAt)) color = 'var(--warn)';
  else if (warnAt == null && dangerAt == null) color = 'var(--accent)';
  else color = 'var(--secure)';
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8, ...style }} {...rest}>
      {label && <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text-faint)' }}>{label}</span>}
      <span style={{ width, height: 5, background: 'var(--surface-inset)', borderRadius: 3, overflow: 'hidden', display: 'inline-block', flex: 'none' }}>
        <span style={{ display: 'block', height: '100%', width: v + '%', background: color, borderRadius: 3, transition: 'width var(--dur-slow) var(--ease-out)' }} />
      </span>
      {showValue && <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--text-body)' }}>{format ? format(v) : v + '%'}</span>}
    </span>
  );
}
