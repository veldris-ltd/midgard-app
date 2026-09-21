/* Midgard marketing-site shell: shared chrome for midgard.eu and the four
   product subdomains. Load AFTER ds-loader.js with
   <script type="text/babel" src="site-shell.jsx">. Exposes window.Site. */
const NS = window.__mgNS();
const { Icon, ProductMark, Button, Badge, StatusPill, SectionLabel, Terminal, Input, Select, StatBlock, Tag } = NS;

const PRODUCTS = [
  { id: 'midgard', domain: 'midgard.eu', href: 'index.html', name: 'MIDGARD', desc: 'The sovereign enclosure' },
  { id: 'forge', domain: 'forge.midgard.eu', href: 'forge.html', name: 'FORGE', desc: 'Data operations' },
  { id: 'mimir', domain: 'mimir.midgard.eu', href: 'mimir.html', name: 'MIMIR', desc: 'Local agentic AI' },
  { id: 'bifrost', domain: 'bifrost.midgard.eu', href: 'bifrost.html', name: 'BIFROST', desc: 'Continuous delivery' },
  { id: 'heimdall', domain: 'heimdall.midgard.eu', href: 'heimdall.html', name: 'HEIMDALL', desc: 'Operational fusion' },
];

/* Light / dark / system segmented toggle; state lives in docs/mode.js. */
function ModeToggle() {
  const [mode, setMode] = React.useState(window.__mgMode ? window.__mgMode.get() : 'dark');
  React.useEffect(() => {
    const f = () => setMode(window.__mgMode.get());
    window.addEventListener('mg-mode', f);
    return () => window.removeEventListener('mg-mode', f);
  }, []);
  const opts = [
    { id: 'light', icon: 'sun', label: 'Light mode' },
    { id: 'dark', icon: 'moon', label: 'Dark mode' },
    { id: 'system', icon: 'monitor', label: 'Follow system' },
  ];
  return (
    <div role="group" aria-label="Colour mode" style={{ display: 'flex', gap: 2, padding: 2, borderRadius: 999, background: 'var(--surface-card)', boxShadow: 'inset 0 0 0 1px var(--border)', flex: 'none' }}>
      {opts.map(o => (
        <button key={o.id} type="button" title={o.label} aria-label={o.label} aria-pressed={mode === o.id}
          onClick={() => window.__mgMode && window.__mgMode.set(o.id)}
          style={{
            all: 'unset', cursor: 'pointer', width: 26, height: 26, borderRadius: 999,
            display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            color: mode === o.id ? 'var(--text-heading)' : 'var(--text-faint)',
            background: mode === o.id ? 'var(--surface-raised)' : 'transparent',
            boxShadow: mode === o.id ? 'inset 0 0 0 1px var(--border-strong, var(--border))' : 'none',
          }}>
          <Icon name={o.icon} size={13} />
        </button>
      ))}
    </div>
  );
}

