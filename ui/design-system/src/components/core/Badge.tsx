import * as React from 'react';

/** Small mono count / flag chip. */
export interface BadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  tone?: 'neutral' | 'accent' | 'secure' | 'warn' | 'danger' | 'info';
  children?: React.ReactNode;
  style?: React.CSSProperties;
}

type BadgeToneKey = NonNullable<BadgeProps['tone']>;

/** Badge, small count / short status flag. */
export function Badge({ tone = 'neutral', children, style, ...rest }: BadgeProps) {
  const tones: Record<BadgeToneKey, { bg: string; fg: string; bd: string }> = {
    neutral: { bg: 'var(--surface-raised)', fg: 'var(--grey-300)', bd: 'var(--border)' },
    accent:  { bg: 'var(--accent-tint)', fg: 'var(--accent-hover)', bd: 'transparent' },
    secure:  { bg: 'var(--secure-tint)', fg: 'var(--secure)', bd: 'transparent' },
    warn:    { bg: 'var(--warn-tint)', fg: 'var(--warn)', bd: 'transparent' },
    danger:  { bg: 'var(--danger-tint)', fg: 'var(--midgard-300)', bd: 'transparent' },
    info:    { bg: 'var(--info-tint)', fg: 'var(--info)', bd: 'transparent' },
  };
  const t = tones[tone] || tones.neutral;
  return (
    <span
      style={{
        display: 'inline-flex', alignItems: 'center',
        padding: '2px 8px',
        fontFamily: 'var(--font-mono)',
        fontSize: 'var(--fs-micro)',
        fontWeight: 'var(--fw-medium)' as React.CSSProperties['fontWeight'],
        letterSpacing: '0.06em',
        borderRadius: 'var(--radius-xs)',
        background: t.bg, color: t.fg,
        border: '1px solid ' + t.bd,
        whiteSpace: 'nowrap',
        ...style,
      }}
      {...rest}
    >
      {children}
    </span>
  );
}
