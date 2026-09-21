/* Shared console shell for all UI-kit screens.
   Load AFTER ds-loader.js with <script type="text/babel" src="../kit-shell.jsx">.
   Exposes window.KitShell = { ConsoleShell, NAV }. Navigation items carry real
   hrefs so screens are separate pages within each kit folder. */
const NS = window.__mgNS();
const { ClassificationBanner, ProductMark, SideNav, StatusPill, IconButton, Icon, Terminal } = NS;

const NAV = {
  midgard: {
    banner: { level: 'confidential' },
    seals: [['secure', 'PQ-SEALED', true], ['online', 'AIR-GAP', false]],
    sections: [
      { title: 'FABRIC', items: [
        { id: 'objects', label: 'Object types', icon: 'boxes', href: 'index.html', badge: '312' },
        { id: 'search', label: 'Search-around', icon: 'search', href: 'search.html' },
      ]},
      { title: 'GOVERNANCE', items: [
        { id: 'policies', label: 'Policies & markings', icon: 'shield-check', href: 'policies.html' },
        { id: 'audit', label: 'Audit lineage', icon: 'scroll-text', href: 'audit.html' },
      ]},
      { title: 'PLATFORM', items: [
        { id: 'identity', label: 'Identity & access', icon: 'fingerprint', href: 'identity.html' },
        { id: 'kms', label: 'Keys & crypto', icon: 'key', href: 'kms.html' },
        { id: 'compute', label: 'Compute', icon: 'server', href: 'compute.html' },
        { id: 'sdk', label: 'SDK & APIs', icon: 'code', href: 'sdk.html' },
        { id: 'obs', label: 'Observability', icon: 'activity', href: 'observability.html' },
        { id: 'resilience', label: 'Resilience', icon: 'refresh-cw', href: 'resilience.html' },
      ]},
    ],
  },
  forge: {
    banner: { level: 'restricted', text: 'RESTRICTED · DATA PLANE' },
    seals: [['secure', 'PQ-SEALED', false]],
    sections: [
      { title: 'DATA OPS', items: [
        { id: 'pipeline', label: 'Pipelines', icon: 'workflow', href: 'index.html', badge: '38' },
        { id: 'connectors', label: 'Connectors', icon: 'database', href: 'connectors.html' },
        { id: 'datasets', label: 'Datasets', icon: 'table', href: 'datasets.html', badge: '412' },
        { id: 'health', label: 'Data health', icon: 'activity', href: 'health.html', badge: '3' },
        { id: 'schedules', label: 'Schedules', icon: 'clock', href: 'schedules.html' },
        { id: 'ide', label: 'Transform IDE', icon: 'code', href: 'ide.html' },
      ]},
      { title: 'FABRIC', items: [
        { id: 'projection', label: 'Object projection', icon: 'boxes', href: 'projection.html' },
      ]},
    ],
  },
  mimir: {
    banner: { level: 'confidential' },
    seals: [['online', 'INFERENCE · LOCAL', true], ['secure', 'NO EGRESS', false]],
    sections: [
      { title: 'BUILD', items: [
        { id: 'agents', label: 'Agents', icon: 'bot', href: 'index.html', badge: '12' },
        { id: 'logic', label: 'Logic (no-code)', icon: 'workflow', href: 'logic.html' },
        { id: 'sessions', label: 'Session traces', icon: 'terminal', href: 'sessions.html' },
      ]},
      { title: 'GOVERN', items: [
        { id: 'evals', label: 'Evals', icon: 'gauge', href: 'evals.html' },
        { id: 'guardrails', label: 'Guardrails', icon: 'shield-check', href: 'guardrails.html' },
        { id: 'feedback', label: 'Governed feedback', icon: 'refresh-cw', href: 'feedback.html' },
      ]},
      { title: 'SERVE', items: [
        { id: 'models', label: 'Model plane', icon: 'cpu', href: 'models.html', badge: '3' },
      ]},
    ],
  },
  bifrost: {
    banner: { level: 'internal', text: 'INTERNAL · RELEASE CONTROL PLANE' },
    seals: [['secure', 'ARTEFACTS ML-DSA ✓', false]],
    sections: [
      { title: 'DELIVERY', items: [
        { id: 'releases', label: 'Releases', icon: 'rocket', href: 'index.html' },
        { id: 'artefacts', label: 'Artefacts', icon: 'package', href: 'artefacts.html', badge: '58' },
        { id: 'channels', label: 'Channels', icon: 'git-branch', href: 'channels.html', badge: '3' },
        { id: 'bundles', label: 'Offline bundles', icon: 'download', href: 'bundles.html', badge: '2' },
      ]},
      { title: 'FLEET', items: [
        { id: 'fleet', label: 'Fleet & drift', icon: 'server', href: 'fleet.html' },
        { id: 'rollback', label: 'Rollback centre', icon: 'undo-2', href: 'rollback.html' },
      ]},
    ],
  },
  admin: {
    banner: { level: 'confidential', text: 'CONFIDENTIAL · MANAGEMENT ZONE · ADMIN TRAFFIC SEGREGATED' },
    seals: [['secure', 'SELF-GOVERNED', false], ['online', '7 SUBSYSTEMS', true]],
    sections: [
      { title: 'OVERSEE', items: [
        { id: 'overview', label: 'Overview', icon: 'gauge', href: 'index.html' },
        { id: 'approvals', label: 'Approvals', icon: 'users', href: 'approvals.html', badge: '3' },
      ]},
      { title: 'ADMINISTER', items: [
        { id: 'iam', label: 'Identity & roles', icon: 'fingerprint', href: 'iam.html' },
        { id: 'config', label: 'Config & policy', icon: 'git-branch', href: 'config.html' },
        { id: 'registry', label: 'Registry & governance', icon: 'cpu', href: 'registry.html' },
        { id: 'ledger', label: 'Audit ledger', icon: 'scroll-text', href: 'ledger.html' },
      ]},
      { title: 'EMERGENCY', items: [
        { id: 'breakglass', label: 'Break-glass', icon: 'alert-triangle', href: 'breakglass.html', badge: '1' },
      ]},
      { title: 'ASSIST', items: [
        { id: 'copilot', label: 'Admin copilot', icon: 'sparkles', href: 'copilot.html' },
      ]},
    ],
  },
  heimdall: {
    banner: { level: 'confidential', text: 'CONFIDENTIAL · OPERATIONAL PICTURE · RECIPIENT EYES ONLY' },
    seals: [['online', '12 FEEDS · FUSED', true], ['secure', 'FEEDS PQ-AUTH ✓', false], ['secure', 'SYNC · 3 SITES', false]],
    sections: [
      { title: 'PICTURE', items: [
        { id: 'cop', label: 'Operating picture', icon: 'map', href: 'index.html' },
        { id: 'feeds', label: 'Feeds', icon: 'radio-tower', href: 'feeds.html', badge: '12' },
        { id: 'entities', label: 'Entities', icon: 'crosshair', href: 'entities.html', badge: '1,204' },
        { id: 'sync', label: 'Picture sync', icon: 'network', href: 'sync.html' },
      ]},
      { title: 'DECIDE', items: [
        { id: 'workflows', label: 'Action workflows', icon: 'workflow', href: 'workflows.html', badge: '2' },
        { id: 'lineage', label: 'Decision lineage', icon: 'scroll-text', href: 'lineage.html' },
      ]},
    ],
  },
};

