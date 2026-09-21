import React from 'react';

/** SectionLabel, the numbered mono eyebrow: 01 // IDENTITY. */
export function SectionLabel({ number, children, muted = false, style, ...rest }) {
  return (
    <span style={{
      display: 'inline-flex', alignItems: 'baseline', gap: 8,
      fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)',
      letterSpacing: 'var(--ls-label)', textTransform: 'uppercase',
      color: muted ? 'var(--text-faint)' : 'var(--accent)',
      ...style,
    }} {...rest}>
      {number != null && <span>{String(number).padStart(2, '0')}&nbsp;//</span>}
      <span style={{ color: muted ? 'var(--text-faint)' : 'var(--text-muted)' }}>{children}</span>
    </span>
  );
}
