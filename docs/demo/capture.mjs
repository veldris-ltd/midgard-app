#!/usr/bin/env node
// Capture the demo screens — every console route and site page the film can
// draw on — in dark and light mode, headlessly, with no network.
//
//   node docs/demo/capture.mjs --fetch-vendor    # once: React + Babel for the site pages
//   node docs/demo/capture.mjs                   # -> screenshots/{dark,light}/*.png
//   node docs/demo/capture.mjs --mode light --only midgard-fabric,site-home
//
// Needs Node >= 22 (built-in WebSocket) and a Chromium browser, Edge or Chrome:
// found on the usual install paths, or named with --browser / $CHROME_PATH.
// Build the consoles first; see README.md.
//
// Every hostname fails to resolve inside the browser. The page reaches only the
// static server this script runs on 127.0.0.1, and the site's unpkg scripts are
// answered from vendor/. Any failed request, HTTP error, page exception, blank
// page or wrong colour mode fails that screen, and its old capture is kept.

import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, '..', '..');
const VENDOR = path.join(HERE, 'vendor');

// The CSS size is what the consoles lay out against, so it is fixed for this
// cut; the scale factor only sets resolution (3840x2400).
const VIEW = { width: 1600, height: 1000, deviceScaleFactor: 2.4 };
const MODES = ['dark', 'light'];

const consoleAt = (product, route = '/') => `/products/${product}/ui/dist/index.html#${route}`;

/** Capture name -> path under the repo root. Each console screen is its route's default state. */
const SHOTS = {
  'midgard-fabric': consoleAt('midgard'),
  'midgard-policies': consoleAt('midgard', '/policies'),
  'midgard-audit': consoleAt('midgard', '/audit'),
  'midgard-kms': consoleAt('midgard', '/kms'),
  'midgard-search': consoleAt('midgard', '/search'),
  'forge-pipeline': consoleAt('forge'),
  'forge-datasets': consoleAt('forge', '/datasets'),
  'forge-health': consoleAt('forge', '/health'),
  'mimir-studio': consoleAt('mimir'),
  'mimir-evals': consoleAt('mimir', '/evals'),
  'mimir-guardrails': consoleAt('mimir', '/guardrails'),
  'mimir-models': consoleAt('mimir', '/models'),
  'heimdall-picture': consoleAt('heimdall'),
  'heimdall-entities': consoleAt('heimdall', '/entities'),
  'heimdall-feeds': consoleAt('heimdall', '/feeds'),
  'heimdall-workflows': consoleAt('heimdall', '/workflows'),
  'bifrost-release': consoleAt('bifrost'),
  'bifrost-fleet': consoleAt('bifrost', '/fleet'),
  'bifrost-bundles': consoleAt('bifrost', '/bundles'),
  'admin-overview': consoleAt('admin'),
  'admin-approvals': consoleAt('admin', '/approvals'),
  'admin-breakglass': consoleAt('admin', '/breakglass'),
  'admin-ledger': consoleAt('admin', '/ledger'),
  'clients-web': consoleAt('clients'),
  'clients-mobile': consoleAt('clients', '/mobile'),
  'clients-desktop': consoleAt('clients', '/desktop'),
  'site-home': '/docs/design/site/index.html',
  'site-arch': '/docs/design/site/architecture.html',
};

// The marketing site loads these three from unpkg.
const VENDOR_URLS = [
  'https://unpkg.com/react@18.3.1/umd/react.development.js',
  'https://unpkg.com/react-dom@18.3.1/umd/react-dom.development.js',
  'https://unpkg.com/@babel/standalone@7.29.0/babel.min.js',
];

const MIME = {
  '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.jsx': 'text/babel',
  '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png',
  '.jpg': 'image/jpeg', '.webp': 'image/webp', '.ico': 'image/x-icon', '.wasm': 'application/wasm',
  '.woff2': 'font/woff2', '.woff': 'font/woff', '.ttf': 'font/ttf', '.otf': 'font/otf',
};

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── vendor ───────────────────────────────────────────────────────────────────

const vendorPath = (url) => path.join(VENDOR, 'unpkg', ...new URL(url).pathname.split('/').filter(Boolean));