function SiteHeader({ active }) {
  const [open, setOpen] = React.useState(false);
  const current = PRODUCTS.find(p => p.id === active) || PRODUCTS[0];
  return (
    <header style={{
      position: 'sticky', top: 0, zIndex: 100,
      display: 'flex', alignItems: 'center', gap: 'clamp(10px, 2vw, 22px)',
      padding: '0 clamp(14px, 3.5vw, 36px)', height: 62,
      background: 'color-mix(in srgb, var(--bg-sunken) 82%, transparent)',
      backdropFilter: 'var(--blur-panel)', WebkitBackdropFilter: 'var(--blur-panel)',
      borderBottom: '1px solid var(--border)',
    }}>
      <a href="index.html" style={{ textDecoration: 'none', flex: 'none' }}>
        <ProductMark product={active === 'midgard' ? 'midgard' : active} size="sm" showDescriptor={false} />
      </a>
      <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.1em', color: 'var(--text-faint)', whiteSpace: 'nowrap' }} className="site-domain">{current.domain}</span>
      <nav style={{ display: 'flex', gap: 2, marginLeft: 'auto' }} className="site-nav">
        {PRODUCTS.slice(1).map(p => (
          <a key={p.id} href={p.href} style={{
            padding: '7px 11px', borderRadius: 'var(--radius-sm)', textDecoration: 'none',
            fontSize: 'var(--fs-sm)', fontWeight: active === p.id ? 600 : 400,
            color: active === p.id ? 'var(--text-heading)' : 'var(--text-muted)',
            background: active === p.id ? 'var(--accent-tint)' : 'transparent',
            whiteSpace: 'nowrap',
          }}>{p.name[0] + p.name.slice(1).toLowerCase()}</a>
        ))}
      </nav>
      <ModeToggle />
      <a href="#early-access" style={{ textDecoration: 'none', flex: 'none' }}>
        <Button variant="primary" size="sm" iconRight={<Icon name="arrow-right" size={14} />}>Early access</Button>
      </a>
      <button type="button" aria-label="Menu" onClick={() => setOpen(o => !o)} className="site-burger" style={{
        all: 'unset', display: 'none', width: 34, height: 30, borderRadius: 5, cursor: 'pointer',
        background: 'var(--surface-card)', boxShadow: 'inset 0 0 0 1px var(--border)',
        color: 'var(--text-body)', alignItems: 'center', justifyContent: 'center',
      }}><Icon name="menu" size={15} /></button>
      {open && ReactDOM.createPortal(
        <>
          <div onClick={() => setOpen(false)} style={{ position: 'fixed', inset: 0, zIndex: 190, background: 'rgba(0,0,0,0.55)' }} />
          <nav style={{ position: 'fixed', top: 0, right: 0, bottom: 0, zIndex: 200, width: 'min(80vw, 300px)', background: 'var(--bg-sunken)', borderLeft: '1px solid var(--border)', padding: '16px 14px', display: 'flex', flexDirection: 'column', gap: 4, overflowY: 'auto' }}>
            <div style={{ display: 'flex', alignItems: 'center', padding: '0 8px 12px' }}>
              <span className="mg-label" style={{ color: 'var(--text-faint)' }}>MIDGARD SUITE</span>
              <button type="button" aria-label="Close" onClick={() => setOpen(false)} style={{ all: 'unset', cursor: 'pointer', marginLeft: 'auto', color: 'var(--text-muted)', display: 'inline-flex', width: 30, height: 30, alignItems: 'center', justifyContent: 'center' }}><Icon name="x" size={16} /></button>
            </div>
            {PRODUCTS.map(p => (
              <a key={p.id} href={p.href} style={{
                display: 'flex', flexDirection: 'column', gap: 2, padding: '10px 12px', minHeight: 44,
                borderRadius: 'var(--radius-sm)', textDecoration: 'none', justifyContent: 'center',
                background: active === p.id ? 'var(--accent-tint)' : 'transparent',
                boxShadow: active === p.id ? 'inset 2px 0 0 var(--accent)' : 'none',
              }}>
                <span style={{ fontFamily: 'var(--font-display)', fontWeight: 700, fontSize: 14, letterSpacing: '0.06em', color: 'var(--text-heading)' }}>{p.name}</span>
                <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: 'var(--text-faint)' }}>{p.domain.toUpperCase()}</span>
              </a>
            ))}
          </nav>
        </>, document.body)}
    </header>
  );
}

