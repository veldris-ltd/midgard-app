import React from 'react';

/** Input, text field with optional mono mode, label, prefix glyph, and status. */
export function Input({
  label, hint, error,
  mono = false, size = 'md',
  prefix = null, suffix = null,
  fullWidth = false,
  style, inputStyle,
  ...rest
}) {
  const [focus, setFocus] = React.useState(false);
  const heights = { sm: 32, md: 38, lg: 46 };
  const h = heights[size] || heights.md;
  return (
    <label style={{ display: 'inline-flex', flexDirection: 'column', gap: 6, width: fullWidth ? '100%' : 'auto', ...style }}>
      {label && (
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: 'var(--ls-label)', textTransform: 'uppercase', color: 'var(--text-muted)' }}>{label}</span>
      )}
      <span
        style={{
          display: 'inline-flex', alignItems: 'center', gap: 8,
          height: h, padding: '0 12px',
          background: 'var(--surface-inset)',
          border: '1px solid ' + (error ? 'var(--danger)' : focus ? 'var(--accent)' : 'var(--border)'),
          borderRadius: 'var(--radius-sm)',
          boxShadow: focus ? 'var(--ring-focus)' : 'none',
          transition: 'border-color var(--dur-fast) var(--ease-out), box-shadow var(--dur-fast) var(--ease-out)',
        }}
      >
        {prefix}
        <input
          onFocus={() => setFocus(true)}
          onBlur={() => setFocus(false)}
          style={{
            all: 'unset', flex: 1, minWidth: 0,
            fontFamily: mono ? 'var(--font-mono)' : 'var(--font-sans)',
            fontSize: 'var(--fs-sm)',
            color: 'var(--grey-100)',
            ...inputStyle,
          }}
          {...rest}
        />
        {suffix}
      </span>
      {(error || hint) && (
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: error ? 'var(--midgard-300)' : 'var(--text-faint)' }}>{error || hint}</span>
      )}
    </label>
  );
}
