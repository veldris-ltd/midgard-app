import * as React from 'react';
import { Link, useLocation } from 'react-router-dom';
import { ClassificationBanner, SectionLabel, Tag, ProvenanceTag } from '@midgard/design-system';
import { ModeToggle } from '@midgard/ui-shared';
import { ChromeWindow } from './frames/ChromeWindow';
import { IOSDevice } from './frames/IOSDevice';
import { AndroidDevice } from './frames/AndroidDevice';
import { LinuxMobileFrame } from './frames/LinuxMobileFrame';
import { DesktopFrame, type DesktopOs } from './frames/DesktopFrame';
import { ClientApp } from './ClientApp';
import { ExtensionPopup } from './ExtensionPopup';

/* App shell — a device gallery. One offline-first core (`ClientApp`) shown
 * inside each platform's chrome, with a top switcher to move between the web,
 * mobile and desktop targets. Reproduces the three client prototypes. */

type Target = 'web' | 'mobile' | 'desktop';

const TARGETS: Array<{ key: Target; path: string; label: string }> = [
  { key: 'web', path: '/', label: 'Web' },
  { key: 'mobile', path: '/mobile', label: 'Mobile' },
  { key: 'desktop', path: '/desktop', label: 'Desktop' },
];

const HEADINGS: Record<Target, { n: number; eyebrow: string; title: string; tags: Array<[string, string]> }> = {
  web: {
    n: 3,
    eyebrow: 'CLIENTS · WEB',
    title: 'Web app & the least-capable extension',
    tags: [
      ['FR-C-8', 'Minimal-privilege extension: no raw keys, brokered operations'],
      ['NFR-SEC-1', 'Zero-trust: identity, device posture, purpose'],
      ['AC-C-13', 'Server-side marking enforcement; nothing above clearance rendered or cached'],
    ],
  },
  mobile: {
    n: 1,
    eyebrow: 'CLIENTS · MOBILE',
    title: 'One core, three mobile shells',
    tags: [
      ['FR-C-1', 'Crypto, sync, storage once in a shared Rust core'],
      ['FR-C-3', "Platform-native shells meeting each OS's HIG"],
      ['FR-C-4', 'Offline-first CRDT sync'],
    ],
  },
  desktop: {
    n: 2,
    eyebrow: 'CLIENTS · DESKTOP',
    title: 'Desktop · one binary, native chrome',
    tags: [
      ['FR-C-1', 'One shared Rust core'],
      ['FR-C-5', 'Hybrid PQ-mTLS; fail closed'],
      ['FR-C-2', 'Keystore abstraction with per-platform floors'],
    ],
  },
};

const captionStyle: React.CSSProperties = { color: 'var(--text-faint)' };

function GalleryShell({ target, children }: { target: Target; children: React.ReactNode }) {
  const loc = useLocation();
  const h = HEADINGS[target];
  return (
    <div style={{ minHeight: '100vh', display: 'flex', flexDirection: 'column' }}>
      <ClassificationBanner level="confidential" />
      <div style={{ padding: '22px 28px 10px', display: 'flex', alignItems: 'flex-end', gap: 24, flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={h.n}>{h.eyebrow}</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>{h.title}</h1>
        </div>
        <div style={{ flex: 1 }} />
        <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
          {TARGETS.map((t) => (
            <Link key={t.key} to={t.path} style={{ textDecoration: 'none' }}>
              <Tag active={loc.pathname === t.path || (t.path === '/' && loc.pathname === '')} onClick={() => undefined}>
                {t.label}
              </Tag>
            </Link>
          ))}
          <ModeToggle />
        </div>
      </div>
      <div style={{ padding: '0 28px 10px', display: 'flex', gap: 6, flexWrap: 'wrap' }}>
        {h.tags.map(([id, title]) => (
          <ProvenanceTag key={id} id={id} title={title} />
        ))}
      </div>
      {children}
    </div>
  );
}

/** Web target — enclosure-hosted WASM app + the minimal-privilege extension popup. */
export function WebPage() {
  return (
    <GalleryShell target="web">
      <div style={{ flex: 1, display: 'flex', justifyContent: 'center', padding: '10px 24px 34px', overflow: 'auto' }}>
        <div style={{ position: 'relative', flex: 'none' }}>
          <ChromeWindow width={1000} height={620} url="fabric.enclosure.internal/workspace">
            <ClientApp platform="web" />
          </ChromeWindow>
          <ExtensionPopup />
        </div>
      </div>
    </GalleryShell>
  );
}

/** Mobile target — iOS, Android and Linux-mobile shells over the shared core.
 *  The device frames take their chrome from the colour mode. [FR-C-3] */
export function MobilePage() {
  return (
    <GalleryShell target="mobile">
      <div style={{ flex: 1, display: 'flex', gap: 34, alignItems: 'flex-start', justifyContent: 'center', flexWrap: 'wrap', padding: '18px 28px 34px' }}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10, alignItems: 'center' }}>
          <IOSDevice>
            <ClientApp platform="ios" density="compact" />
          </IOSDevice>
          <span className="mg-label" style={captionStyle}>iOS · SWIFTUI · APP STORE</span>
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10, alignItems: 'center' }}>
          <AndroidDevice>
            <ClientApp platform="android" density="compact" />
          </AndroidDevice>
          <span className="mg-label" style={captionStyle}>ANDROID · COMPOSE · PLAY + APK</span>
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10, alignItems: 'center' }}>
          <LinuxMobileFrame>
            <ClientApp platform="linux" density="compact" />
          </LinuxMobileFrame>
          <span className="mg-label" style={captionStyle}>LINUX MOBILE · GTK · FLATPAK</span>
        </div>
      </div>
    </GalleryShell>
  );
}

const OS_CAPTION: Record<DesktopOs, string> = {
  macos: 'MACOS · TAURI/SWIFTUI · DMG NOTARISED',
  windows: 'WINDOWS · TAURI + SVELTEKIT · MSI / STORE',
  linux: 'LINUX · TAURI + SVELTEKIT · FLATPAK / AUR',
};

/** Desktop target — Windows, macOS and Linux desktop chrome over the shared core. */
export function DesktopPage() {
  const [os, setOs] = React.useState<DesktopOs>('macos');
  return (
    <GalleryShell target="desktop">
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 12, padding: '14px 28px 34px' }}>
        <div style={{ display: 'flex', gap: 6 }}>
          {(['macos', 'windows', 'linux'] as DesktopOs[]).map((k) => (
            <Tag key={k} active={os === k} onClick={() => setOs(k)}>
              {k === 'macos' ? 'macOS' : k === 'windows' ? 'Windows' : 'Linux'}
            </Tag>
          ))}
        </div>
        <div style={{ maxWidth: '100%', overflowX: 'auto' }}>
          <DesktopFrame os={os} width={820} height={520}>
            <ClientApp platform={os} />
          </DesktopFrame>
        </div>
        <span className="mg-label" style={captionStyle}>{OS_CAPTION[os]}</span>
      </div>
    </GalleryShell>
  );
}