/** An exact-version copy already in node_modules (plain or pnpm store), if there is one. */
function fromNodeModules(url) {
  const m = new URL(url).pathname.match(/^\/((?:@[^/]+\/)?[^/@]+)@([^/]+)\/(.+)$/);
  if (!m) return null;
  const [, name, version, rest] = m;
  const dirs = [path.join(ROOT, 'node_modules', name)];
  const store = path.join(ROOT, 'node_modules', '.pnpm');
  const flat = `${name.replace('/', '+')}@${version}`;
  if (fs.existsSync(store)) {
    for (const e of fs.readdirSync(store)) {
      if (e === flat || e.startsWith(`${flat}_`)) dirs.push(path.join(store, e, 'node_modules', name));
    }
  }
  for (const dir of dirs) {
    try {
      if (JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8')).version !== version) continue;
    } catch {
      continue;
    }
    const file = path.join(dir, rest);
    if (fs.existsSync(file)) return file;
  }
  return null;
}

async function fetchVendor() {
  for (const url of VENDOR_URLS) {
    const dst = vendorPath(url);
    const rel = path.relative(HERE, dst);
    if (fs.existsSync(dst)) {
      console.log(`  have ${rel}`);
      continue;
    }
    const local = fromNodeModules(url);
    let buf;
    if (local) {
      buf = fs.readFileSync(local);
    } else {
      const res = await fetch(url);
      if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
      buf = Buffer.from(await res.arrayBuffer());
    }
    fs.mkdirSync(path.dirname(dst), { recursive: true });
    fs.writeFileSync(dst, buf);
    const sha = crypto.createHash('sha256').update(buf).digest('hex');
    console.log(`  ${rel}  ${buf.length} bytes  sha256:${sha}  (from ${local ? 'node_modules' : 'unpkg'})`);
  }
  console.log(`vendor -> ${VENDOR}`);
}

// ── static server ────────────────────────────────────────────────────────────

function serve() {
  const server = http.createServer((req, res) => {
    const file = path.resolve(ROOT, `.${decodeURIComponent(new URL(req.url, 'http://x').pathname)}`);
    if (!file.startsWith(ROOT + path.sep)) {
      res.writeHead(403);
      return res.end();
    }
    fs.readFile(file, (err, buf) => {
      if (err) {
        res.writeHead(404);
        return res.end();
      }
      res.writeHead(200, { 'content-type': MIME[path.extname(file).toLowerCase()] ?? 'application/octet-stream' });
      res.end(buf);
    });
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server)));
}

// ── browser ──────────────────────────────────────────────────────────────────

function findBrowser() {
  const under = (base, rel) => (base ? path.join(base, rel) : null);
  const known = {
    win32: [
      under(process.env['ProgramFiles(x86)'], 'Microsoft/Edge/Application/msedge.exe'),
      under(process.env.ProgramFiles, 'Microsoft/Edge/Application/msedge.exe'),
      under(process.env.ProgramFiles, 'Google/Chrome/Application/chrome.exe'),
      under(process.env.LOCALAPPDATA, 'Google/Chrome/Application/chrome.exe'),
    ],
    darwin: [
      '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
      '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
      '/Applications/Chromium.app/Contents/MacOS/Chromium',
    ],
  }[process.platform] ?? [];
  const names = process.platform === 'win32'
    ? ['msedge.exe', 'chrome.exe']
    : ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser', 'microsoft-edge'];
  const onPath = (process.env.PATH ?? '').split(path.delimiter).filter(Boolean)
    .flatMap((d) => names.map((n) => path.join(d, n)));
  return [...known, ...onPath].find((p) => p && fs.existsSync(p));
}

async function connect(url) {
  const ws = new WebSocket(url);
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = () => reject(new Error(`DevTools socket failed: ${url}`));
  });
  let seq = 0;
  const pending = new Map();
  const handlers = new Set();
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data);
    if (msg.id === undefined) return handlers.forEach((h) => h(msg));
    const p = pending.get(msg.id);
    pending.delete(msg.id);
    if (msg.error) p.reject(new Error(`${p.method}: ${msg.error.message}`));
    else p.resolve(msg.result);
  };
  return {
    send: (method, params = {}, sessionId) =>
      new Promise((resolve, reject) => {
        const id = ++seq;
        pending.set(id, { resolve, reject, method });
        ws.send(JSON.stringify({ id, method, params, sessionId }));
      }),
    on: (h) => (handlers.add(h), () => handlers.delete(h)),
    close: () => ws.close(),
  };
}

