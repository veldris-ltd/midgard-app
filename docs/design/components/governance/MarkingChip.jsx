import React from 'react';

/** MarkingChip, per-object classification marking (mandatory access control). */
export function MarkingChip({ level = 'confidential', label, compartment, style, ...rest }) {
  const levels = {
    confidential: { c: 'var(--marking-confidential)', short: 'C' },
    restricted:   { c: 'var(--marking-restricted)', short: 'R' },
    internal:     { c: 'var(--marking-internal)', short: 'I' },
    open:         { c: 'var(--marking-open)', short: 'O' },
  };
  const l = levels[level] || levels.confidential;
  return (
    <span style={{
      display: 'inline-flex', alignItems: 'center', gap: 6,
      padding: '2px 8px 2px 4px',
      fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)',
      letterSpacing: '0.08em', textTransform: 'uppercase',
      color: 'var(--grey-300)',
      background: 'var(--surface-inset)',
      border: '1px solid var(--border)',
      borderRadius: 'var(--radius-pill)',
      whiteSpace: 'nowrap',
      ...style,
    }} {...rest}>
      <span style={{
        width: 14, height: 14, borderRadius: '50%', flex: 'none',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        background: l.c, color: level === 'restricted' || level === 'open' ? '#0a0b0d' : '#fff',
        fontSize: 8, fontWeight: 'var(--fw-bold)',
      }}>{l.short}</span>
      {label || level}
      {compartment && <span style={{ color: 'var(--text-faint)' }}>// {compartment}</span>}
    </span>
  );
}
