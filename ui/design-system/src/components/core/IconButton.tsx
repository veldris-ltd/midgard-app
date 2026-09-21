import * as React from 'react';

/** Square icon-only action button. */
export interface IconButtonProps extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'onClick'> {
  /** sm 28 · md 36 · lg 44 px square */
  size?: 'sm' | 'md' | 'lg';
  /** 'ghost' = bare; 'solid' = ink surface + border */
  variant?: 'ghost' | 'solid';
  /** Accent-tinted active state */
  active?: boolean;
  disabled?: boolean;
  /** Accessible label (also the tooltip) */
  label?: string;
  onClick?: (e: React.MouseEvent) => void;
  /** Usually an <Icon /> */
  children?: React.ReactNode;
  style?: React.CSSProperties;
}

/** IconButton, square icon-only action. */
export function IconButton({ size = 'md', variant = 'ghost', active = false, disabled = false, label, onClick, children, style, ...rest }: IconButtonProps) {
  const [hover, setHover] = React.useState(false);
  const [press, setPress] = React.useState(false);
  const dims: Record<'sm' | 'md' | 'lg', number> = { sm: 28, md: 36, lg: 44 };
  const d = dims[size] || dims.md;
  const bg = active
    ? 'var(--accent-tint)'
    : variant === 'solid'
      ? (hover ? 'var(--surface-raised)' : 'var(--surface-card)')
      : hover ? 'rgba(255,255,255,0.06)' : 'transparent';
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={disabled ? undefined : onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => { setHover(false); setPress(false); }}
      onMouseDown={() => setPress(true)}
      onMouseUp={() => setPress(false)}
      style={{
        width: d, height: d, flex: 'none',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        background: disabled ? 'transparent' : bg,
        color: disabled ? 'var(--text-faint)' : active ? 'var(--accent-hover)' : hover ? 'var(--text-heading)' : 'var(--text-muted)',
        border: '1px solid ' + (variant === 'solid' ? 'var(--border)' : 'transparent'),
        borderRadius: 'var(--radius-sm)',
        cursor: disabled ? 'not-allowed' : 'pointer',
        transform: press && !disabled ? 'scale(0.96)' : 'none',
        transition: 'background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out), transform var(--dur-fast) var(--ease-out)',
        ...style,
      }}
      {...rest}
    >
      {children}
    </button>
  );
}