async function launch(exe) {
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'midgard-capture-'));
  const proc = spawn(exe, [
    '--headless=new', '--disable-gpu', '--hide-scrollbars', '--mute-audio',
    '--no-first-run', '--no-default-browser-check', '--disable-extensions',
    `--user-data-dir=${profile}`, '--remote-debugging-port=0',
    `--window-size=${VIEW.width},${VIEW.height}`,
    '--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1',
    'about:blank',
  ], { stdio: 'ignore' });
  const exited = new Promise((r) => proc.once('exit', r));

  const portFile = path.join(profile, 'DevToolsActivePort');
  let lines = [];
  for (let i = 0; i < 150 && lines.length < 2; i++) {
    await sleep(100);
    try {
      lines = fs.readFileSync(portFile, 'utf8').split('\n').filter(Boolean);
    } catch {
      /* not written yet */
    }
  }
  if (lines.length < 2) {
    proc.kill();
    throw new Error(`${exe} did not open a DevTools endpoint`);
  }
  const b = await connect(`ws://127.0.0.1:${lines[0]}${lines[1]}`);
  b.shutdown = async () => {
    await b.send('Browser.close').catch(() => {});
    b.close();
    await Promise.race([exited, sleep(5000)]);
    proc.kill();
    fs.rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  };
  return b;
}

// ── one screen ───────────────────────────────────────────────────────────────

const PAGE_STATE = `({
  text: document.body ? document.body.innerText.length : 0,
  fonts: document.fonts.status === 'loaded',
  light: document.documentElement.classList.contains('mode-light'),
})`;

