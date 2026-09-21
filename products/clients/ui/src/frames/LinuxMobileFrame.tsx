import * as React from 'react';

/* LinuxMobileFrame — a Phosh-style Linux-mobile bezel. Ported from the
 * LinuxMobileFrame helper in docs/design/ui_kits/clients/mobile.html.
 * Presentational wrapper taking children. */

export interface LinuxMobileFrameProps {
  children?: React.ReactNode;
  width?: number;
  height?: number;
}

/** LinuxMobileFrame — GTK/Phosh mobile shell bezel over the shared core. */
export function LinuxMobileFrame({ children, width = 300, height = 560 }: LinuxMobileFrameProps) {
  return (
    <div style={{ width, borderRadius: 24, border: '2px solid var(--border-strong)', background: 'var(--bg-sunken)', overflow: 'hidden', boxShadow: 'var(--shadow-lg)' }}>
      <div style={{ height: 22, display: 'flex', alignItems: 'center', padding: '0 12px', fontFamily: 'var(--font-mono)', fontSize: 8, color: 'var(--text-faint)', borderBottom: '1px solid var(--divider)', gap: 8 }}>
        <span>14:32</span>
        <span style={{ marginLeft: 'auto' }}>PHOSH · WLAN ✕ · ▮▮▮</span>
      </div>
      <div style={{ height }}>{children}</div>
      <div style={{ height: 26, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <span style={{ width: 60, height: 3.5, borderRadius: 2, background: 'var(--border-strong)' }} />
      </div>
    </div>
  );
}