/* Early-access signup: persists to localStorage, issues a mono receipt. */
function EarlyAccess({ product }) {
  const KEY = 'mg-early-access';
  const [form, setForm] = React.useState({ name: '', email: '', org: '', interest: product || 'suite' });
  const [done, setDone] = React.useState(null);
  const set = (k) => (e) => setForm(f => ({ ...f, [k]: e.target ? e.target.value : e }));
  const submit = () => {
    if (!form.email.trim() || !/.+@.+\..+/.test(form.email)) { setDone({ error: 'A valid email is required.' }); return; }
    const list = JSON.parse(localStorage.getItem(KEY) || '[]');
    const id = 'EA-' + String(1000 + list.length + 1);
    const rec = { ...form, id, ts: new Date().toISOString() };
    list.push(rec);
    localStorage.setItem(KEY, JSON.stringify(list));
    setDone(rec);
  };
  return (
    <section id="early-access" className="mg-grid-bg" style={{ background: 'color-mix(in srgb, var(--bg-sunken) 55%, transparent)', borderTop: '1px solid var(--border)', padding: 'clamp(48px, 8vw, 96px) clamp(16px, 4vw, 36px)' }}>
      <div style={{ maxWidth: 880, margin: '0 auto' }}>
        <SectionLabel number={9}>EARLY ACCESS</SectionLabel>
        <h2 style={{ fontSize: 'clamp(1.7rem, 4vw, 2.6rem)', margin: '14px 0 10px' }}>Get inside the enclosure first.</h2>
        <p style={{ margin: '0 0 28px', fontSize: 'var(--fs-body)', color: 'var(--text-muted)', maxWidth: 560, lineHeight: 1.65 }}>
          The suite is in closed development. Early-access partners get the first self-host builds, the reference hardware guide, and a direct line to the architecture team.
        </p>
        {done && done.id ? (
          <div style={{ background: 'var(--surface-card)', border: '1px solid var(--secure)', borderRadius: 'var(--radius-lg)', padding: 'clamp(18px, 3vw, 28px)', maxWidth: 560 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 14 }}>
              <Icon name="shield-check" size={18} color="var(--secure)" />
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, letterSpacing: '0.14em', color: 'var(--secure)' }}>REQUEST SEALED · {done.id}</span>
            </div>
            <Terminal lines={[
              '> midgard access request --verify',
              'applicant  ' + (done.name || 'unnamed'),
              'contact    ' + done.email,
              'interest   ' + done.interest,
              'stored     locally · nothing left this page',
              '# we will reach out when your cohort opens',
            ]} />
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 14, maxWidth: 560 }}>
            <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
              <Input label="Name" placeholder="Ada Lovelace" value={form.name} onChange={set('name')} style={{ flex: '1 1 200px' }} fullWidth />
              <Input label="Work email" placeholder="you@organisation.eu" value={form.email} onChange={set('email')} style={{ flex: '1 1 220px' }} fullWidth />
            </div>
            <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', alignItems: 'flex-end' }}>
              <Input label="Organisation" placeholder="Optional" value={form.org} onChange={set('org')} style={{ flex: '1 1 200px' }} fullWidth />
              <Select label="Primary interest" value={form.interest} onChange={set('interest')} style={{ flex: '1 1 180px' }} fullWidth
                options={[{ value: 'suite', label: 'Full suite' }, { value: 'midgard', label: 'Midgard core' }, { value: 'forge', label: 'Forge' }, { value: 'mimir', label: 'Mimir' }, { value: 'bifrost', label: 'Bifrost' }, { value: 'heimdall', label: 'Heimdall' }]} />
            </div>
            {done && done.error && <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--midgard-300)' }}>{done.error}</span>}
            <div style={{ display: 'flex', alignItems: 'center', gap: 16, flexWrap: 'wrap' }}>
              <Button variant="primary" size="lg" onClick={submit} iconRight={<Icon name="arrow-right" size={16} />}>Request early access</Button>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9.5, letterSpacing: '0.1em', color: 'var(--text-faint)' }}>STORED LOCALLY · NO TRACKING · NO EGRESS</span>
            </div>
          </div>
        )}
      </div>
    </section>
  );
}

/* GDPR storage notice: the site sets no cookies and loads no analytics, so this
   is an information banner, not a consent gate. Dismissal persists locally. */
