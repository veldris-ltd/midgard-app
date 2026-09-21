import React from 'react';

/** StatBlock, big display numeral + mono ALL-CAPS label. */
export function StatBlock({ value, label, sub, size = 'md', accent = false, style, ...rest }) {
  const sizes = { sm: 'var(--fs-h3)', md: 'var(--fs-h2)', lg: 'var(--fs-h1)' };
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4, ...style }} {...rest}>
      <span style={{
        fontFamily: 'var(--font-display)', fontWeight: 'var(--fw-black)',
        fontSize: sizes[size] || sizes.md, lineHeight: 'var(--lh-tight)',
        letterSpacing: 'var(--ls-tight)',
        color: accent ? 'var(--accent-hover)' : 'var(--text-heading)',
      }}>{value}</span>
      <span style={{
        fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)',
        letterSpacing: 'var(--ls-label)', textTransform: 'uppercase',
        color: 'var(--text-faint)',
      }}>{label}</span>
      {sub && <span style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>{sub}</span>}
    </div>
  );
}
