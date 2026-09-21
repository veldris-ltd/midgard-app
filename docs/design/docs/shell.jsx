/* Midgard docs shell, shared chrome for the documentation pages.
   Loaded with <script type="text/babel" src="shell.jsx"> AFTER ds-loader.js.
   Exposes window.Docs. */
const NS = window.__mgNS();
const { ProductMark, Icon, ClassificationBanner, SectionLabel, Badge } = NS;

const PAGES = [
  { id: 'overview', label: 'Overview', href: 'index.html' },
  { id: 'foundations', label: 'Foundations', href: 'foundations.html' },
  { id: 'components', label: 'Components', href: 'components.html' },
  { id: 'patterns', label: 'Patterns', href: 'patterns.html' },
  { id: 'screens', label: 'Screens', href: 'screens.html' },
  { id: 'clients', label: 'Clients', href: 'clients.html' },
  { id: 'admin', label: 'Admin', href: 'admin.html' },
  { id: 'coverage', label: 'Coverage', href: 'coverage.html' },
  { id: 'flows', label: 'User flows', href: 'flows.html' },
];

const PLATFORMS = ['midgard', 'forge', 'mimir', 'bifrost', 'heimdall'];
const PLATFORM_HEX = { midgard: 'var(--midgard-500)', forge: 'var(--forge-500)', mimir: 'var(--mimir-500)', bifrost: 'var(--bifrost-500)', heimdall: 'var(--heimdall-500)' };

function getPlatform() {
  const p = localStorage.getItem('mg-platform');
  return PLATFORMS.includes(p) ? p : 'midgard';
}

function PlatformSwitcher({ value, onChange }) {
  return (
    <div role="radiogroup" aria-label="Platform scope" style={{ display: 'flex', gap: 2, background: 'var(--surface-inset)', border: '1px solid var(--border)', borderRadius: 'var(--radius-sm)', padding: 3 }}>
      {PLATFORMS.map(p => {
        const active = value === p;
        return (
          <button key={p} type="button" role="radio" aria-checked={active} onClick={() => onChange(p)}
            title={p.charAt(0).toUpperCase() + p.slice(1)}
            style={{
              all: 'unset', boxSizing: 'border-box', display: 'inline-flex', alignItems: 'center', gap: 6,
              padding: '4px 9px', borderRadius: 4, cursor: 'pointer',
              fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.08em', textTransform: 'uppercase',
              color: active ? 'var(--text-heading)' : 'var(--text-faint)',
              background: active ? 'var(--surface-card)' : 'transparent',
              boxShadow: active ? 'inset 0 0 0 1px var(--border-strong)' : 'none',
              transition: 'background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out)',
            }}>
            <span style={{ width: 8, height: 8, borderRadius: 2, background: PLATFORM_HEX[p], flex: 'none' }} />
            {active ? p : p.slice(0, 1)}
          </button>
        );
      })}
    </div>
  );
}