function useMedia(q) {
  const [m, setM] = React.useState(() => window.matchMedia(q).matches);
  React.useEffect(() => {
    const mq = window.matchMedia(q);
    const f = () => setM(mq.matches);
    mq.addEventListener ? mq.addEventListener('change', f) : mq.addListener(f);
    return () => { mq.removeEventListener ? mq.removeEventListener('change', f) : mq.removeListener(f); };
  }, [q]);
  return m;
}

function Scrim({ onClick }) {
  return <div onClick={onClick} style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.55)', zIndex: 90 }} />;
}

function PanelHandle({ side, open, onClick, label }) {
  const [h, setH] = React.useState(false);
  const chev = side === 'left' ? (open ? '‹' : '›') : (open ? '›' : '‹');
  return (
    <button type="button" aria-label={label} aria-expanded={open} title={label}
      onClick={onClick} onMouseEnter={() => setH(true)} onMouseLeave={() => setH(false)}
      style={{
        all: 'unset', boxSizing: 'border-box', width: 15, flex: 'none', alignSelf: 'stretch',
        display: 'flex', alignItems: 'center', justifyContent: 'center', cursor: 'pointer',
        background: h ? 'var(--accent-tint)' : 'var(--bg-sunken)',
        borderRight: side === 'left' ? '1px solid var(--border)' : 'none',
        borderLeft: side === 'right' ? '1px solid var(--border)' : 'none',
        color: h ? 'var(--accent-hover)' : 'var(--text-faint)',
        fontFamily: 'var(--font-mono)', fontSize: 12, lineHeight: 1,
        transition: 'background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out)',
      }}>
      {chev}
    </button>
  );
}

