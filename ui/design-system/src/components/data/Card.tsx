import * as React from 'react';

/** Card, flat ink surface panel; optional hover lift + accent top rule. */
export interface CardProps extends Omit<React.HTMLAttributes<HTMLDivElement>, 'onClick'> {
  /** Lifts one ink step on hover */
  interactive?: boolean;
  /** 2px accent top rule */
  accentRule?: boolean;
  /** Padding: sm 14 · md 20 · lg 28 (lg also uses radius-lg) */
  size?: 'sm' | 'md' | 'lg';
  onClick?: () => void;
  children?: React.ReactNode;
  style?: React.CSSProperties;
}

export function Card({ interactive = false, accentRule = false, size = 'md', onClick, children, style, ...rest }: CardProps) {
  const [hover, setHover] = React.useState(false);
  const pads: Record<string, number> = { sm: 14, md: 20, lg: 28 };
  return (
    <div
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: 'relative',
        flexShrink: 0,
        background: interactive && hover ? 'var(--surface-raised)' : 'var(--surface-card)',
        border: '1px solid ' + (interactive && hover ? 'var(--border-strong)' : 'var(--border)'),
        borderRadius: size === 'lg' ? 'var(--radius-lg)' : 'var(--radius-md)',
        padding: pads[size] || pads.md,
        cursor: interactive || onClick ? 'pointer' : 'default',
        transition: 'background var(--dur-fast) var(--ease-out), border-color var(--dur-fast) var(--ease-out)',
        overflow: 'hidden',
        ...style,
      }}
      {...rest}
    >
      {accentRule && (
        <span style={{ position: 'absolute', top: 0, left: 0, right: 0, height: 2, background: 'var(--accent)', opacity: interactive && !hover ? 0.7 : 1 }} />
      )}
      {children}
    </div>
  );
}
