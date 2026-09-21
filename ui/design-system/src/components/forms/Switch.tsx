import * as React from 'react';

/** Switch, binary toggle; product accent when on. */
export interface SwitchProps extends Omit<React.LabelHTMLAttributes<HTMLLabelElement>, 'onChange'> {
  checked?: boolean;
  onChange?: (next: boolean) => void;
  label?: string;
  disabled?: boolean;
  style?: React.CSSProperties;
}

export function Switch({ checked = false, onChange, label, disabled = false, style, ...rest }: SwitchProps) {
  return (
    <label style={{ display: 'inline-flex', alignItems: 'center', gap: 10, cursor: disabled ? 'not-allowed' : 'pointer', ...style }} {...rest}>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => !disabled && onChange && onChange(!checked)}
        style={{
          all: 'unset', boxSizing: 'border-box',
          width: 38, height: 22, flex: 'none',
          borderRadius: 'var(--radius-pill)',
          background: checked ? 'var(--accent)' : 'var(--surface-raised)',
          border: '1px solid ' + (checked ? 'var(--accent)' : 'var(--border-strong)'),
          position: 'relative',
          opacity: disabled ? 0.45 : 1,
          transition: 'background var(--dur-base) var(--ease-out), border-color var(--dur-base) var(--ease-out)',
          cursor: 'inherit',
        }}
      >
        <span style={{
          position: 'absolute', top: 2, left: checked ? 18 : 2,
          width: 16, height: 16, borderRadius: '50%',
          background: checked ? '#fff' : 'var(--grey-400)',
          transition: 'left var(--dur-base) var(--ease-out), background var(--dur-base) var(--ease-out)',
        }} />
      </button>
      {label && <span style={{ fontSize: 'var(--fs-sm)', color: disabled ? 'var(--text-faint)' : 'var(--text-body)' }}>{label}</span>}
    </label>
  );
}