function ConsoleShell({ product, active, context, toolbar, aside, asideWidth = 330, navFooter, children, mainStyle }) {
  const cfg = NAV[product];
  const isMobile = useMedia('(max-width: 900px)');
  const isNarrow = useMedia('(max-width: 640px)');
  const [navOpen, setNavOpen] = React.useState(() => localStorage.getItem('mg-nav') !== 'closed');
  const [asideOpen, setAsideOpen] = React.useState(() => localStorage.getItem('mg-aside') !== 'closed');
  const [navDrawer, setNavDrawer] = React.useState(false);   // mobile overlays
  const [asideDrawer, setAsideDrawer] = React.useState(false);

  const toggleNav = () => {
    if (isMobile) { setNavDrawer(v => !v); return; }
    setNavOpen(v => { localStorage.setItem('mg-nav', v ? 'closed' : 'open'); return !v; });
  };
  const toggleAside = () => {
    if (isMobile) { setAsideDrawer(v => !v); return; }
    setAsideOpen(v => { localStorage.setItem('mg-aside', v ? 'closed' : 'open'); return !v; });
  };

  const sections = cfg.sections.map(s => ({
    title: s.title,
    items: s.items.map(it => ({ id: it.id, label: it.label, badge: it.badge, icon: <Icon name={it.icon} size={16} /> })),
  }));
  const hrefFor = (id) => {
    for (const s of cfg.sections) for (const it of s.items) if (it.id === id) return it.href;
    return null;
  };
  const go = (id) => { const h = hrefFor(id); if (h && id !== active) location.href = h; };

  /* Side navigation without its own border: the PanelHandle gutter carries the divider */
  const nav = (
    <SideNav sections={sections} activeId={active} onSelect={go} footer={navFooter}
      width={isMobile ? 'min(80vw, 280px)' : undefined} style={isMobile ? { height: '100%' } : { borderRight: 'none' }} />
  );
  /* Icon-only rail when collapsed on desktop */
  const rail = (
    <nav aria-label="Collapsed navigation" style={{ width: 52, flex: 'none', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 2, padding: '12px 0', background: 'var(--bg-sunken)', overflowY: 'auto' }}>
      {cfg.sections.flatMap(s => s.items).map(it => (
        <IconButton key={it.id} label={it.label} active={it.id === active} onClick={() => go(it.id)}>
          <Icon name={it.icon} size={17} />
        </IconButton>
      ))}
    </nav>
  );

  const asideBody = aside && (
    <aside style={isMobile ? {
      position: 'fixed', top: 0, right: 0, bottom: 0, zIndex: 100,
      width: 'min(92vw, ' + asideWidth + 'px)',
      borderLeft: '1px solid var(--border)', background: 'var(--bg-sunken)',
      padding: 18, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 14,
      boxShadow: 'var(--shadow-lg)',
    } : {
      width: asideWidth, flex: 'none', background: 'var(--bg-sunken)',
      padding: 18, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 14,
    }}>
      {isMobile && (
        <div style={{ display: 'flex', justifyContent: 'flex-end', margin: '-6px -6px 0 0' }}>
          <IconButton label="Close inspector" onClick={() => setAsideDrawer(false)}><Icon name="x" size={16} /></IconButton>
        </div>
      )}
      {aside}
    </aside>
  );

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh', background: 'var(--bg-page)' }}>
      <ClassificationBanner level={cfg.banner.level} text={cfg.banner.text} compact={isNarrow} />
      <header style={{ minHeight: 'var(--toolbar-h)', flex: 'none', display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: isNarrow ? 8 : 14, padding: isNarrow ? '6px 10px' : '6px 16px', borderBottom: '1px solid var(--border)', background: 'var(--bg-sunken)' }}>
        <ProductMark product={product} size="sm" showDescriptor={false} />
        {!isNarrow && <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: 'var(--ls-label)', color: 'var(--text-faint)' }}>{context}</span>}
        <div style={{ flex: 1 }} />
        {!isNarrow && cfg.seals.map(([st, lb, pulse]) => <StatusPill key={lb} status={st} label={lb} pulse={pulse} />)}
        {isNarrow && cfg.seals.slice(0, 1).map(([st, lb, pulse]) => <StatusPill key={lb} status={st} label={lb} pulse={pulse} />)}
        {toolbar}
      </header>
      <div style={{ flex: 1, display: 'flex', minHeight: 0, position: 'relative' }}>
        {!isMobile && (navOpen ? nav : rail)}
        <PanelHandle side="left" open={isMobile ? navDrawer : navOpen} onClick={toggleNav}
          label={(isMobile ? navDrawer : navOpen) ? 'Collapse navigation' : 'Expand navigation'} />
        {isMobile && navDrawer && (
          <>
            <Scrim onClick={() => setNavDrawer(false)} />
            <div style={{ position: 'fixed', top: 0, left: 0, bottom: 0, zIndex: 100, display: 'flex', boxShadow: 'var(--shadow-lg)' }}>{nav}</div>
          </>
        )}
        <main style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', padding: isNarrow ? '14px 12px' : '20px 24px', gap: 16, overflowY: 'auto', ...mainStyle }}>
          {children}
        </main>
        {aside && (
          <PanelHandle side="right" open={isMobile ? asideDrawer : asideOpen} onClick={toggleAside}
            label={(isMobile ? asideDrawer : asideOpen) ? 'Collapse inspector' : 'Expand inspector'} />
        )}
        {!isMobile && asideOpen && asideBody}
        {isMobile && asideDrawer && (<><Scrim onClick={() => setAsideDrawer(false)} />{asideBody}</>)}
      </div>
    </div>
  );
}

window.KitShell = { ConsoleShell, NAV, useMedia };
