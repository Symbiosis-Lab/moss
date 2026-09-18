#!/usr/bin/env node
// Verify the served site, including the assets whose failure leaves an HTTP-200 page unstyled.
import { readFile } from 'node:fs/promises';

const input = process.argv[2];
if (!input) {
  console.error('Usage: node scripts/check-site-preview.mjs <preview-url>');
  process.exit(2);
}
const base = new URL(input);
const contract = JSON.parse(await readFile(new URL('./landing-routes.json', import.meta.url), 'utf8'));
const routes = ['/', ...contract.required];
const failures = [];
const assets = new Map();
const attr = (tag, name) => tag.match(new RegExp(`\\b${name}\\s*=\\s*["']([^"']*)["']`, 'i'))?.[1];
async function read(url, expected) {
  try {
    const response = await fetch(url, { signal: AbortSignal.timeout(15000) });
    const type = response.headers.get('content-type') || '';
    const text = await response.text();
    if (!response.ok || !expected.test(type) || !text.trim()) {
      failures.push({ url: String(url), status: response.status, type, problem: 'Missing, empty, or wrong content type' });
      return null;
    }
    return text;
  } catch (error) {
    failures.push({ url: String(url), problem: error.message });
    return null;
  }
}
for (const route of routes) {
  const url = new URL(route, base);
  const html = await read(url, /text\/html/i);
  if (html === null) continue;
  if (/Directory listing for/i.test(html)) failures.push({ url: String(url), problem: 'Serving a directory listing instead of the site' });
  if (route === '/' && !html.includes('id="intro"')) failures.push({ url: String(url), problem: 'Custom landing homepage is missing' });
  let styles = 0;
  for (const [tag] of html.matchAll(/<(?:link|script)\b[^>]*>/gi)) {
    const isStyle = /\bstylesheet\b/i.test(attr(tag, 'rel') || '');
    const path = isStyle ? attr(tag, 'href') : /^<script\b/i.test(tag) ? attr(tag, 'src') : null;
    if (!path) continue;
    if (isStyle) styles++;
    const asset = new URL(path.replaceAll('&amp;', '&'), url);
    if (asset.origin === base.origin) assets.set(asset.href, isStyle ? 'css' : 'js');
  }
  if (route !== '/' && !styles) failures.push({ url: String(url), problem: 'Documentation has no linked stylesheet' });
}
for (const [url, kind] of assets) {
  const text = await read(url, kind === 'css' ? /text\/css/i : /(?:javascript|ecmascript)/i);
  if (text !== null && /^\s*</.test(text)) failures.push({ url, problem: 'HTML returned in place of a stylesheet or script' });
}
console.log(JSON.stringify({ baseUrl: base.href, pagesChecked: routes.length, assetsChecked: assets.size, failures }, null, 2));
process.exitCode = failures.length ? 1 : 0;