function ConsentBanner() {
  const [seen, setSeen] = React.useState(() => {
    try { return !!localStorage.getItem('mg-consent'); } catch (e) { return true; }
  });
  if (seen) return null;
  const ack = () => { try { localStorage.setItem('mg-consent', new Date().toISOString()); } catch (e) {} setSeen(true); };
  return (
    <div role="region" aria-label="Storage notice" style={{
      position: 'fixed', left: 16, right: 16, bottom: 16, zIndex: 300,
      maxWidth: 620, margin: '0 auto', padding: '16px 18px',
      background: 'var(--surface-card)', border: '1px solid var(--border)',
      borderRadius: 'var(--radius-lg)', boxShadow: 'var(--shadow-lg)',
      display: 'flex', flexDirection: 'column', gap: 10,
    }}>
      <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.14em', color: 'var(--secure)' }}>NO COOKIES · NO TRACKING · NO EGRESS</span>
      <p style={{ margin: 0, fontSize: 'var(--fs-sm)', color: 'var(--text-body)', lineHeight: 1.65 }}>
        This site sets no cookies and loads no analytics. Your browser's local storage is used for functional state only: your colour-mode choice and anything you submit in forms here, which never leaves your device.
      </p>
      <div style={{ display: 'flex', alignItems: 'center', gap: 16, flexWrap: 'wrap' }}>
        <Button variant="primary" size="sm" onClick={ack}>Understood</Button>
        <a href="privacy.html" style={{ fontFamily: 'var(--font-mono)', fontSize: 10.5, letterSpacing: '0.08em', color: 'var(--accent-hover)', textDecoration: 'none' }}>PRIVACY NOTICE →</a>
      </div>
    </div>
  );
}

function SiteFooter() {
  return (
    <footer style={{ borderTop: '1px solid var(--border)', background: 'var(--bg-sunken)', padding: 'clamp(28px, 5vw, 48px) clamp(16px, 4vw, 36px)' }}>
      <div style={{ maxWidth: 1200, margin: '0 auto', display: 'flex', gap: '28px 48px', flexWrap: 'wrap' }}>
        <div style={{ flex: '1 1 240px' }}>
          <ProductMark product="midgard" size="sm" />
          <p style={{ margin: '14px 0 0', fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', lineHeight: 1.7, maxWidth: 300 }}>
            A sovereign, open-source, post-quantum platform suite by Veldris Ltd. Nothing leaves the enclosure.
          </p>
        </div>
        {[
          ['PRODUCTS', PRODUCTS.slice(1).map(p => [p.name[0] + p.name.slice(1).toLowerCase(), p.href])],
          ['PLATFORM', [['Midgard core', 'index.html'], ['Architecture', 'architecture.html'], ['Pricing & licence', 'pricing.html'], ['Source on GitHub', 'github.html'], ['Early access', '#early-access']]],
          ['COMPANY', [['Veldris Ltd · veldris.io', 'https://veldris.io'], ['About us', 'about.html'], ['Contact us', 'contact.html'], ['Security disclosure', 'contact.html#security']]],
          ['LEGAL', [['Privacy notice', 'privacy.html'], ['Terms of use', 'terms.html'], ['Legal notice', 'legal.html'], ['Your GDPR rights', 'privacy.html#rights']]],
        ].map(([title, links]) => (
          <div key={title} style={{ flex: '0 1 160px' }}>
            <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 12 }}>{title}</div>
            {links.map(([label, href]) => (
              <a key={label} href={href} style={{ display: 'block', padding: '5px 0', fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', textDecoration: 'none' }}>{label}</a>
            ))}
          </div>
        ))}
        <div style={{ flex: '1 1 100%', display: 'flex', gap: 14, flexWrap: 'wrap', alignItems: 'center', borderTop: '1px solid var(--divider)', paddingTop: 18 }}>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--text-faint)' }}>© 2026 VELDRIS LTD · OPEN-SOURCE CORE · OSI LICENCE</span>
          <span style={{ marginLeft: 'auto', fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--text-faint)' }}>PQ-SEALED · ML-KEM-1024 · AIR-GAP READY</span>
        </div>
      </div>
    </footer>
  );
}

function SiteShell({ active, children }) {
  return (
    <div style={{ minHeight: '100vh', display: 'flex', flexDirection: 'column', position: 'relative', zIndex: 1 }}>
      <style>{'@media (max-width: 860px){ .site-nav{display:none!important} .site-burger{display:inline-flex!important} .site-domain{display:none} }'}</style>
      <SiteHeader active={active} />
      <main style={{ flex: 1 }}>{children}</main>
      <EarlyAccess product={active === 'midgard' ? 'suite' : active} />
      <SiteFooter />
      <ConsentBanner />
    </div>
  );
}

window.Site = { SiteShell, PRODUCTS, NS };
