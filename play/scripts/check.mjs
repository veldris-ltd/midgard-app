#!/usr/bin/env node
/* Verify a built playground. [ADR-PLAY-001] [ADR-PLAY-002] [ADR-PLAY-003]
 *
 *   node play/scripts/check.mjs            static checks, then browser checks if
 *                                          Playwright is installed
 *   node play/scripts/check.mjs --static   static checks only (CI, deploy)
 *   node play/scripts/check.mjs --origin https://play.veldris.dev
 *                                          browser checks against a deployed site
 *
 * Static: the shell and all seven consoles are in dist; every console page
 * carries the pinned CSP; no page loads a script or stylesheet from another
 * origin; every tour step names a real console route.
 *
 * Browser (needs `playwright` resolvable): opens every tour step through the
 * shell and fails if a step's highlight is not found, if any page logs an
 * error (CSP violations included), or if anything is requested from outside
 * the playground's own origin. */

import { readFileSync, existsSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { transformWithEsbuild } from 'vite';

const HERE = dirname(fileURLToPath(import.meta.url));
const PLAY = resolve(HERE, '..');
const ROOT = resolve(PLAY, '..');
const DIST = join(PLAY, 'dist');
const CONSOLES = ['midgard', 'forge', 'mimir', 'heimdall', 'bifrost', 'admin', 'clients'];
// tour.ts is TypeScript; compile it with the esbuild that Vite already carries, so
// this runs on any Node that runs Vite, not only on Node with type stripping.
const tourSrc = join(PLAY, 'src', 'tour.ts');
const { code } = await transformWithEsbuild(readFileSync(tourSrc, 'utf8'), tourSrc, { loader: 'ts', format: 'esm' });
const { TOUR } = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

const failures = [];
/* A script, stylesheet, icon or preload fetched from another origin. Plain links
   (canonical, a hyperlink) load nothing and are allowed. */
const external = (html) =>
  [...html.matchAll(/<(script|link)\b[^>]*>/gi)].some(([tag, name]) => {
    if (name.toLowerCase() === 'link' && !/\brel="(stylesheet|preload|modulepreload|icon)"/i.test(tag)) return false;
    return /\b(src|href)="(https?:)?\/\//i.test(tag);
  });
const fail = (msg) => failures.push(msg);

// ── static ────────────────────────────────────────────────────────────────────
const routesOf = (id) => {
  const src = readFileSync(join(ROOT, 'products', id, 'ui', 'src', 'app', 'routes.tsx'), 'utf8');
  return new Set([...src.matchAll(/path:\s*['"]([^'"]+)['"]/g)].map((m) => m[1]));
};

if (!existsSync(join(DIST, 'index.html'))) fail('dist/index.html is missing: run node play/build.mjs');
for (const id of CONSOLES) {
  const page = join(DIST, 'consoles', id, 'index.html');
  if (!existsSync(page)) {
    fail(`consoles/${id}/index.html is missing`);
    continue;
  }
  const html = readFileSync(page, 'utf8');
  if (!/http-equiv="Content-Security-Policy"[^>]*script-src 'self' 'sha256-/.test(html)) fail(`consoles/${id}: no pinned CSP`);
  if (external(html)) fail(`consoles/${id}: loads from another origin`);
}
if (existsSync(join(DIST, 'index.html'))) {
  const shell = readFileSync(join(DIST, 'index.html'), 'utf8');
  if (!/Content-Security-Policy[^>]*default-src 'self'/.test(shell)) fail('shell: no CSP');
  if (external(shell)) fail('shell: loads from another origin');
}
const routes = Object.fromEntries(CONSOLES.map((id) => [id, routesOf(id)]));
TOUR.forEach((s, i) => {
  if (!CONSOLES.includes(s.console)) fail(`tour step ${i + 1}: unknown console ${s.console}`);
  else if (!routes[s.console].has(s.route)) fail(`tour step ${i + 1}: ${s.console} has no route ${s.route}`);
  if (!s.title || !s.body?.length) fail(`tour step ${i + 1}: needs a title and a body`);
});
console.log(`play check: static · ${CONSOLES.length} consoles · ${TOUR.length} tour steps`);

// ── browser ───────────────────────────────────────────────────────────────────
async function browser() {
  let chromium;
  try {
    ({ chromium } = createRequire(import.meta.url)('playwright'));
  } catch {
    console.log('play check: browser checks skipped (playwright is not installed)');
    return;
  }
  // --origin https://play.veldris.dev checks a deployed site instead of dist.
  const at = process.argv.indexOf('--origin');
  const remote = at > 0 ? process.argv[at + 1].replace(/\/$/, '') : null;
  const port = 4199;
  const server = remote ? null : spawn(process.execPath, [join(HERE, 'serve.mjs'), String(port)], { stdio: 'ignore' });
  if (server) await new Promise((r) => setTimeout(r, 600));
  const origin = remote ?? `http://127.0.0.1:${port}`;
  const b = await chromium.launch(remote && process.env.HTTPS_PROXY ? { proxy: { server: process.env.HTTPS_PROXY } } : {});
  try {
    // Desktop, and a phone: at phone width the consoles fold their detail rail
    // behind "Expand inspector", and the tour must ring that instead, then the
    // target once the inspector is open.
    for (const [label, viewport] of [['desktop', { width: 1440, height: 900 }], ['phone', { width: 390, height: 844 }]]) {
      const ctx = await b.newContext({ viewport });
      const page = await ctx.newPage();
      const errors = [];
      page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
      page.on('pageerror', (e) => errors.push(String(e)));
      page.on('request', (r) => {
        if (!r.url().startsWith(origin) && !r.url().startsWith('data:') && !r.url().startsWith('blob:')) fail(`${label}: request outside the origin: ${r.url()}`);
      });
      page.on('response', (r) => r.status() >= 400 && fail(`${label}: ${r.status()} ${r.url()}`));
      await page.goto(`${origin}/`, { waitUntil: 'networkidle' });
      const ringShown = () =>
        page
          .waitForFunction(() => {
            const r = document.querySelector('.ring');
            return r && !r.hidden && r.getBoundingClientRect().width > 0;
          }, null, { timeout: 8000 })
          .then(() => true, () => false);
      for (let n = 1; n <= TOUR.length; n++) {
        const step = TOUR[n - 1];
        await page.evaluate((h) => (location.hash = h), `#/tour/${n}`);
        if (!step.target) {
          await page.waitForTimeout(400);
          continue;
        }
        if (!(await ringShown())) {
          fail(`${label}: tour step ${n} (${step.console} ${step.route}): highlight "${step.target}" not found`);
          continue;
        }
        // If the ring is on the inspector control, open it as a visitor would and
        // expect the ring to move to the step's own target.
        const viaInspector = await page.locator('.panel', { hasText: 'Expand inspector' }).count();
        if (viaInspector) {
          const frame = page.frames().find((f) => f.url().includes(`/consoles/${step.console}/`));
          await frame?.getByRole('button', { name: 'Expand inspector' }).first().click();
          const moved = await page
            .waitForFunction(() => !document.querySelector('.panel')?.textContent?.includes('Expand inspector'), null, { timeout: 5000 })
            .then(() => true, () => false);
          if (!moved || !(await ringShown())) fail(`${label}: tour step ${n}: "${step.target}" not ringed after opening the inspector`);
        }
      }
      for (const id of CONSOLES) {
        await page.evaluate((h) => (location.hash = h), `#/console/${id}`);
        await page.waitForTimeout(700);
      }
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      if (overflow > 0) fail(`${label}: the page scrolls sideways by ${overflow}px`);
      [...new Set(errors)].forEach((e) => fail(`${label}: page error: ${e}`));
      await ctx.close();
    }
    console.log(`play check: browser · ${TOUR.length} steps and ${CONSOLES.length} consoles, desktop and phone, at ${origin}`);
  } finally {
    await b.close();
    server?.kill();
  }
}
if (!process.argv.includes('--static') && failures.length === 0) await browser();

if (failures.length) {
  console.error(`\nplay check: ${failures.length} problem(s)\n  - ${failures.join('\n  - ')}`);
  process.exit(1);
}
console.log('play check: OK');
