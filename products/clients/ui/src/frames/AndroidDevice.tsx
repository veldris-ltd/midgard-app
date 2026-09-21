import * as React from 'react';
import { useResolvedMode } from '@midgard/ui-shared';

/* AndroidDevice — a simplified Android (Material 3) device frame: bezel, status
 * bar, top app bar and gesture-nav pill. Ported from
 * docs/design/ui_kits/clients/android-frame.jsx. Presentational: wrap screen
 * content in <AndroidDevice>. The Material palette hexes are deliberate platform
 * chrome, not Midgard tokens. The chrome follows the console's colour mode
 * unless `dark` is given. [FR-C-3] */

const MD_C = {
  surface: '#f4fbf8',
  onSurface: '#171d1b',
  onSurfaceVar: '#49454f',
  frameBorder: 'rgba(116,119,117,0.5)',
};

function AndroidStatusBar({ dark = false }: { dark?: boolean }) {
  const c = dark ? '#fff' : MD_C.onSurface;
  return (
    <div
      style={{
        height: 40,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '0 16px',
        position: 'relative',
        fontFamily: 'Roboto, system-ui, sans-serif',
      }}
    >
      <div style={{ width: 128, display: 'flex', alignItems: 'center', gap: 8 }}>
        <span style={{ fontSize: 14, fontWeight: 400, letterSpacing: 0.25, lineHeight: '20px', color: c }}>9:30</span>
      </div>
      {/* camera punch-hole */}
      <div style={{ position: 'absolute', left: '50%', top: 8, transform: 'translateX(-50%)', width: 24, height: 24, borderRadius: 100, background: '#2e2e2e' }} />
      <div style={{ display: 'flex', alignItems: 'center' }}>
        <div style={{ display: 'flex', paddingRight: 2 }}>
          <svg width="16" height="16" viewBox="0 0 16 16" style={{ marginRight: -2 }}>
            <path d="M8 13.3L.67 5.97a10.37 10.37 0 0114.66 0L8 13.3z" fill={c} />
          </svg>
          <svg width="16" height="16" viewBox="0 0 16 16" style={{ marginRight: -2 }}>
            <path d="M14.67 14.67V1.33L1.33 14.67h13.34z" fill={c} />
          </svg>
        </div>
        <svg width="16" height="16" viewBox="0 0 16 16">
          <rect x="3.75" y="2" width="8.5" height="13" rx="1.5" fill={c} />
          <rect x="5.5" y="0.9" width="5" height="2" rx="0.5" fill={c} />
        </svg>
      </div>
    </div>
  );
}

function AndroidAppBar({ title }: { title: string }) {
  const iconDot = (
    <div style={{ width: 48, height: 48, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
      <div style={{ width: 22, height: 22, borderRadius: '50%', background: MD_C.onSurfaceVar, opacity: 0.3 }} />
    </div>
  );
  return (
    <div style={{ background: MD_C.surface, padding: '4px 4px 0' }}>
      <div style={{ height: 56, display: 'flex', alignItems: 'center', gap: 4 }}>
        {iconDot}
        <span style={{ flex: 1, fontSize: 22, fontWeight: 400, color: MD_C.onSurface, fontFamily: 'Roboto, system-ui, sans-serif' }}>{title}</span>
        {iconDot}
      </div>
    </div>
  );
}

function AndroidNavBar({ dark = false }: { dark?: boolean }) {
  return (
    <div style={{ height: 24, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
      <div style={{ width: 108, height: 4, borderRadius: 2, background: dark ? '#fff' : MD_C.onSurface, opacity: 0.4 }} />
    </div>
  );
}

export interface AndroidDeviceProps {
  children?: React.ReactNode;
  width?: number;
  height?: number;
  /** Dark platform chrome. Defaults to the console's resolved colour mode. */
  dark?: boolean;
  title?: string;
}

/** AndroidDevice — Material 3 bezel with status bar, optional app bar and gesture nav. */
export function AndroidDevice({ children, width = 300, height = 620, dark, title }: AndroidDeviceProps) {
  const mode = useResolvedMode();
  const isDark = dark ?? mode === 'dark';
  return (
    <div
      style={{
        width,
        height,
        borderRadius: 18,
        overflow: 'hidden',
        background: isDark ? '#1d1b20' : MD_C.surface,
        border: `8px solid ${MD_C.frameBorder}`,
        boxShadow: 'var(--shadow-lg)',
        display: 'flex',
        flexDirection: 'column',
        boxSizing: 'border-box',
      }}
    >
      <AndroidStatusBar dark={isDark} />
      {title !== undefined && <AndroidAppBar title={title} />}
      <div style={{ flex: 1, overflow: 'auto' }}>{children}</div>
      <AndroidNavBar dark={isDark} />
    </div>
  );
}
