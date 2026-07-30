// Minimal static server for the production bundle, used by the Playwright e2e
// run. Serves dist/fleetwatch-web/browser with SPA fallback to index.html, and the
// correct content types (the service worker — ngsw-worker.js / ngsw.json — must
// be served as JS/JSON or it won't register). No deps; Node stdlib only.
import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(fileURLToPath(new URL('.', import.meta.url)), '..', 'dist', 'fleetwatch-web', 'browser');
// No default, deliberately. A default is exactly how this drifts: each app's
// harness is copied from a sibling, the config's PORT is updated and this line
// is not — so running `node e2e/serve.mjs` by hand silently squats on the
// SIBLING's port and breaks that app's concurrent e2e run. All three apps that
// had a mismatch got it this way (utterance defaulted to recall's port, memview
// to health's, fleetwatch to life's). Playwright always passes the port, so
// requiring it costs nothing and makes the mismatch unrepresentable.
const PORT = Number(process.argv[2]);
if (!Number.isInteger(PORT) || PORT <= 0) {
  console.error("usage: node e2e/serve.mjs <port>   (the PORT from playwright.config.ts)");
  process.exit(2);
}

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.webmanifest': 'application/manifest+json; charset=utf-8',
  '.ico': 'image/x-icon',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
  '.woff2': 'font/woff2',
};

async function fileFor(urlPath) {
  const clean = normalize(decodeURIComponent(urlPath.split('?')[0])).replace(/^(\.\.[/\\])+/, '');
  const candidate = join(ROOT, clean);
  try {
    if ((await stat(candidate)).isFile()) return candidate;
  } catch {
    /* fall through to SPA fallback */
  }
  return join(ROOT, 'index.html');
}

// A tiny mock of the read API, so the offline-data e2e can prove that responses
// are cached and served with no network. Real prod is the Rust backend.
const API = {
  '/api/me': { userId: 'test', displayName: 'Test', avatarUrl: '', nextcloud: 'not_linked' },
  '/api/items': [
    { id: 1, product_id: null, name: 'Cached Avocado', brand: null, category: 'food',
      quantity: null, unit: null, expiry: null, location_id: null, barcode: null, has_image: false },
  ],
};

createServer(async (req, res) => {
  const path = (req.url ?? '/').split('?')[0];
  if (path.startsWith('/api/')) {
    res.writeHead(200, { 'content-type': 'application/json; charset=utf-8' });
    res.end(JSON.stringify(path in API ? API[path] : []));
    return;
  }
  const file = await fileFor(req.url ?? '/');
  try {
    const body = await readFile(file);
    res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' });
    res.end(body);
  } catch {
    res.writeHead(404).end('not found');
  }
}).listen(PORT, () => console.log(`serving ${ROOT} on http://localhost:${PORT}`));
