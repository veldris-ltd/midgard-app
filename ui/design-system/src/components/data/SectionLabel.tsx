import * as React from 'react';

/** SectionLabel, the numbered mono eyebrow: 01 // IDENTITY. */
export interface SectionLabelProps extends React.HTMLAttributes<HTMLSpanElement> {
  /** Section number; zero-padded automatically */
  number?: number | string;
  children?: React.ReactNode;
  /** Grey instead of accent numeral */
  muted?: boolean;
  style?: React.CSSProperties;
}

export function SectionLabel({ number, children, muted = false, style, ...rest }: SectionLabelProps) {
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
