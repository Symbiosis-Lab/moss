#!/usr/bin/env node
// One place for what every scripts/check-landing-*.mjs (and the docs/preview
// checks that share the same compiled site) used to duplicate: loading
// Playwright from PLAYWRIGHT_MODULE and serving the compiled build so a
// script can run with no argument. A script that is handed a URL on the
// command line still uses it as-is, so the same script runs against a
// deployed site with no code change. Each script still launches and closes
// its own engines, the same per-engine loop it had before — only the
// boilerplate around it moved here.
import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { extname, join, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const HERE = fileURLToPath(new URL('.', import.meta.url));
export const DEFAULT_BUILD_DIR = resolve(HERE, '..', 'site', '.moss', 'build', 'current');

const MIME = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8', '.json': 'application/json; charset=utf-8', '.xml': 'application/xml; charset=utf-8',
  '.txt': 'text/plain; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg',
  '.gif': 'image/gif', '.webp': 'image/webp', '.ico': 'image/x-icon', '.woff': 'font/woff', '.woff2': 'font/woff2',
  '.mp4': 'video/mp4', '.webm': 'video/webm',
};

// Named viewport/context bundles a script can spread into newPage(), so a
// check that genuinely wants one of these four shapes says so by name
// instead of repeating the literal. Nothing requires a script to use one —
// a check with its own bespoke viewport keeps it.
export const PRESETS = {
  desktop: { viewport: { width: 1440, height: 900 } },
  smallDesktop: { viewport: { width: 1100, height: 700 } },
  phone: { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true },
  reducedMotion: { viewport: { width: 1440, height: 900 }, reducedMotion: 'reduce' },
};

async function resolveFile(root, urlPath) {
  const decoded = decodeURIComponent(urlPath);
  const target = resolve(join(root, decoded));
  if (target !== root && !target.startsWith(root + sep)) return null; // no escaping root
  const tryRead = async (path) => {
    const st = await stat(path).catch(() => null);
    if (!st) return null;
    if (st.isDirectory()) return null;
    return path;
  };
  if (!decoded.endsWith('/')) {
    const direct = await tryRead(target);
    if (direct) return direct;
  }
  const indexPath = join(decoded.endsWith('/') ? target : target + sep, 'index.html');
  return tryRead(indexPath);
}

async function serveDirectory(root) {
  const absRoot = resolve(root);
  const server = createServer(async (req, res) => {
    try {
      const url = new URL(req.url, 'http://localhost');
      const file = await resolveFile(absRoot, url.pathname);
      if (!file) { res.writeHead(404, { 'Content-Type': 'text/plain' }); res.end('Not found'); return; }
      const body = await readFile(file);
      res.writeHead(200, { 'Content-Type': MIME[extname(file)] || 'application/octet-stream' });
      res.end(body);
    } catch (error) {
      res.writeHead(500, { 'Content-Type': 'text/plain' });
      res.end(String(error?.stack || error));
    }
  });
  await new Promise((done, fail) => { server.once('error', fail); server.listen(0, '127.0.0.1', done); });
  const { port } = server.address();
  return { server, baseURL: `http://127.0.0.1:${port}/` };
}

// Resolves what every script used to require as `process.argv[2]`: a URL
// still wins when given (so a script runs unchanged against a deployed
// site), and its absence now serves `root` (default: the compiled build)
// from a free local port instead of requiring a separately started server.
export async function resolveBaseURL(argURL, { root = DEFAULT_BUILD_DIR } = {}) {
  if (argURL) {
    const href = new URL(argURL).href;
    return { baseURL: href.endsWith('/') ? href : href + '/', close: async () => {} };
  }
  const { server, baseURL } = await serveDirectory(root);
  return { baseURL, close: () => new Promise((done) => server.close(done)) };
}

export async function loadPlaywright() {
  const moduleName = process.env.PLAYWRIGHT_MODULE || 'playwright';
  try {
    return await import(moduleName.startsWith('/') ? pathToFileURL(moduleName).href : moduleName);
  } catch (error) {
    throw new Error(`Could not load Playwright from ${moduleName}. Set PLAYWRIGHT_MODULE to playwright/index.mjs in an existing install.\n${error}`);
  }
}

// Attaches the one error listener every check that cares about page errors
// duplicated by hand. Returns the live array so a script's own assertion
// reads it after the run, the same shape as before.
export function trackErrors(page) {
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  return errors;
}

// The true end of boot, not a proxy for it. `window.__landing.state().ready`
// (`!!(ed && sh) && primed()`) can go true while boot's own ready() function
// in the runtime script is still mid-flight -- ed/sh are assigned just before
// ready() is called, but ready() still has to decide whether to fire its own
// runJoin() call (independent of any caller's own scroll/join logic) before
// it finishes. A script that starts driving the page as soon as state().ready
// flips races that decision (found in check-landing-invariants.mjs's I-fuzz:
// a fuzz loop's own jumps landed mid-decision and a real join ran for
// reasons that had nothing to do with the call site under test). Boot sets
// documentElement.dataset.ready='1' only after that decision is made, on the
// success path; the failed-boot path (a demo iframe never initializing) sets
// dataset.static='1' instead and never reaches ready() at all. Every landing
// check waits on this one function rather than keeping its own variant, so
// there is one place that knows what "the page has settled its own boot
// decisions" means.
export async function whenReady(page, { timeout = 30000 } = {}) {
  const handle = await page.waitForFunction(() => {
    const root = document.documentElement.dataset;
    if (root.ready === '1') return 'ready';
    if (root.static === '1') return 'static';
    return false;
  }, null, { timeout });
  return handle.jsonValue();
}