function Dropdown({ label, valueNode, options, onPick, width = 168 }) {
  const [open, setOpen] = React.useState(false);
  const [alignLeft, setAlignLeft] = React.useState(true);
  const ref = React.useRef(null);
  const toggle = () => {
    if (!open && ref.current) {
      const r = ref.current.getBoundingClientRect();
      /* anchor to whichever edge keeps the menu on screen */
      setAlignLeft(r.left + width <= window.innerWidth - 8);
    }
    setOpen(o => !o);
  };
  React.useEffect(() => {
    if (!open) return;
    const close = (e) => { if (ref.current && !ref.current.contains(e.target)) setOpen(false); };
    document.addEventListener('mousedown', close);
    const esc = (e) => { if (e.key === 'Escape') setOpen(false); };
    document.addEventListener('keydown', esc);
    return () => { document.removeEventListener('mousedown', close); document.removeEventListener('keydown', esc); };
  }, [open]);
  return (
    <span ref={ref} style={{ position: 'relative', display: 'inline-flex' }}>
      <button type="button" aria-haspopup="listbox" aria-expanded={open} aria-label={label} title={label}
        onClick={toggle}
        style={{
          all: 'unset', boxSizing: 'border-box', display: 'inline-flex', alignItems: 'center', gap: 7,
          height: 26, padding: '0 8px', borderRadius: 5, cursor: 'pointer',
          background: open ? 'var(--surface-raised)' : 'var(--surface-card)',
          boxShadow: 'inset 0 0 0 1px ' + (open ? 'var(--border-strong)' : 'var(--border)'),
          fontFamily: 'var(--font-mono)', fontSize: 9.5, letterSpacing: '0.08em', textTransform: 'uppercase',
          color: 'var(--text-body)',
          transition: 'background var(--dur-fast) var(--ease-out)',
        }}>
        {valueNode}
        <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" style={{ color: 'var(--text-faint)', transform: open ? 'rotate(180deg)' : 'none', transition: 'transform var(--dur-fast) var(--ease-out)' }}><path d="m6 9 6 6 6-6" /></svg>
      </button>
      {open && (
        <div role="listbox" aria-label={label} style={{
          position: 'absolute', top: 'calc(100% + 5px)', ...(alignLeft ? { left: 0 } : { right: 0 }), zIndex: 200, width,
          background: 'var(--surface-card)', border: '1px solid var(--border-strong)',
          borderRadius: 'var(--radius-sm)', boxShadow: 'var(--shadow-md)', padding: 4,
          display: 'flex', flexDirection: 'column', gap: 1,
        }}>
          {options.map(o => (
            <button key={o.id} type="button" role="option" aria-selected={o.active}
              onClick={() => { onPick(o.id); setOpen(false); }}
              style={{
                all: 'unset', boxSizing: 'border-box', display: 'flex', alignItems: 'center', gap: 8,
                padding: '6px 8px', borderRadius: 4, cursor: 'pointer',
                fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.08em', textTransform: 'uppercase',
                color: o.active ? 'var(--text-heading)' : 'var(--text-muted)',
                background: o.active ? 'var(--accent-tint)' : 'transparent',
                boxShadow: o.active ? 'inset 2px 0 0 var(--accent)' : 'none',
              }}
              onMouseEnter={(e) => { if (!o.active) e.currentTarget.style.background = 'var(--surface-raised)'; }}
              onMouseLeave={(e) => { if (!o.active) e.currentTarget.style.background = 'transparent'; }}>
              {o.node}
              {o.active && <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="var(--accent-hover)" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" style={{ marginLeft: 'auto' }}><path d="M20 6 9 17l-5-5" /></svg>}
            </button>
          ))}
        </div>
      )}
    </span>
  );
}

function ModeToggle() {
  const [mode, setModeState] = React.useState(window.__mgMode.get());
  React.useEffect(() => {
    const f = () => setModeState(window.__mgMode.get());
    window.addEventListener('mg-mode', f);
    return () => window.removeEventListener('mg-mode', f);
  }, []);
  const opts = [
    { id: 'dark', icon: 'eye-off', label: 'Dark' },
    { id: 'light', icon: 'eye', label: 'Light' },
    { id: 'system', icon: 'settings', label: 'System' },
  ];
  const cur = opts.find(o => o.id === mode) || opts[2];
  return (
    <Dropdown label="Colour mode" width={132}
      valueNode={<span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}><Icon name={cur.icon} size={12} />{cur.label}</span>}
      options={opts.map(o => ({ id: o.id, active: o.id === mode, node: <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}><Icon name={o.icon} size={12} />{o.label}</span> }))}
      onPick={(id) => window.__mgMode.set(id)}
    />
  );
}

function ControlCluster({ platform, onPlatform }) {
  const swatch = (p, size = 8) => <span style={{ width: size, height: size, borderRadius: 2, background: PLATFORM_HEX[p], flex: 'none' }} />;
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
      <Dropdown label="Platform scope" width={158}
        valueNode={<span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, color: 'var(--text-heading)' }}>{swatch(platform)}{platform}</span>}
        options={PLATFORMS.map(p => ({ id: p, active: p === platform, node: <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>{swatch(p)}{p}</span> }))}
        onPick={onPlatform}
      />
      <ModeToggle />
    </div>
  );
}

