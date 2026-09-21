import * as React from 'react';

/** Select, native select styled as a sunken ink well. */
export interface SelectProps extends Omit<React.SelectHTMLAttributes<HTMLSelectElement>, 'onChange' | 'value' | 'size'> {
  label?: string;
  /** Strings or {value,label} pairs */
  options?: Array<string | { value: string; label: string }>;
  value?: string;
  onChange?: (e: React.ChangeEvent<HTMLSelectElement>) => void;
  size?: 'sm' | 'md' | 'lg';
  fullWidth?: boolean;
  disabled?: boolean;
  style?: React.CSSProperties;
}

export function Select({ label, options = [], value, onChange, size = 'md', fullWidth = false, disabled = false, style, ...rest }: SelectProps) {
  const [focus, setFocus] = React.useState(false);
  const heights: Record<string, number> = { sm: 32, md: 38, lg: 46 };
  const h = heights[size] || heights.md;
  return (
    <label style={{ display: 'inline-flex', flexDirection: 'column', gap: 6, width: fullWidth ? '100%' : 'auto', ...style }}>
      {label && (
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: 'var(--ls-label)', textTransform: 'uppercase', color: 'var(--text-muted)' }}>{label}</span>
      )}
      <span style={{ position: 'relative', display: 'inline-flex', width: fullWidth ? '100%' : 'auto' }}>
        <select
          value={value}
          disabled={disabled}
          onChange={onChange}
          onFocus={() => setFocus(true)}
          onBlur={() => setFocus(false)}
          style={{
            appearance: 'none', WebkitAppearance: 'none',
            width: fullWidth ? '100%' : 'auto',
            height: h, padding: '0 34px 0 12px',
            background: 'var(--surface-inset)',
            color: disabled ? 'var(--text-faint)' : 'var(--grey-100)',
            fontFamily: 'var(--font-sans)', fontSize: 'var(--fs-sm)',
            border: '1px solid ' + (focus ? 'var(--accent)' : 'var(--border)'),
            borderRadius: 'var(--radius-sm)',
            boxShadow: focus ? 'var(--ring-focus)' : 'none',
            outline: 'none',
            cursor: disabled ? 'not-allowed' : 'pointer',
            transition: 'border-color var(--dur-fast) var(--ease-out), box-shadow var(--dur-fast) var(--ease-out)',
          }}
          {...rest}
        >
          {options.map((o) => {
            const opt = typeof o === 'string' ? { value: o, label: o } : o;
            return <option key={opt.value} value={opt.value}>{opt.label}</option>;
          })}
        </select>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
          style={{ position: 'absolute', right: 11, top: '50%', transform: 'translateY(-50%)', pointerEvents: 'none', color: 'var(--text-faint)' }}>
          <path d="m6 9 6 6 6-6" />
        </svg>
      </span>
    </label>
  );
}
