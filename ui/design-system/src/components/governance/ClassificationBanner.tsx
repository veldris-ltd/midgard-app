import * as React from 'react';

/** Full-width classification strip (top of screen). Fixed colours, theme-independent. */
export interface ClassificationBannerProps extends React.HTMLAttributes<HTMLDivElement> {
  level?: 'confidential' | 'restricted' | 'internal' | 'open';
  /** Override the default banner text */
  text?: string;
  /** 20px instead of 26px */
  compact?: boolean;
  style?: React.CSSProperties;
}

type ClassificationLevel = NonNullable<ClassificationBannerProps['level']>;

/** ClassificationBanner, full-width marking strip pinned to the top of every classified surface. */
export function ClassificationBanner({
  level = 'confidential',
  text,
  compact = false,
  style,
  ...rest
}: ClassificationBannerProps) {
  const levels: Record<ClassificationLevel, { bg: string; fg: string; label: string }> = {
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
      fontWeight: 'var(--fw-semibold)' as React.CSSProperties['fontWeight'], letterSpacing: '0.18em',
      textTransform: 'uppercase',
      userSelect: 'none', flex: 'none',
      ...style,
    }} {...rest}>
      {text || l.label}
    </div>
  );
}
