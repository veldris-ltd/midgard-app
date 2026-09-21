import * as React from 'react';
import { useResolvedMode } from '@midgard/ui-shared';

/* IOSDevice — a simplified iOS device frame (bezel, dynamic island, status bar,
 * home indicator). Ported from docs/design/ui_kits/clients/ios-frame.jsx.
 * Presentational: wrap screen content in <IOSDevice> to get the bezel + chrome.
 * The Apple status-bar hexes are deliberate platform chrome, not Midgard tokens.
 * The chrome follows the console's colour mode unless `dark` is given, so the
 * status bar stays legible on a light screen. [FR-C-3] */

function IOSStatusBar({ dark = false, time = '9:41' }: { dark?: boolean; time?: string }) {
  const c = dark ? '#fff' : '#000';
  return (
    <div
      style={{
        display: 'flex',
        gap: 154,
        alignItems: 'center',
        justifyContent: 'center',
        padding: '21px 24px 19px',
        boxSizing: 'border-box',
        position: 'relative',
        zIndex: 20,
        width: '100%',
      }}
    >
      <div style={{ flex: 1, height: 22, display: 'flex', alignItems: 'center', justifyContent: 'center', paddingTop: 1.5 }}>
        <span style={{ fontFamily: '-apple-system, "SF Pro", system-ui', fontWeight: 590, fontSize: 17, lineHeight: '22px', color: c }}>
          {time}
        </span>
      </div>
      <div style={{ flex: 1, height: 22, display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 7, paddingTop: 1, paddingRight: 1 }}>
        <svg width="19" height="12" viewBox="0 0 19 12">
          <rect x="0" y="7.5" width="3.2" height="4.5" rx="0.7" fill={c} />
          <rect x="4.8" y="5" width="3.2" height="7" rx="0.7" fill={c} />
          <rect x="9.6" y="2.5" width="3.2" height="9.5" rx="0.7" fill={c} />
          <rect x="14.4" y="0" width="3.2" height="12" rx="0.7" fill={c} />
        </svg>
        <svg width="17" height="12" viewBox="0 0 17 12">
          <path d="M8.5 3.2C10.8 3.2 12.9 4.1 14.4 5.6L15.5 4.5C13.7 2.7 11.2 1.5 8.5 1.5C5.8 1.5 3.3 2.7 1.5 4.5L2.6 5.6C4.1 4.1 6.2 3.2 8.5 3.2Z" fill={c} />
          <path d="M8.5 6.8C9.9 6.8 11.1 7.3 12 8.2L13.1 7.1C11.8 5.9 10.2 5.1 8.5 5.1C6.8 5.1 5.2 5.9 3.9 7.1L5 8.2C5.9 7.3 7.1 6.8 8.5 6.8Z" fill={c} />
          <circle cx="8.5" cy="10.5" r="1.5" fill={c} />
        </svg>
        <svg width="27" height="13" viewBox="0 0 27 13">
          <rect x="0.5" y="0.5" width="23" height="12" rx="3.5" stroke={c} strokeOpacity="0.35" fill="none" />
          <rect x="2" y="2" width="20" height="9" rx="2" fill={c} />
          <path d="M25 4.5V8.5C25.8 8.2 26.5 7.2 26.5 6.5C26.5 5.8 25.8 4.8 25 4.5Z" fill={c} fillOpacity="0.4" />
        </svg>
      </div>
    </div>
  );
}

export interface IOSDeviceProps {
  children?: React.ReactNode;
  width?: number;
  height?: number;
  /** Dark platform chrome. Defaults to the console's resolved colour mode, so
   *  the status bar never paints white on a light screen. */
  dark?: boolean;
}

/** IOSDevice — iOS bezel with dynamic island, status bar and home indicator. */
export function IOSDevice({ children, width = 300, height = 620, dark }: IOSDeviceProps) {
  const mode = useResolvedMode();
  const isDark = dark ?? mode === 'dark';
  return (
    <div
      style={{
        width,
        height,
        borderRadius: 48,
        overflow: 'hidden',
        position: 'relative',
        background: isDark ? '#000' : '#F2F2F7',
        boxShadow: 'var(--shadow-lg)',
        fontFamily: '-apple-system, system-ui, sans-serif',
        WebkitFontSmoothing: 'antialiased',
      }}
    >
      {/* dynamic island */}
      <div
        style={{
          position: 'absolute',
          top: 11,
          left: '50%',
          transform: 'translateX(-50%)',
          width: 126,
          height: 37,
          borderRadius: 24,
          background: '#000',
          zIndex: 50,
        }}
      />
      {/* status bar (absolute) */}
      <div style={{ position: 'absolute', top: 0, left: 0, right: 0, zIndex: 10 }}>
        <IOSStatusBar dark={isDark} />
      </div>
      {/* content — starts below the status bar's safe area so the dynamic
          island never sits over the app */}
      <div style={{ height: '100%', display: 'flex', flexDirection: 'column', paddingTop: 59, boxSizing: 'border-box' }}>
        <div style={{ flex: 1, overflow: 'auto' }}>{children}</div>
      </div>
      {/* home indicator — always on top */}
      <div
        style={{
          position: 'absolute',
          bottom: 0,
          left: 0,
          right: 0,
          zIndex: 60,
          height: 34,
          display: 'flex',
          justifyContent: 'center',
          alignItems: 'flex-end',
          paddingBottom: 8,
          pointerEvents: 'none',
        }}
      >
        <div style={{ width: 139, height: 5, borderRadius: 100, background: isDark ? 'rgba(255,255,255,0.7)' : 'rgba(0,0,0,0.25)' }} />
      </div>
    </div>
  );
}