async function shoot(b, origin, mode, name) {
  const { targetId } = await b.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await b.send('Target.attachToTarget', { targetId, flatten: true });
  const s = (method, params) => b.send(method, params, sessionId);
  const problems = [];
  const urls = new Map();
  let loaded;
  const onLoad = new Promise((r) => (loaded = r));

  const off = b.on((m) => {
    if (m.sessionId !== sessionId) return;
    const p = m.params;
    switch (m.method) {
      case 'Page.loadEventFired':
        loaded();
        break;
      case 'Fetch.requestPaused': {
        const file = vendorPath(p.request.url);
        if (!fs.existsSync(file)) {
          problems.push(`not vendored: ${p.request.url} (run --fetch-vendor once)`);
          s('Fetch.failRequest', { requestId: p.requestId, errorReason: 'BlockedByClient' }).catch(() => {});
          break;
        }
        s('Fetch.fulfillRequest', {
          requestId: p.requestId,
          responseCode: 200,
          responseHeaders: [
            { name: 'Content-Type', value: 'text/javascript' },
            { name: 'Access-Control-Allow-Origin', value: '*' },
          ],
          body: fs.readFileSync(file).toString('base64'),
        }).catch(() => {});
        break;
      }
      case 'Network.requestWillBeSent':
        urls.set(p.requestId, p.request.url);
        break;
      case 'Network.loadingFailed':
        if (p.errorText !== 'net::ERR_ABORTED' && !p.blockedReason) {
          problems.push(`request failed: ${urls.get(p.requestId) ?? p.requestId} (${p.errorText})`);
        }
        break;
      case 'Network.responseReceived':
        if (p.response.status >= 400 && !p.response.url.endsWith('/favicon.ico')) {
          problems.push(`HTTP ${p.response.status}: ${p.response.url}`);
        }
        break;
      case 'Runtime.exceptionThrown':
        problems.push(`page exception: ${p.exceptionDetails.exception?.description ?? p.exceptionDetails.text}`);
        break;
    }
  });

  try {
    await s('Page.enable');
    await s('Network.enable');
    await s('Runtime.enable');
    await s('Fetch.enable', { patterns: [{ urlPattern: 'https://unpkg.com/*' }] });
    await s('Emulation.setDeviceMetricsOverride', { ...VIEW, mobile: false });
    await s('Emulation.setScrollbarsHidden', { hidden: true });
    await s('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: mode }] });
    // The consoles and the site read the mode before first paint, so it is set
    // before any page script runs. mg-consent keeps the site's one-time storage
    // notice closed, as it is for a returning visitor.
    await s('Page.addScriptToEvaluateOnNewDocument', {
      source: `try{localStorage.setItem('mg-mode',${JSON.stringify(mode)});localStorage.setItem('mg-consent','capture')}catch(e){}`,
    });
    await s('Page.navigate', { url: origin + SHOTS[name] });
    await Promise.race([onLoad, sleep(30_000)]);

    // Settle: fonts loaded and the rendered text unchanged for 1.5s, after at
    // least 2.5s (the site compiles its JSX in the page), then a beat for
    // entry transitions.
    const t0 = Date.now();
    let state = { text: 0 };
    let last = -1;
    let quiet = 0;
    while (Date.now() - t0 < 25_000) {
      await sleep(500);
      state = (await s('Runtime.evaluate', { expression: PAGE_STATE, returnByValue: true })).result.value;
      quiet = state.text === last ? quiet + 1 : 0;
      last = state.text;
      if (state.fonts && quiet >= 3 && Date.now() - t0 >= 2500) break;
    }
    await sleep(1000);

    if (state.light !== (mode === 'light')) problems.push(`page did not render in ${mode} mode`);
    if (state.text < 200) problems.push(`page looks blank (${state.text} characters of text)`);
    if (problems.length) return { problems };
    const { data } = await s('Page.captureScreenshot', { format: 'png' });
    return { png: Buffer.from(data, 'base64') };
  } finally {
    off();
    await b.send('Target.closeTarget', { targetId }).catch(() => {});
  }
}

// ── main ─────────────────────────────────────────────────────────────────────

const USAGE = `usage: node docs/demo/capture.mjs [options]

  --mode dark|light|both   colour mode(s) to capture          (default: both)
  --only a,b,...           capture just these screens         (default: all ${Object.keys(SHOTS).length})
  --out DIR                writes DIR/<mode>/<name>.png       (default: docs/demo/screenshots)
  --browser PATH           Chromium browser                   (default: $CHROME_PATH, else found)
  --fetch-vendor           fill vendor/ for the site pages and exit
  --list                   print the screens and their paths and exit`;

async function main() {
  const { values: opt } = parseArgs({
    options: {
      mode: { type: 'string', default: 'both' },
      only: { type: 'string' },
      out: { type: 'string', default: path.join(HERE, 'screenshots') },
      browser: { type: 'string', default: process.env.CHROME_PATH },
      'fetch-vendor': { type: 'boolean', default: false },
      list: { type: 'boolean', default: false },
      help: { type: 'boolean', short: 'h', default: false },
    },
  });
  if (opt.help) return console.log(USAGE);
  if (opt.list) return Object.entries(SHOTS).forEach(([n, p]) => console.log(`${n.padEnd(20)} ${p}`));
  if (opt['fetch-vendor']) return fetchVendor();

  const modes = opt.mode === 'both' ? MODES : [opt.mode];
  if (!modes.every((m) => MODES.includes(m))) throw new Error(`--mode must be dark, light or both\n\n${USAGE}`);
  const names = opt.only ? opt.only.split(',').map((n) => n.trim()) : Object.keys(SHOTS);
  const unknown = names.filter((n) => !SHOTS[n]);
  if (unknown.length) throw new Error(`unknown screen(s): ${unknown.join(', ')} (see --list)`);

  const unbuilt = [...new Set(names.map((n) => SHOTS[n].split('#')[0]))]
    .filter((p) => p.includes('/dist/') && !fs.existsSync(path.join(ROOT, p)));
  if (unbuilt.length) throw new Error(`not built:\n  ${unbuilt.join('\n  ')}\nBuild the consoles first; see README.md.`);
  if (names.some((n) => n.startsWith('site-')) && !VENDOR_URLS.every((u) => fs.existsSync(vendorPath(u)))) {
    throw new Error('the site pages need vendor/: run `node docs/demo/capture.mjs --fetch-vendor` once');
  }

  const exe = opt.browser ?? findBrowser();
  if (!exe) throw new Error('no Chromium browser found: pass --browser PATH or set CHROME_PATH');

  const server = await serve();
  const origin = `http://127.0.0.1:${server.address().port}`;
  const b = await launch(exe);
  let failed = 0;
  try {
    for (const mode of modes) {
      fs.mkdirSync(path.join(opt.out, mode), { recursive: true });
      for (const name of names) {
        const { png, problems } = await shoot(b, origin, mode, name);
        const label = `${mode}/${name}`.padEnd(26);
        if (problems) {
          failed++;
          console.log(`${label} FAILED, kept the previous capture\n    ${problems.join('\n    ')}`);
          continue;
        }
        fs.writeFileSync(path.join(opt.out, mode, `${name}.png`), png);
        console.log(`${label} ${png.readUInt32BE(16)}x${png.readUInt32BE(20)}  ${Math.round(png.length / 1024)} KB`);
      }
    }
  } finally {
    await b.shutdown();
    server.close();
  }
  const total = modes.length * names.length;
  console.log(failed ? `${failed} of ${total} screens failed` : `${total} screens -> ${opt.out}`);
  process.exitCode = failed ? 1 : 0;
}

main().catch((e) => {
  console.error(e.message);
  process.exitCode = 1;
});
