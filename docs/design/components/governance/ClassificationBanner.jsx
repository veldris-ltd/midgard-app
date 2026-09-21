import React from 'react';

/** ClassificationBanner, full-width marking strip pinned to the top of every classified surface. */
export function ClassificationBanner({ level = 'confidential', text, compact = false, style, ...rest }) {
  const levels = {
    confidential: { bg: 'var(--marking-confidential)', fg: '#fff', label: 'CONFIDENTIAL · RECIPIENT EYES ONLY' },
    restricted:   { bg: 'var(--marking-restricted)', fg: '#0a0b0d', label: 'RESTRICTED' },
    internal:     { bg: 'var(--marking-internal)', fg: '#fff', label: 'INTERNAL' },
    open:         { bg: 'var(--marking-open)', fg: '#0a0b0d', label: 'OPEN · UNCLASSIFIED' },
  };
  const l = levels[level] || levels.confidential;
  return (
    <div style={{
      height: compact ? 20 : 'var(--banner-h)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      background: l.bg, color: l.fg,
      fontFamily: 'var(--font-mono)', fontSize: compact ? 9 : 10,
      fontWeight: 'var(--fw-semibold)', letterSpacing: '0.18em',
      textTransform: 'uppercase',
      userSelect: 'none', flex: 'none',
      ...style,
    }} {...rest}>
      {text || l.label}
    </div>
  );
}
