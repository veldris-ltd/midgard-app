import * as React from 'react';

/* Desktop window chrome for the three desktop targets. Ported from
 * docs/design/ui_kits/clients/desktop.html (MacFrame / WindowsFrame / LinuxFrame).
 * Presentational wrappers taking children; the title bar uses design-system
 * tokens, with the macOS traffic-light hexes kept as deliberate platform chrome. */

export type DesktopOs = 'macos' | 'windows' | 'linux';

export interface DesktopFrameProps {
  os: DesktopOs;
  title?: string;
  width?: number;
  height?: number;
  children?: React.ReactNode;
}

function MacFrame({ children, title, width, height }: Omit<DesktopFrameProps, 'os'>) {
  return (
    <div style={{ width, borderRadius: 12, border: '1px solid var(--border-strong)', overflow: 'hidden', background: 'var(--bg-sunken)', boxShadow: 'var(--shadow-lg)' }}>
      <div style={{ height: 34, display: 'flex', alignItems: 'center', position: 'relative', background: 'var(--surface-inset)', borderBottom: '1px solid var(--border)' }}>
        <div style={{ display: 'flex', gap: 7, padding: '0 14px' }}>
          {['#ff5f57', '#febc2e', '#28c840'].map((c) => (
            <span key={c} style={{ width: 11, height: 11, borderRadius: '50%', background: c, opacity: 0.9 }} />
          ))}
        </div>
        <span style={{ position: 'absolute', left: '50%', transform: 'translateX(-50%)', fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-muted)', whiteSpace: 'nowrap' }}>{title}</span>
      </div>
      <div style={{ height }}>{children}</div>
    </div>
  );
}

function WindowsFrame({ children, title, width, height }: Omit<DesktopFrameProps, 'os'>) {
  return (
    <div style={{ width, borderRadius: 8, border: '1px solid var(--border-strong)', overflow: 'hidden', background: 'var(--bg-sunken)', boxShadow: 'var(--shadow-lg)' }}>
      <div style={{ height: 32, display: 'flex', alignItems: 'center', background: 'var(--surface-inset)', borderBottom: '1px solid var(--border)' }}>
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-muted)', padding: '0 12px' }}>{title}</span>
        <div style={{ flex: 1 }} />
        {['–', '▢', '✕'].map((g) => (
          <span key={g} style={{ width: 42, height: 32, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', fontSize: 10, color: 'var(--text-muted)' }}>{g}</span>
        ))}
      </div>
      <div style={{ height }}>{children}</div>
    </div>
  );
}

function LinuxFrame({ children, title, width, height }: Omit<DesktopFrameProps, 'os'>) {
  return (
    <div style={{ width, borderRadius: 10, border: '1px solid var(--border-strong)', overflow: 'hidden', background: 'var(--bg-sunken)', boxShadow: 'var(--shadow-lg)' }}>
      <div style={{ height: 34, display: 'flex', alignItems: 'center', justifyContent: 'center', position: 'relative', background: 'var(--surface-inset)', borderBottom: '1px solid var(--border)' }}>
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-muted)' }}>{title}</span>
        <div style={{ position: 'absolute', right: 10, display: 'flex', gap: 8 }}>
          {['–', '✕'].map((g) => (
            <span key={g} style={{ width: 20, height: 20, borderRadius: '50%', background: 'var(--surface-raised)', display: 'inline-flex', alignItems: 'center', justifyContent: 'center', fontSize: 9, color: 'var(--text-muted)' }}>{g}</span>
          ))}
        </div>
      </div>
      <div style={{ height }}>{children}</div>
    </div>
  );
}

/** DesktopFrame — native desktop window chrome for macOS, Windows or Linux. */
export function DesktopFrame({ os, title = 'Midgard — Object Fabric', width = 820, height = 520, children }: DesktopFrameProps) {
  if (os === 'windows') return <WindowsFrame title={title} width={width} height={height}>{children}</WindowsFrame>;
  if (os === 'linux') return <LinuxFrame title={title} width={width} height={height}>{children}</LinuxFrame>;
  return <MacFrame title={title} width={width} height={height}>{children}</MacFrame>;
}
