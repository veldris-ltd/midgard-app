import * as React from 'react';

export interface TabDef { label: string; icon?: React.ReactNode; badge?: React.ReactNode; }

/** Underline tabs; 2px accent rule marks the active tab. */
export interface TabsProps extends Omit<React.HTMLAttributes<HTMLDivElement>, 'onChange'> {
  tabs?: Array<string | TabDef>;
  active?: number;
  onChange?: (index: number) => void;
  size?: 'sm' | 'md';
  style?: React.CSSProperties;
}

/** Tabs, underline tabs with 2px accent rule on the active tab. */
export function Tabs({ tabs = [], active = 0, onChange, size = 'md', style, ...rest }: TabsProps) {
  const [hover, setHover] = React.useState(-1);
  return (
    <div style={{ display: 'flex', gap: 4, borderBottom: '1px solid var(--border)', ...style }} {...rest}>
      {tabs.map((t, i) => {
        const tab: TabDef = typeof t === 'string' ? { label: t } : t;
        const isActive = i === active;
        return (
          <button
            key={i}
            type="button"
            onClick={() => onChange && onChange(i)}
            onMouseEnter={() => setHover(i)}
            onMouseLeave={() => setHover(-1)}
            style={{
              all: 'unset', boxSizing: 'border-box',
              display: 'inline-flex', alignItems: 'center', gap: 7,
              padding: size === 'sm' ? '7px 12px' : '10px 16px',
              fontFamily: 'var(--font-sans)',
              fontSize: size === 'sm' ? 'var(--fs-xs)' : 'var(--fs-sm)',
              fontWeight: 'var(--fw-medium)' as React.CSSProperties['fontWeight'],
              color: isActive ? 'var(--text-heading)' : hover === i ? 'var(--text-body)' : 'var(--text-muted)',
              boxShadow: isActive ? 'inset 0 -2px 0 var(--accent)' : 'none',
              cursor: 'pointer',
              transition: 'color var(--dur-fast) var(--ease-out)',
              marginBottom: -1,
            }}
          >
            {tab.icon}
            {tab.label}
            {tab.badge != null && (
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: isActive ? 'var(--accent-hover)' : 'var(--text-faint)' }}>{tab.badge}</span>
            )}
          </button>
        );
      })}
    </div>
  );
}
