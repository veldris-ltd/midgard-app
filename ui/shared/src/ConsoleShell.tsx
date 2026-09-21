import * as React from 'react';
import {
  ClassificationBanner,
  Icon,
  IconButton,
  ProductMark,
  SideNav,
  StatusPill,
} from '@midgard/design-system';
import { NAV, type ConsoleProduct } from './nav';
import { PanelHandle, Scrim, useMedia } from './responsive';

export interface ConsoleShellProps {
  product: ConsoleProduct;
  /** Active nav item id (matches NAV[product] item id). */
  active: string;
  /** Mono context line shown next to the product mark, e.g. "ENCLOSURE / OBJECT FABRIC". */
  context: string;
  toolbar?: React.ReactNode;
  aside?: React.ReactNode;
  asideWidth?: number;
  navFooter?: React.ReactNode;
  children?: React.ReactNode;
  mainStyle?: React.CSSProperties;
  /** Router hook: called with the target item's href + id on nav selection.
   *  Defaults to `location.href = href` to match the static prototype. */
  onNavigate?: (href: string, id: string) => void;
}

/**
 * The shared console frame for every Midgard product console.
 * Anatomy: classification banner -> top bar (mark + context + seals + toolbar)
 * -> [ side nav | main | inspector rail ]. Nav collapses to a 52px icon rail
 * (persisted), and below 900px both nav and inspector become overlay drawers.
 * Ported from docs/design/ui_kits/kit-shell.jsx. `[NFR-SEC-2]`
 */