function BurgerNav({ active }) {
  const [open, setOpen] = React.useState(false);
  React.useEffect(() => {
    if (!open) return;
    const esc = (e) => { if (e.key === 'Escape') setOpen(false); };
    document.addEventListener('keydown', esc);
    return () => document.removeEventListener('keydown', esc);
  }, [open]);
  return (
    <>
      <button type="button" aria-label="Open navigation" aria-expanded={open} onClick={() => setOpen(true)}
        style={{
          all: 'unset', boxSizing: 'border-box', display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          width: 32, height: 28, borderRadius: 5, cursor: 'pointer',
          background: 'var(--surface-card)', boxShadow: 'inset 0 0 0 1px var(--border)',
          color: 'var(--text-body)',
        }}>
        <Icon name="menu" size={15} />
      </button>
      {open && ReactDOM.createPortal(
        <>
          <div onClick={() => setOpen(false)} style={{ position: 'fixed', inset: 0, zIndex: 190, background: 'rgba(0,0,0,0.5)' }} />
          <nav aria-label="Documentation pages" style={{
            position: 'fixed', top: 0, right: 0, bottom: 0, zIndex: 200, width: 'min(78vw, 280px)',
            background: 'var(--bg-sunken)', borderLeft: '1px solid var(--border)', boxShadow: 'var(--shadow-lg)',
            display: 'flex', flexDirection: 'column', padding: '14px 12px', gap: 2, overflowY: 'auto',
          }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '2px 8px 12px' }}>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: 'var(--ls-label)', color: 'var(--text-faint)' }}>DESIGN SYSTEM</span>
              <button type="button" aria-label="Close navigation" onClick={() => setOpen(false)}
                style={{ all: 'unset', cursor: 'pointer', marginLeft: 'auto', width: 28, height: 28, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', borderRadius: 5, color: 'var(--text-muted)' }}>
                <Icon name="x" size={15} />
              </button>
            </div>
            {PAGES.map(p => {
              const isActive = p.id === active;
              return (
                <a key={p.id} href={p.href} style={{
                  display: 'flex', alignItems: 'center', minHeight: 44, padding: '0 12px',
                  borderRadius: 'var(--radius-sm)', textDecoration: 'none',
                  fontSize: 'var(--fs-sm)', fontWeight: isActive ? 600 : 400,
                  color: isActive ? 'var(--text-heading)' : 'var(--text-muted)',
                  background: isActive ? 'var(--accent-tint)' : 'transparent',
                  boxShadow: isActive ? 'inset 2px 0 0 var(--accent)' : 'none',
                }}>{p.label}</a>
              );
            })}
          </nav>
        </>,
        document.body
      )}
    </>
  );
}

