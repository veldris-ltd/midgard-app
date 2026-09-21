// Static server for the walkthrough: repo root at /, scratchpad pages at /launch/.
const http = require('http'), fs = require('fs'), path = require('path');
const ROOT = path.resolve(__dirname, '..', '..', '..');
const PAGES = path.join(__dirname, 'pages');
const MIME = { '.html':'text/html', '.js':'text/javascript', '.mjs':'text/javascript', '.jsx':'text/babel', '.css':'text/css', '.json':'application/json', '.svg':'image/svg+xml', '.png':'image/png', '.jpg':'image/jpeg', '.webp':'image/webp', '.ico':'image/x-icon', '.woff2':'font/woff2', '.woff':'font/woff', '.ttf':'font/ttf', '.otf':'font/otf', '.mp4':'video/mp4' };
http.createServer((req, res) => {
  const p = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const file = p.startsWith('/launch/') ? path.join(PAGES, p.slice(8)) : path.join(ROOT, p);
  fs.readFile(file, (err, buf) => {
    if (err) { res.writeHead(404); return res.end(); }
    res.writeHead(200, { 'content-type': MIME[path.extname(file).toLowerCase()] ?? 'application/octet-stream', 'cache-control': 'no-store' });
    res.end(buf);
  });
}).listen(5173, '127.0.0.1', () => console.log('serving on http://localhost:5173'));
