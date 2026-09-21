import React from 'react';

/** Tag, removable label chip for filters, topics, dataset tags. */
export function Tag({ onRemove, active = false, onClick, children, style, ...rest }) {
  const [hover, setHover] = React.useState(false);
  return (
    <span
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 6,
        padding: '4px 10px',
        fontFamily: 'var(--font-mono)',
        fontSize: 'var(--fs-xs)',
        letterSpacing: '0.03em',
        borderRadius: 'var(--radius-pill)',
        background: active ? 'var(--accent-tint)' : hover && onClick ? 'var(--surface-raised)' : 'var(--surface-inset)',
        color: active ? 'var(--accent-hover)' : 'var(--grey-300)',
        border: '1px solid ' + (active ? 'var(--accent)' : 'var(--border)'),
        cursor: onClick ? 'pointer' : 'default',
        transition: 'background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out)',
        whiteSpace: 'nowrap',
        ...style,
      }}
      {...rest}
    >
      {children}
      {onRemove && (
        <button
          type="button"
          onClick={(e) => { e.stopPropagation(); onRemove(); }}
          style={{ all: 'unset', cursor: 'pointer', lineHeight: 1, fontSize: 12, color: 'var(--grey-500)' }}
          aria-label="Remove"
        >×</button>
      )}
    </span>
  );
}
