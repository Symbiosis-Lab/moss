#!/usr/bin/env node
// What a real-browser check on the compiled site needs and would otherwise duplicate: loading
// Playwright from PLAYWRIGHT_MODULE, and serving the compiled build so a script can run with no
// argument. A script that is handed a URL on the command line still uses it as-is, so the same
// script runs against a deployed site with no code change.
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

// Resolves what every script used to require as `process.argv[2]`: a URL still wins when given
// (so a script runs unchanged against a deployed site), and its absence now serves `root`
// (default: the compiled build) from a free local port instead of requiring a separately started
// server.
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