function DocsShell({ active, children }) {
  const [platform, setPlatform] = React.useState(getPlatform());
  const choose = (p) => { localStorage.setItem('mg-platform', p); setPlatform(p); };
  /* Burger unless the whole menu fits on one line: measure the full inline
     nav against the space the header can give it. */
  const headerRef = React.useRef(null);
  const measureRef = React.useRef(null);
  const [burger, setBurger] = React.useState(false);
  React.useEffect(() => {
    const check = () => {
      if (!headerRef.current || !measureRef.current) return;
      const headerW = headerRef.current.clientWidth;
      const navW = measureRef.current.scrollWidth;
      /* logo + label + controls + paddings/gaps ≈ 460px of fixed chrome */
      setBurger(navW + 470 > headerW);
    };
    check();
    window.addEventListener('resize', check);
    return () => window.removeEventListener('resize', check);
  }, []);
  const navLinks = (
    <nav style={{ display: 'flex', gap: 2, whiteSpace: 'nowrap' }}>
      {PAGES.map(p => {
        const isActive = p.id === active;
        return (
          <a key={p.id} href={p.href} style={{
            padding: '7px 10px', borderRadius: 'var(--radius-sm)',
            fontSize: 'var(--fs-sm)', fontWeight: isActive ? 600 : 400,
            color: isActive ? 'var(--text-heading)' : 'var(--text-muted)',
            background: isActive ? 'var(--accent-tint)' : 'transparent',
            boxShadow: isActive ? 'inset 0 -2px 0 var(--accent)' : 'none',
            textDecoration: 'none', whiteSpace: 'nowrap',
          }}>{p.label}</a>
        );
      })}
    </nav>
  );
  return (
    <div className={'theme-' + platform} style={{ minHeight: '100vh', display: 'flex', flexDirection: 'column', background: 'var(--bg-page)' }}>
      <ClassificationBanner level="internal" text="INTERNAL · MIDGARD PLATFORM DESIGN SYSTEM v1.0" compact />
      <header ref={headerRef} style={{
        position: 'sticky', top: 0, zIndex: 50, flex: 'none',
        display: 'flex', alignItems: 'center', gap: '6px 14px',
        padding: '8px clamp(10px, 2.5vw, 20px)', minHeight: 58,
        background: 'color-mix(in srgb, var(--bg-sunken) 82%, transparent)',
        backdropFilter: 'var(--blur-panel)', WebkitBackdropFilter: 'var(--blur-panel)',
        borderBottom: '1px solid var(--border)',
      }}>
        {/* invisible measuring copy of the full nav */}
        <div ref={measureRef} aria-hidden="true" style={{ position: 'absolute', visibility: 'hidden', pointerEvents: 'none', display: 'flex', gap: 2, whiteSpace: 'nowrap', left: -9999 }}>
          {PAGES.map(p => <span key={p.id} style={{ padding: '7px 10px', fontSize: 'var(--fs-sm)' }}>{p.label}</span>)}
        </div>
        <a href="index.html" style={{ display: 'inline-flex', textDecoration: 'none', flex: 'none' }}>
          <ProductMark product={platform} size="sm" showDescriptor={false} />
        </a>
        {!burger && <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: 'var(--ls-label)', color: 'var(--text-faint)' }}>DESIGN SYSTEM</span>}
        {!burger && navLinks}
        <div style={{ flex: 1 }} />
        <ControlCluster platform={platform} onPlatform={choose} />
        {burger && <BurgerNav active={active} />}
      </header>
      <main style={{ flex: 1, width: '100%', maxWidth: 1200, margin: '0 auto', padding: '48px clamp(12px, 4vw, 28px) 96px' }}>
        {children}
      </main>
      <footer style={{ borderTop: '1px solid var(--border)', padding: '22px clamp(14px, 3vw, 28px)', display: 'flex', alignItems: 'center', gap: '8px 18px', flexWrap: 'wrap', background: 'var(--bg-sunken)' }}>
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.14em', color: 'var(--text-faint)' }}>MIDGARD PLATFORM DESIGN SYSTEM · VELDRIS LTD.</span>
        <div style={{ flex: 1 }} />
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.14em', color: 'var(--text-faint)' }}>PQ-SEALED · NO EGRESS · AIR-GAP READY</span>
      </footer>
    </div>
  );
}

function PageIntro({ num, eyebrow, title, lead }) {
  return (
    <div style={{ marginBottom: 44 }}>
      <SectionLabel number={num}>{eyebrow}</SectionLabel>
      <h1 style={{ fontSize: 'var(--fs-h1)', marginTop: 12, maxWidth: 720 }}>{title}</h1>
      {lead && <p style={{ fontSize: 'var(--fs-body-lg)', lineHeight: 1.7, color: 'var(--text-muted)', maxWidth: 680, marginTop: 14, textWrap: 'pretty' }}>{lead}</p>}
    </div>
  );
}

function Section({ id, num, title, lead, children }) {
  return (
    <section id={id} style={{ marginBottom: 72, scrollMarginTop: 90 }}>
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 14, borderBottom: '1px solid var(--border)', paddingBottom: 12, marginBottom: 22 }}>
        {num != null && <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--accent)', letterSpacing: '0.1em' }}>{String(num).padStart(2, '0')} //</span>}
        <h2 style={{ fontSize: 'var(--fs-h3)' }}>{title}</h2>
      </div>
      {lead && <p style={{ fontSize: 'var(--fs-body)', lineHeight: 1.7, color: 'var(--text-muted)', maxWidth: 680, margin: '0 0 22px', textWrap: 'pretty' }}>{lead}</p>}
      {children}
    </section>
  );
}

