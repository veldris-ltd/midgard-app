import React from 'react';

/** SideNav, console left rail: sections of items with icons, accent rail on the active item. */
export function SideNav({ sections = [], activeId, onSelect, footer = null, width, style, ...rest }) {
  const [hover, setHover] = React.useState(null);
  return (
    <nav style={{
      width: width || 'var(--sidenav-w)', flex: 'none',
      display: 'flex', flexDirection: 'column',
      background: 'var(--bg-sunken)',
      borderRight: '1px solid var(--border)',
      padding: '12px 10px',
      gap: 18,
      overflowY: 'auto',
      ...style,
    }} {...rest}>
      {sections.map((sec, si) => (
        <div key={si} style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {sec.title && (
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: 'var(--ls-label)', textTransform: 'uppercase', color: 'var(--text-faint)', padding: '0 10px 6px' }}>{sec.title}</div>
          )}
          {(sec.items || []).map((it) => {
            const isActive = it.id === activeId;
            const isHover = hover === it.id;
            return (
              <button
                key={it.id}
                type="button"
                onClick={() => onSelect && onSelect(it.id)}
                onMouseEnter={() => setHover(it.id)}
                onMouseLeave={() => setHover(null)}
                style={{
                  all: 'unset', boxSizing: 'border-box',
                  display: 'flex', alignItems: 'center', gap: 10,
                  height: 34, padding: '0 10px',
                  borderRadius: 'var(--radius-sm)',
                  fontFamily: 'var(--font-sans)', fontSize: 'var(--fs-sm)',
                  fontWeight: isActive ? 'var(--fw-semibold)' : 'var(--fw-regular)',
                  color: isActive ? 'var(--text-heading)' : isHover ? 'var(--text-body)' : 'var(--text-muted)',
                  background: isActive ? 'var(--accent-tint)' : isHover ? 'rgba(255,255,255,0.04)' : 'transparent',
                  boxShadow: isActive ? 'inset 2px 0 0 var(--accent)' : 'none',
                  cursor: 'pointer',
                  transition: 'background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out)',
                }}
              >
                {it.icon}
                <span style={{ flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{it.label}</span>
                {it.badge != null && <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: isActive ? 'var(--accent-hover)' : 'var(--text-faint)' }}>{it.badge}</span>}
              </button>
            );
          })}
        </div>
      ))}
      {footer && <div style={{ marginTop: 'auto' }}>{footer}</div>}
    </nav>
  );
}
