#!/usr/bin/env node
/* Build the Midgard playground into play/dist. [ADR-PLAY-001] [ADR-PLAY-003]
 *
 *   node play/build.mjs
 *
 * 1. The shell (launcher and guided tour) from play/ into dist/.
 * 2. Each of the seven consoles from its own source and its own vite.config.ts,
 *    unchanged, into dist/consoles/<id>/. The Midgard console is built with
 *    VITE_GATEWAY=mock so it runs on its in-browser gateway; the other six run
 *    on theirs by default.
 * 3. A Content Security Policy written into every built console page that allows
 *    its one inline script, the colour-mode bootstrap, by SHA-256 hash and nothing
 *    else inline.
 *
 * The Vite configuration for the shell lives here, not in a vite.config.* file,
 * so tools/ci/js-artefacts.sh does not treat the playground as a release
 * artefact. */

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'vite';
import react from '@vitejs/plugin-react';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..');
const DIST = join(HERE, 'dist');

export const CONSOLE_IDS = ['midgard', 'forge', 'mimir', 'heimdall', 'bifrost', 'admin', 'clients'];

const CONSOLE_CSP = (hashes) =>
  [
    "default-src 'self'",
    `script-src 'self' ${hashes.map((h) => `'sha256-${h}'`).join(' ')}`.trim(),
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: blob:",
    "font-src 'self' data:",
    "connect-src 'self'",
    "frame-src 'none'",
    "base-uri 'none'",
    "form-action 'none'",
    "object-src 'none'",
  ].join('; ');

function commit() {
  if (process.env.GITHUB_SHA) return process.env.GITHUB_SHA.slice(0, 12);
  try {
    return execFileSync('git', ['rev-parse', '--short=12', 'HEAD'], { cwd: ROOT, encoding: 'utf8' }).trim();
  } catch {
    return 'unknown';
  }
}

const r = (p) => join(ROOT, p);
const alias = [
  { find: '@midgard/design-system/tokens.css', replacement: r('ui/design-system/src/tokens/styles.css') },
  { find: '@midgard/design-system', replacement: r('ui/design-system/src/index.ts') },
  { find: '@midgard/ui-shared/nav', replacement: r('ui/shared/src/nav.ts') },
  { find: '@midgard/ui-shared', replacement: r('ui/shared/src/index.ts') },
  { find: '@midgard/sdk', replacement: r('sdk/ts/src/index.ts') },
];

async function buildShell(sha) {
  await build({
    configFile: false,
    root: HERE,
    base: './',
    logLevel: 'warn',
    plugins: [react()],
    resolve: { alias },
    define: { __PLAY_COMMIT__: JSON.stringify(sha) },
    build: { outDir: DIST, emptyOutDir: true },
  });
}

async function buildConsole(id) {
  const root = r(`products/${id}/ui`);
  const prev = process.env.VITE_GATEWAY;
  if (id === 'midgard') process.env.VITE_GATEWAY = 'mock';
  try {
    await build({
      configFile: join(root, 'vite.config.ts'),
      root,
      logLevel: 'warn',
      build: { outDir: join(DIST, 'consoles', id), emptyOutDir: true },
    });
  } finally {
    if (prev === undefined) delete process.env.VITE_GATEWAY;
    else process.env.VITE_GATEWAY = prev;
  }
  pinCsp(join(DIST, 'consoles', id, 'index.html'));
}

/* Hash every inline <script> and put the CSP meta first in <head>. */
function pinCsp(file) {
  let html = readFileSync(file, 'utf8');
  const hashes = [...html.matchAll(/<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/g)].map((m) =>
    createHash('sha256').update(m[1], 'utf8').digest('base64'),
  );
  const meta = `<meta http-equiv="Content-Security-Policy" content="${CONSOLE_CSP(hashes)}" />`;
  html = html.replace(/<meta charset="utf-8" \/>/i, (m) => `${m}\n    ${meta}`);
  if (!html.includes('Content-Security-Policy')) throw new Error(`${file}: no <meta charset> to anchor the CSP`);
  writeFileSync(file, html);
}

const t0 = Date.now();
const sha = commit();
await buildShell(sha);
for (const id of CONSOLE_IDS) {
  if (!existsSync(r(`products/${id}/ui/vite.config.ts`))) throw new Error(`products/${id}/ui has no vite.config.ts`);
  await buildConsole(id);
  console.log(`play: built ${id}`);
}
writeFileSync(join(DIST, 'build.json'), JSON.stringify({ commit: sha, consoles: CONSOLE_IDS, builtAt: new Date().toISOString() }, null, 2) + '\n');
console.log(`play: dist ready in ${((Date.now() - t0) / 1000).toFixed(1)}s · commit ${sha}`);