function SubHead({ children }) {
  return <div style={{ fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: 'var(--ls-label)', textTransform: 'uppercase', color: 'var(--text-faint)', margin: '26px 0 12px' }}>{children}</div>;
}

function Demo({ label, theme, grid, children, style }) {
  return (
    <div style={{ border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', overflow: 'hidden', marginBottom: 14 }}>
      {label && <div style={{ padding: '7px 14px', borderBottom: '1px solid var(--border)', background: 'var(--surface-inset)', fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.14em', textTransform: 'uppercase', color: 'var(--text-faint)' }}>{label}</div>}
      <div className={(theme ? 'theme-' + theme : '') + (grid ? ' mg-grid-bg' : '')} style={{ padding: 22, display: 'flex', gap: 14, flexWrap: 'wrap', alignItems: 'center', background: 'var(--bg-page)', ...style }}>
        {children}
      </div>
    </div>
  );
}

function Code({ children }) {
  return (
    <pre style={{ margin: '0 0 14px', padding: '13px 16px', background: 'var(--bg-sunken)', border: '1px solid var(--border)', borderRadius: 'var(--radius-sm)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', lineHeight: 1.8, color: 'var(--text-body)', overflowX: 'auto' }}>{children}</pre>
  );
}

function PropsTable({ rows }) {
  return (
    <div style={{ border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', overflowX: 'auto', marginBottom: 14 }}>
      <table style={{ width: '100%', minWidth: 520, borderCollapse: 'collapse', fontSize: 'var(--fs-xs)' }}>
        <thead><tr style={{ background: 'var(--surface-inset)' }}>
          {['Prop', 'Type', 'Notes'].map(h => <th key={h} style={{ textAlign: 'left', padding: '7px 14px', fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.14em', textTransform: 'uppercase', color: 'var(--text-faint)', borderBottom: '1px solid var(--border)' }}>{h}</th>)}
        </tr></thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i}>
              <td style={{ padding: '7px 14px', fontFamily: 'var(--font-mono)', color: 'var(--accent-hover)', borderBottom: i < rows.length - 1 ? '1px solid var(--divider)' : 'none', whiteSpace: 'nowrap' }}>{r[0]}</td>
              <td style={{ padding: '7px 14px', fontFamily: 'var(--font-mono)', color: 'var(--text-muted)', borderBottom: i < rows.length - 1 ? '1px solid var(--divider)' : 'none' }}>{r[1]}</td>
              <td style={{ padding: '7px 14px', color: 'var(--text-body)', borderBottom: i < rows.length - 1 ? '1px solid var(--divider)' : 'none' }}>{r[2]}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function DoDont({ dos = [], donts = [] }) {
  const col = (title, items, ok) => (
    <div style={{ flex: 1, minWidth: 260, border: '1px solid var(--border)', borderTop: '2px solid ' + (ok ? 'var(--secure)' : 'var(--danger)'), borderRadius: 'var(--radius-md)', padding: '14px 16px', background: 'var(--surface-card)' }}>
      <div style={{ fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.14em', color: ok ? 'var(--secure)' : 'var(--danger)', marginBottom: 10 }}>{title}</div>
      {items.map((it, i) => (
        <div key={i} style={{ display: 'flex', gap: 9, padding: '5px 0', fontSize: 'var(--fs-sm)', lineHeight: 1.55, color: 'var(--text-body)' }}>
          <Icon name={ok ? 'check' : 'x'} size={14} color={ok ? 'var(--secure)' : 'var(--danger)'} style={{ marginTop: 3 }} />
          <span>{it}</span>
        </div>
      ))}
    </div>
  );
  return <div style={{ display: 'flex', gap: 14, flexWrap: 'wrap', marginBottom: 14 }}>{col('DO', dos, true)}{col("DON'T", donts, false)}</div>;
}

window.Docs = { DocsShell, PageIntro, Section, SubHead, Demo, Code, PropsTable, DoDont, ModeToggle, ControlCluster, PlatformSwitcher, PLATFORMS, getPlatform, NS };
