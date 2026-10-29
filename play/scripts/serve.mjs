#!/usr/bin/env node
/* Serve play/dist on loopback, with no dependencies. [ADR-PLAY-005]
 *
 *   node play/scripts/serve.mjs [port]      default 4173, or PLAY_PORT
 *
 * Binds to 127.0.0.1 only: the playground is for this machine. Directories
 * serve their index.html; anything outside dist is refused. */

import { createServer } from 'node:http';
import { createReadStream, statSync } from 'node:fs';
import { dirname, extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(process.env.PLAY_DIST ?? join(dirname(fileURLToPath(import.meta.url)), '..', 'dist'));
const PORT = Number(process.argv[2] ?? process.env.PLAY_PORT ?? 4173);
const HOST = '127.0.0.1';

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
  '.ttf': 'font/ttf',
  '.woff2': 'font/woff2',
  '.txt': 'text/plain; charset=utf-8',
};

function resolvePath(urlPath) {
  let p;
  try {
    p = decodeURIComponent(urlPath.split('?')[0]);
  } catch {
    return null;
  }
  const file = normalize(join(ROOT, p));
  if (file !== ROOT && !file.startsWith(ROOT + sep)) return null;
  try {
    const st = statSync(file);
    if (st.isDirectory()) return resolvePath(join(p, 'index.html'));
    return st.isFile() ? file : null;
  } catch {
    return null;
  }
}

try {
  statSync(join(ROOT, 'index.html'));
} catch {
  console.error(`play: ${ROOT} has no index.html; build it first with: node play/build.mjs`);
  process.exit(1);
}

createServer((req, res) => {
  if (req.method !== 'GET' && req.method !== 'HEAD') {
    res.writeHead(405, { allow: 'GET, HEAD' }).end();
    return;
  }
  const file = resolvePath(req.url ?? '/');
  if (!file) {
    res.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' }).end('not found');
    return;
  }
  res.writeHead(200, {
    'content-type': MIME[extname(file).toLowerCase()] ?? 'application/octet-stream',
    'x-content-type-options': 'nosniff',
    'referrer-policy': 'no-referrer',
    'cache-control': 'no-store',
  });
  if (req.method === 'HEAD') res.end();
  else createReadStream(file).pipe(res);
}).listen(PORT, HOST, () => {
  console.log(`\n  Midgard playground: http://${HOST}:${PORT}/\n  Sample data, served from ${ROOT}. Press Ctrl+C to stop.\n`);
});