export function ConsoleShell({
  product,
  active,
  context,
  toolbar,
  aside,
  asideWidth = 330,
  navFooter,
  children,
  mainStyle,
  onNavigate,
}: ConsoleShellProps) {
  const cfg = NAV[product];
  const isMobile = useMedia('(max-width: 900px)');
  const isNarrow = useMedia('(max-width: 640px)');
  const [navOpen, setNavOpen] = React.useState(() => localStorage.getItem('mg-nav') !== 'closed');
  const [asideOpen, setAsideOpen] = React.useState(() => localStorage.getItem('mg-aside') !== 'closed');
  const [navDrawer, setNavDrawer] = React.useState(false);
  const [asideDrawer, setAsideDrawer] = React.useState(false);

  const toggleNav = () => {
    if (isMobile) {
      setNavDrawer((v) => !v);
      return;
    }
    setNavOpen((v) => {
      localStorage.setItem('mg-nav', v ? 'closed' : 'open');
      return !v;
    });
  };
  const toggleAside = () => {
    if (isMobile) {
      setAsideDrawer((v) => !v);
      return;
    }
    setAsideOpen((v) => {
      localStorage.setItem('mg-aside', v ? 'closed' : 'open');
      return !v;
    });
  };

  const sections = cfg.sections.map((s) => ({
    title: s.title,
    items: s.items.map((it) => ({
      id: it.id,
      label: it.label,
      badge: it.badge,
      icon: <Icon name={it.icon} size={16} />,
    })),
  }));

  const hrefFor = (id: string): string | null => {
    for (const s of cfg.sections) for (const it of s.items) if (it.id === id) return it.href;
    return null;
  };
  const go = (id: string) => {
    if (id === active) return;
    const href = hrefFor(id);
    if (!href) return;
    if (onNavigate) onNavigate(href, id);
    else window.location.href = href;
    if (isMobile) setNavDrawer(false);
  };

  const nav = (
    <SideNav
      sections={sections}
      activeId={active}
      onSelect={go}
      footer={navFooter}
      width={isMobile ? 'min(80vw, 280px)' : undefined}
      style={isMobile ? { height: '100%' } : { borderRight: 'none' }}
    />
  );

  const rail = (
    <nav
      aria-label="Collapsed navigation"
      style={{
        width: 52,
        flex: 'none',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        gap: 2,
        padding: '12px 0',
        background: 'var(--bg-sunken)',
        overflowY: 'auto',
      }}
    >
      {cfg.sections
        .flatMap((s) => s.items)
        .map((it) => (
          <IconButton key={it.id} label={it.label} active={it.id === active} onClick={() => go(it.id)}>
            <Icon name={it.icon} size={17} />
          </IconButton>
        ))}
    </nav>
  );

  const asideBody = aside && (
    <aside
      style={
        isMobile
          ? {
              position: 'fixed',
              top: 0,
              right: 0,
              bottom: 0,
              zIndex: 100,
              width: `min(92vw, ${asideWidth}px)`,
              borderLeft: '1px solid var(--border)',
              background: 'var(--bg-sunken)',
              padding: 18,
              overflowY: 'auto',
              display: 'flex',
              flexDirection: 'column',
              gap: 14,
              boxShadow: 'var(--shadow-lg)',
            }
          : {
              width: asideWidth,
              flex: 'none',
              background: 'var(--bg-sunken)',
              padding: 18,
              overflowY: 'auto',
              display: 'flex',
              flexDirection: 'column',
              gap: 14,
            }
      }
    >
      {isMobile && (
        <div style={{ display: 'flex', justifyContent: 'flex-end', margin: '-6px -6px 0 0' }}>
          <IconButton label="Close inspector" onClick={() => setAsideDrawer(false)}>
            <Icon name="x" size={16} />
          </IconButton>
        </div>
      )}
      {aside}
    </aside>
  );

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh', background: 'var(--bg-page)' }}>
      <ClassificationBanner level={cfg.banner.level} text={cfg.banner.text} compact={isNarrow} />
      <header
        style={{
          minHeight: 'var(--toolbar-h)',
          flex: 'none',
          display: 'flex',
          alignItems: 'center',
          flexWrap: 'wrap',
          gap: isNarrow ? 8 : 14,
          padding: isNarrow ? '6px 10px' : '6px 16px',
          borderBottom: '1px solid var(--border)',
          background: 'var(--bg-sunken)',
        }}
      >
        <ProductMark product={product} size="sm" showDescriptor={false} />
        {!isNarrow && (
          <span
            style={{
              fontFamily: 'var(--font-mono)',
              fontSize: 'var(--fs-micro)',
              letterSpacing: 'var(--ls-label)',
              color: 'var(--text-faint)',
            }}
          >
            {context}
          </span>
        )}
        <div style={{ flex: 1 }} />
        {!isNarrow && cfg.seals.map(([st, lb, pulse]) => <StatusPill key={lb} status={st} label={lb} pulse={pulse} />)}
        {isNarrow && cfg.seals.slice(0, 1).map(([st, lb, pulse]) => <StatusPill key={lb} status={st} label={lb} pulse={pulse} />)}
        {toolbar}
      </header>
      <div style={{ flex: 1, display: 'flex', minHeight: 0, position: 'relative' }}>
        {!isMobile && (navOpen ? nav : rail)}
        <PanelHandle
          side="left"
          open={isMobile ? navDrawer : navOpen}
          onClick={toggleNav}
          label={(isMobile ? navDrawer : navOpen) ? 'Collapse navigation' : 'Expand navigation'}
        />
        {isMobile && navDrawer && (
          <>
            <Scrim onClick={() => setNavDrawer(false)} />
            <div style={{ position: 'fixed', top: 0, left: 0, bottom: 0, zIndex: 100, display: 'flex', boxShadow: 'var(--shadow-lg)' }}>
              {nav}
            </div>
          </>
        )}
        <main
          style={{
            flex: 1,
            minWidth: 0,
            display: 'flex',
            flexDirection: 'column',
            padding: isNarrow ? '14px 12px' : '20px 24px',
            gap: 16,
            overflowY: 'auto',
            ...mainStyle,
          }}
        >
          {children}
        </main>
        {aside && (
          <PanelHandle
            side="right"
            open={isMobile ? asideDrawer : asideOpen}
            onClick={toggleAside}
            label={(isMobile ? asideDrawer : asideOpen) ? 'Collapse inspector' : 'Expand inspector'}
          />
        )}
        {!isMobile && asideOpen && asideBody}
        {isMobile && asideDrawer && (
          <>
            <Scrim onClick={() => setAsideDrawer(false)} />
            {asideBody}
          </>
        )}
      </div>
    </div>
  );
}
