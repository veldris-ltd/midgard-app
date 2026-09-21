// Record the Midgard launch walkthrough: real edge for the Midgard console, mock
// gateways for the other consoles, title/terminal/end cards from ./pages.
const { chromium } = require('playwright-core');
const fs = require('fs'), path = require('path');

const ORIGIN = 'http://localhost:5173';
const VENDOR = path.resolve(__dirname, '..', '..', 'demo', 'vendor', 'unpkg');
const MODE = process.env.MG_MODE === 'light' ? 'light' : 'dark';
const LIGHT = MODE === 'light';
const OUT = path.join(__dirname, LIGHT ? 'out-light' : 'out');
const SHOTS = path.join(OUT, 'screens');
fs.mkdirSync(SHOTS, { recursive: true });

const ACC = LIGHT
  ? { midgard: '#a8171e', forge: '#b25715', mimir: '#17828e', bifrost: '#6244c2', heimdall: '#a17a17', none: '#5c636b' }
  : { midgard: '#f26b70', forge: '#f39b57', mimir: '#63d3dd', bifrost: '#ab93f5', heimdall: '#ecc55c', none: '#9199a3' };
const OV = LIGHT
  ? { bg: 'rgba(255,255,255,0.94)', border: '#d6d9de', text: '#101114', body: '#383d44', shadow: 'rgba(0,0,0,0.18)' }
  : { bg: 'rgba(10,11,13,0.90)', border: '#2a2d34', text: '#e9ebee', body: '#b6bcc4', shadow: 'rgba(0,0,0,0.55)' };
const consoleAt = (p, r = '/') => `${ORIGIN}/products/${p}/ui/dist/index.html#${r}`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let shotN = 0;

// Overlay installed on every document: a caption bar and a visible cursor.
const OVERLAY = `
(() => {
  try { localStorage.setItem('mg-mode', '${MODE}'); localStorage.setItem('mg-consent', 'capture'); } catch (e) {}
  const css = \`
    #mg-cap { position: fixed; left: 40px; bottom: 34px; max-width: 980px; z-index: 2147483000; pointer-events: none;
      background: ${OV.bg}; border: 1px solid ${OV.border}; border-radius: 8px; padding: 14px 20px 15px;
      font-family: 'Space Grotesk', system-ui, sans-serif; color: ${OV.text}; box-shadow: 0 16px 48px ${OV.shadow};
      opacity: 0; transition: opacity 320ms ease; }
    #mg-cap.on { opacity: 1; }
    #mg-cap .e { font-family: 'JetBrains Mono', ui-monospace, monospace; font-size: 11px; letter-spacing: 0.18em; text-transform: uppercase; margin-bottom: 5px; }
    #mg-cap .h { font-size: 21px; font-weight: 600; line-height: 1.25; }
    #mg-cap .b { font-size: 14px; color: ${OV.body}; margin-top: 4px; line-height: 1.4; }
    #mg-cur { position: fixed; left: 0; top: 0; width: 22px; height: 22px; margin: -11px 0 0 -11px; border-radius: 50%;
      border: 2px solid #fff; background: rgba(202,29,37,0.55); box-shadow: 0 0 0 3px rgba(0,0,0,0.35); z-index: 2147483001;
      pointer-events: none; transition: transform 120ms ease; display: none; }
    #mg-cur.down { transform: scale(0.7); background: rgba(202,29,37,0.95); }
  \`;
  const install = () => {
    if (document.getElementById('mg-cap')) return;
    const s = document.createElement('style'); s.textContent = css; document.head.appendChild(s);
    const c = document.createElement('div'); c.id = 'mg-cap'; c.innerHTML = '<div class="e"></div><div class="h"></div><div class="b"></div>'; document.body.appendChild(c);
    const k = document.createElement('div'); k.id = 'mg-cur'; document.body.appendChild(k);
    document.addEventListener('mousemove', (e) => { k.style.display = 'block'; k.style.left = e.clientX + 'px'; k.style.top = e.clientY + 'px'; }, true);
    document.addEventListener('mousedown', () => k.classList.add('down'), true);
    document.addEventListener('mouseup', () => k.classList.remove('down'), true);
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', install); else install();
  window.__mgCaption = (eyebrow, head, body, colour) => {
    install();
    const c = document.getElementById('mg-cap');
    if (!eyebrow && !head) { c.classList.remove('on'); return; }
    c.querySelector('.e').textContent = eyebrow; c.querySelector('.e').style.color = colour;
    c.querySelector('.h').textContent = head; c.querySelector('.b').textContent = body || '';
    c.querySelector('.b').style.display = body ? 'block' : 'none';
    c.classList.add('on');
  };
  window.__mgHide = (hide) => { for (const id of ['mg-cap', 'mg-cur']) { const el = document.getElementById(id); if (el) el.style.visibility = hide ? 'hidden' : ''; } };
})();`;

