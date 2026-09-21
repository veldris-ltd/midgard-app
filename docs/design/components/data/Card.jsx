import React from 'react';

/** Card, flat ink surface panel; optional hover lift + accent top rule. */
export function Card({ interactive = false, accentRule = false, size = 'md', onClick, children, style, ...rest }) {
  const [hover, setHover] = React.useState(false);
  const pads = { sm: 14, md: 20, lg: 28 };
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
