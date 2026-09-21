import * as React from 'react';

/** StatBlock, big display numeral + mono ALL-CAPS label. */
export interface StatBlockProps extends React.HTMLAttributes<HTMLDivElement> {
  /** The number/readout, e.g. "99.9%" */
  value: React.ReactNode;
  /** Mono label, e.g. "CONTROL-PLANE SLO" */
  label: string;
  /** Optional small sub-line */
  sub?: React.ReactNode;
  size?: 'sm' | 'md' | 'lg';
  /** Colour the numeral in the product accent */
  accent?: boolean;
  style?: React.CSSProperties;
}

export function StatBlock({ value, label, sub, size = 'md', accent = false, style, ...rest }: StatBlockProps) {
  const sizes: Record<string, string> = { sm: 'var(--fs-h3)', md: 'var(--fs-h2)', lg: 'var(--fs-h1)' };
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4, ...style }} {...rest}>
      <span style={{
        fontFamily: 'var(--font-display)', fontWeight: 'var(--fw-black)' as React.CSSProperties['fontWeight'],
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
