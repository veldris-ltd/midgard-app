import * as React from 'react';
import { getMode, setMode, Icon, type Mode } from '@midgard/design-system';

/* Segmented light / dark / system control. Ported from site-shell.jsx's
 * ModeToggle: reads/writes the mode manager and re-renders on `mg-mode`. */

const OPTIONS: { mode: Mode; icon: string; label: string }[] = [
  { mode: 'light', icon: 'sun', label: 'Light' },
  { mode: 'dark', icon: 'moon', label: 'Dark' },
  { mode: 'system', icon: 'monitor', label: 'System' },
];

export function ModeToggle() {
  const [mode, setLocal] = React.useState<Mode>(() => getMode());
  React.useEffect(() => {
    const fn = () => setLocal(getMode());
    window.addEventListener('mg-mode', fn);
    return () => window.removeEventListener('mg-mode', fn);
  }, []);

  return (
    <div
      role="group"
      aria-label="Colour mode"
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 2,
        padding: 2,
        background: 'var(--surface-inset)',
        border: '1px solid var(--border)',
        borderRadius: 'var(--radius-pill)',
      }}
    >
      {OPTIONS.map((o) => {
        const on = mode === o.mode;
        return (
          <button
            key={o.mode}
            type="button"
            aria-pressed={on}
            title={o.label}
            onClick={() => {
              setMode(o.mode);
              setLocal(o.mode);
            }}
            style={{
              all: 'unset',
              boxSizing: 'border-box',
              cursor: 'pointer',
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
              width: 26,
              height: 22,
              borderRadius: 'var(--radius-pill)',
              color: on ? 'var(--text-on-accent)' : 'var(--text-faint)',
              background: on ? 'var(--accent)' : 'transparent',
              transition: 'background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out)',
            }}
          >
            <Icon name={o.icon} size={14} />
          </button>
        );
      })}
    </div>
  );
}