async function main() {
  const browser = await chromium.launch({ channel: 'msedge', headless: true, args: ['--host-resolver-rules=MAP localhost 127.0.0.1, MAP * ~NOTFOUND, EXCLUDE 127.0.0.1', '--hide-scrollbars'] });
  const ctx = await browser.newContext({
    viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1, colorScheme: MODE,
    recordVideo: { dir: path.join(OUT, 'raw'), size: { width: 1920, height: 1080 } },
  });
  await ctx.route('https://unpkg.com/**', (route) => {
    const file = path.join(VENDOR, ...new URL(route.request().url()).pathname.split('/').filter(Boolean));
    if (fs.existsSync(file)) route.fulfill({ path: file, contentType: 'text/javascript' }); else route.abort();
  });
  await ctx.addInitScript(OVERLAY);
  const page = await ctx.newPage();
  page.on('pageerror', (e) => console.log('  pageerror:', e.message));
  page.on('requestfailed', (r) => { if (!r.url().includes('favicon')) console.log('  requestfailed:', r.url()); });

  const cap = (eyebrow, head, body, acc = 'midgard') => page.evaluate(([e, h, b, c]) => window.__mgCaption(e, h, b, c), [eyebrow, head, body || '', ACC[acc]]);
  const clearCap = () => page.evaluate(() => window.__mgCaption('', ''));
  const shot = async (name) => {
    await page.evaluate(() => window.__mgHide(true));
    await sleep(120);
    await page.screenshot({ path: path.join(SHOTS, `${String(++shotN).padStart(2, '0')}-${name}.png`) });
    await page.evaluate(() => window.__mgHide(false));
  };
  const glide = async (x, y, steps = 28) => { await page.mouse.move(x, y, { steps }); };
  const clickAt = async (loc) => {
    const b = await loc.boundingBox();
    if (!b) throw new Error('no box for locator');
    await glide(b.x + b.width / 2, b.y + b.height / 2);
    await sleep(250);
    await loc.click();
  };
  const settle = async (ms = 1400) => { await page.waitForLoadState('networkidle').catch(() => {}); await sleep(ms); };
  const navTo = async (label) => {
    const btn = page.getByRole('button', { name: label, exact: false }).first();
    await clickAt(btn);
    await settle(1200);
  };
  const scrollBy = async (dy, steps = 24) => { for (let i = 0; i < steps; i++) { await page.mouse.wheel(0, dy / steps); await sleep(40); } };
  const t0 = Date.now();
  const mark = (s) => console.log(`${String(((Date.now() - t0) / 1000).toFixed(1)).padStart(6)}s  ${s}`);

  // ── 1. title card ───────────────────────────────────────────────────────
  await page.goto(`${ORIGIN}/launch/title.html`); await page.evaluate(() => document.fonts.ready);
  await sleep(5500); mark('title');

  // ── 2. site home ────────────────────────────────────────────────────────
  await page.goto(`${ORIGIN}/docs/design/site/index.html`); await settle(1800);
  await cap('MIDGARD · VELDRIS LTD', 'One enclosure for your data, your models and your operations.', 'Four products, one trust boundary, zero outbound dependencies.');
  await glide(1200, 620); await sleep(2400); await shot('site-home');
  await scrollBy(1500, 40); await sleep(2200);
  await scrollBy(1100, 30); await sleep(2000); mark('site');

  // ── 3. terminal: the real edge ───────────────────────────────────────────
  await page.goto(`${ORIGIN}/launch/terminal.html`); await page.evaluate(() => document.fonts.ready);
  await cap('THE ENCLOSURE EDGE · REAL RUST SERVICE', 'Start the edge. Log in. Query the fabric. Read the ledger.', 'Every response is authorised by the policy decision point and written to a PQ-signed audit ledger first.');
  await page.waitForFunction(() => window.__done === true, null, { timeout: 90000 });
  await sleep(3500); await shot('edge-terminal'); mark('terminal');

  // ── 4. Midgard console: login against the edge ──────────────────────────
  await page.goto(`${ORIGIN}/launch/midgard/index.html#/`); await settle(1200);
  await cap('MIDGARD CONSOLE · SIGN-IN', 'Credentials become an ML-DSA-signed session token.', 'A wrong password, a missing token and a forged token all get the same uniform 401.');
  await shot('console-login');
  const user = page.getByLabel('Username'); const pass = page.getByLabel('Password');
  await clickAt(user); await page.keyboard.type('operator', { delay: 70 });
  await clickAt(pass); await page.keyboard.type('midgard-demo', { delay: 70 });
  await sleep(500);
  await clickAt(page.getByRole('button', { name: /sign in|log in|authenticate/i }).first());
  await settle(2200); mark('login');

  // ── 5. Object fabric ────────────────────────────────────────────────────
  await cap('MIDGARD CONSOLE · OBJECT FABRIC', 'Your organisation, described once.', 'Aircraft, facilities, crew, sensor feeds: real things with properties, links and the actions a person may take on them.');
  await sleep(2200); await shot('midgard-fabric');
  const rows = page.locator('table tbody tr');
  await clickAt(rows.nth(2)); await sleep(1600);
  await clickAt(rows.nth(0)); await sleep(1600);
  await cap('MIDGARD CONSOLE · HUMAN GATE', 'Consequential actions stop for a person.', '"Ground aircraft" is human-gated: the edge refuses it until an operator authorises, and the authorisation itself is audited.');
  const ground = page.getByRole('button', { name: /Ground aircraft/ }).first();
  await clickAt(ground); await sleep(1800);
  await shot('midgard-humangate-pending');
  await clickAt(page.getByRole('button', { name: 'Authorise' }).first()); await settle(1600);
  await shot('midgard-humangate-authorised'); await sleep(2600); mark('fabric');

  // ── 6. Policies ─────────────────────────────────────────────────────────
  await navTo('Policies & markings');
  await cap('MIDGARD CONSOLE · POLICIES & MARKINGS', 'Access is proven, not assumed.', 'Deny by default. Dry-run a rule for this subject, clearance and purpose before it goes live; the decision is signed.');
  await sleep(1800); await shot('midgard-policies');
  await scrollBy(700, 20); await sleep(800);
  await clickAt(page.getByRole('button', { name: 'Evaluate' }).first()); await settle(1200);
  await shot('midgard-policy-dryrun'); await sleep(2800); mark('policies');

  // ── 7. Audit lineage ────────────────────────────────────────────────────
  await navTo('Audit lineage');
  await cap('MIDGARD CONSOLE · AUDIT LINEAGE', 'Audit is a query, not an archaeology project.', 'Every read, write and gate decision from this session is already here, each entry re-verified: ML-DSA-65.');
  await sleep(2000); await shot('midgard-audit');
  await clickAt(page.locator('table tbody tr').nth(1)); await sleep(2400);
  await scrollBy(500, 16); await sleep(1600); mark('audit');

  // ── 8. Search-around ────────────────────────────────────────────────────
  await navTo('Search-around');
  await cap('MIDGARD CONSOLE · SEARCH-AROUND', 'Follow the links, not the tables.', 'From one aircraft, two hops out: its route, its base, its crew and its open maintenance event.');
  await sleep(2200); await shot('midgard-search'); await glide(1100, 560); await sleep(2200); mark('search');

  // ── 9. Keys ─────────────────────────────────────────────────────────────
  await navTo('Keys & crypto');
  await cap('MIDGARD CONSOLE · KEYS & CRYPTO', 'Post-quantum by default.', 'Hybrid X25519 + ML-KEM-768 in transit, AES-256-GCM under ML-KEM-1024-wrapped keys at rest, ML-DSA signatures everywhere.');
  await sleep(2400); await shot('midgard-kms'); await sleep(1600); mark('kms');

  // ── 10. Forge ───────────────────────────────────────────────────────────
  await page.goto(consoleAt('forge')); await settle(1600);
  await cap('FORGE · PIPELINE BUILDER', 'Getting data in, without losing track of it.', 'Draw the route from raw source to finished dataset, or write it in code. Every build is versioned and carries its lineage.', 'forge');
  await sleep(2400); await shot('forge-pipeline'); await glide(900, 500); await sleep(1400);
  await navTo('Data health');
  await cap('FORGE · EXPECTATIONS & DRIFT', 'Bad data stops before it reaches anyone.', 'Expectations run on every build. Failing ones block promotion, so nobody downstream ever sees them.', 'forge');
  await sleep(2400); await shot('forge-health'); await sleep(1400); mark('forge');

  // ── 11. Mimir ───────────────────────────────────────────────────────────
  await page.goto(consoleAt('mimir')); await settle(1600);
  await cap('MIMIR · AGENT STUDIO', 'AI that never phones home.', 'Answers grounded in your own data with every claim cited. Mimir drafts the fix, then stops, because acting changes something real.', 'mimir');
  await sleep(2600); await shot('mimir-studio'); await glide(1000, 600); await sleep(1400);
  await navTo('Guardrails');
  await cap('MIMIR · GUARDRAIL CONSOLE', 'Five rails screen every input and every tool call.', 'A prompt-injection attempt is quarantined before a single tool runs. Nothing has ever left the enclosure.', 'mimir');
  await sleep(2400); await shot('mimir-guardrails'); await sleep(1400); mark('mimir');

  // ── 12. Heimdall ────────────────────────────────────────────────────────
  await page.goto(consoleAt('heimdall')); await settle(1600);
  await cap('HEIMDALL · COMMON OPERATING PICTURE', 'Many feeds, one picture everyone shares.', 'Radar, transponder, maritime and ground sources fused into a single live view, with anomalies raised for a human.', 'heimdall');
  await sleep(2600); await shot('heimdall-picture'); await glide(1100, 520); await sleep(1400);
  await navTo('Workflows');
  await cap('HEIMDALL · ACTION WORKFLOWS', 'AI ranks the options. People authorise the action.', 'The authorisation, the evidence behind it and what happened next become one signed record.', 'heimdall');
  await sleep(2400); await shot('heimdall-workflows'); await sleep(1400); mark('heimdall');

  // ── 13. Bifrost ─────────────────────────────────────────────────────────
  await page.goto(consoleAt('bifrost')); await settle(1600);
  await cap('BIFROST · RELEASE CONSOLE', 'Nothing deploys unless it proves itself.', 'Rebuilt bit-for-bit, SBOM attached, signature verified, then rolled out ring by ring with health gates between.', 'bifrost');
  await sleep(2600); await shot('bifrost-release'); await glide(1000, 560); await sleep(1400);
  await navTo('Bundles');
  await cap('BIFROST · OFFLINE BUNDLES', 'Even where the internet cannot reach.', 'Air-gapped sites get the identical release on signed media; it verifies itself offline and returns a signed receipt.', 'bifrost');
  await sleep(2400); await shot('bifrost-bundles'); await sleep(1400); mark('bifrost');

  // ── 14. Admin ───────────────────────────────────────────────────────────
  await page.goto(consoleAt('admin', '/approvals')); await settle(1600);
  await cap('ADMIN CONTROL PANEL · DUAL CONTROL', 'No single person can do the dangerous things.', 'High-risk changes need a second, different approver. There is no super-admin, and a self-approval attempt is refused and recorded.');
  await sleep(2600); await shot('admin-approvals'); await glide(1000, 560); await sleep(1400);
  await navTo('Ledger');
  await cap('ADMIN CONTROL PANEL · AUDIT LEDGER', 'Seven subsystems, one tamper-evident ledger.', 'Approvals, denials, break-glass sessions and rehearsals all land in the same signed record.');
  await sleep(2400); await shot('admin-ledger'); await sleep(1400); mark('admin');

  // ── 15. Clients ─────────────────────────────────────────────────────────
  await page.goto(consoleAt('clients', '/mobile')); await settle(1800);
  await cap('CLIENT PLATFORMS · MOBILE, DESKTOP, WEB', 'It works where the work happens.', 'One shared core behind phone, desktop and browser. Keys stay in device hardware; the app keeps working with no signal.', 'none');
  await sleep(2800); await shot('clients-mobile'); await glide(960, 540); await sleep(1600); mark('clients');

  // ── 16. site architecture + end card ────────────────────────────────────
  await page.goto(`${ORIGIN}/docs/design/site/architecture.html`); await settle(1800);
  await cap('MIDGARD · ARCHITECTURE', 'One boundary. Everything inside it.', 'From a laptop to a fleet of air-gapped sites: the same releases, the same signatures, every topology.');
  await sleep(2400); await shot('site-arch'); await scrollBy(900, 30); await sleep(2400);
  await page.goto(`${ORIGIN}/launch/end.html`); await page.evaluate(() => document.fonts.ready);
  await sleep(6000); mark('end');

  const video = page.video();
  await ctx.close();
  const raw = await video.path();
  fs.copyFileSync(raw, path.join(OUT, 'walkthrough.webm'));
  await browser.close();
  console.log('done', raw);
}
main().catch((e) => { console.error(e); process.exit(1); });
