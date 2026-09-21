import * as React from 'react';

/** Reactive media-query hook (ported from kit-shell.jsx). */
export function useMedia(query: string): boolean {
  const [match, setMatch] = React.useState<boolean>(() =>
    typeof window !== 'undefined' ? window.matchMedia(query).matches : false,
  );
  React.useEffect(() => {
    const mq = window.matchMedia(query);
    const fn = () => setMatch(mq.matches);
    fn();
    mq.addEventListener('change', fn);
    return () => mq.removeEventListener('change', fn);
  }, [query]);
  return match;
}

/** Full-viewport dimmer behind mobile drawers. */
export function Scrim({ onClick }: { onClick?: () => void }) {
  return (
    <div
      onClick={onClick}
      style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.55)', zIndex: 90 }}
    />
  );
}

export interface PanelHandleProps {
  side: 'left' | 'right';
  open: boolean;
  onClick: () => void;
  label: string;
}

/** The 15px chevron gutter that carries a panel divider and toggles collapse. */
export function PanelHandle({ side, open, onClick, label }: PanelHandleProps) {
  const [h, setH] = React.useState(false);
  const chev = side === 'left' ? (open ? '‹' : '›') : open ? '›' : '‹';
  return (
    <button
      type="button"
      aria-label={label}
      aria-expanded={open}
      title={label}
      onClick={onClick}
      onMouseEnter={() => setH(true)}
      onMouseLeave={() => setH(false)}
      style={{
        all: 'unset',
        boxSizing: 'border-box',
        width: 15,
        flex: 'none',
        alignSelf: 'stretch',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: 'pointer',
        background: h ? 'var(--accent-tint)' : 'var(--bg-sunken)',
        borderRight: side === 'left' ? '1px solid var(--border)' : 'none',
        borderLeft: side === 'right' ? '1px solid var(--border)' : 'none',
        color: h ? 'var(--accent-hover)' : 'var(--text-faint)',
        fontFamily: 'var(--font-mono)',
        fontSize: 12,
        lineHeight: 1,
        transition: 'background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out)',
      }}
    >
      {chev}
    </button>
  );
}
