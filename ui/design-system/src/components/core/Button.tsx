import * as React from 'react';

/**
 * Midgard Button, the primary action primitive.
 * Crisp corners, one product accent (via theme scope), decisive hover/press.
 */
export type ButtonVariant = 'primary' | 'secondary' | 'outline' | 'ghost';
export type ButtonSize = 'sm' | 'md' | 'lg';

export interface ButtonProps extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'type'> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  fullWidth?: boolean;
  disabled?: boolean;
  iconLeft?: React.ReactNode;
  iconRight?: React.ReactNode;
  type?: 'button' | 'submit' | 'reset';
  children?: React.ReactNode;
}

export function Button({
  variant = 'primary',
  size = 'md',
  fullWidth = false,
  disabled = false,
  iconLeft = null,
  iconRight = null,
  type = 'button',
  onClick,
  children,
  style,
  ...rest
}: ButtonProps) {
  const [hover, setHover] = React.useState(false);
  const [press, setPress] = React.useState(false);

  const sizes: Record<ButtonSize, { pad: string; fs: string; h: number; gap: number }> = {
    sm: { pad: '7px 13px', fs: 'var(--fs-xs)', h: 30, gap: 7 },
    md: { pad: '9px 18px', fs: 'var(--fs-sm)', h: 38, gap: 8 },
    lg: { pad: '13px 26px', fs: 'var(--fs-body)', h: 48, gap: 10 },
  };
  const s = sizes[size] || sizes.md;

  const base: React.CSSProperties = {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: s.gap,
    width: fullWidth ? '100%' : 'auto',
    minHeight: s.h,
    padding: s.pad,
    fontFamily: 'var(--font-sans)',
    fontSize: s.fs,
    fontWeight: 'var(--fw-semibold)' as unknown as number,
    letterSpacing: '0.01em',
    lineHeight: 1,
    borderRadius: 'var(--radius-sm)',
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: 'transparent',
    cursor: disabled ? 'not-allowed' : 'pointer',
    transition:
      'background var(--dur-fast) var(--ease-out), border-color var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out), box-shadow var(--dur-fast) var(--ease-out), transform var(--dur-fast) var(--ease-out)',
    transform: press && !disabled ? 'translateY(1px)' : 'none',
    whiteSpace: 'nowrap',
    ...style,
  };

  const variants: Record<ButtonVariant, React.CSSProperties> = {
    primary: {
      background: press ? 'var(--accent-press)' : hover ? 'var(--accent-hover)' : 'var(--accent)',
      color: 'var(--text-on-accent)',
      boxShadow: hover && !press ? 'var(--shadow-glow)' : 'none',
    },
    secondary: {
      background: hover ? 'var(--surface-raised)' : 'var(--surface-card)',
      color: 'var(--text-heading)',
      borderColor: hover ? 'var(--border-strong)' : 'var(--border)',
    },
    outline: {
      background: hover ? 'var(--accent-tint)' : 'transparent',
      color: hover ? 'var(--accent-hover)' : 'var(--accent)',
      borderColor: 'var(--accent)',
    },
    ghost: {
      background: hover ? 'rgba(255,255,255,0.05)' : 'transparent',
      color: 'var(--text-body)',
    },
  };

  const disabledStyle: React.CSSProperties = disabled
    ? { background: 'var(--surface-inset)', color: 'var(--text-faint)', borderColor: 'var(--border)', boxShadow: 'none' }
    : {};

  return (
    <button
      type={type}
      disabled={disabled}
      onClick={disabled ? undefined : onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => {
        setHover(false);
        setPress(false);
      }}
      onMouseDown={() => setPress(true)}
      onMouseUp={() => setPress(false)}
      style={{ ...base, ...(variants[variant] || variants.primary), ...disabledStyle }}
      {...rest}
    >
      {iconLeft}
      {children}
      {iconRight}
    </button>
  );
}
