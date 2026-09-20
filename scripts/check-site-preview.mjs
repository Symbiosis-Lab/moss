#!/usr/bin/env node
// Verify the served site, including the assets whose failure leaves an HTTP-200 page unstyled.
import { readFile } from 'node:fs/promises';
import { resolveBaseURL } from './landing-harness.mjs';

const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const base = new URL(baseURL);
const contract = JSON.parse(await readFile(new URL('./landing-routes.json', import.meta.url), 'utf8'));
const landingRoutes = new Set(['/', '/zh-hans/', '/zh-hant/']);
const landingLocale = { '/': 'en', '/zh-hans/': 'zh-hans', '/zh-hant/': 'zh-hant' };
const routes = [...landingRoutes, ...contract.required];
const failures = [];
const assets = new Map();
const attr = (tag, name) => tag.match(new RegExp(`\\b${name}\\s*=\\s*["']([^"']*)["']`, 'i'))?.[1];
const localAsset = (path, pageUrl) => {
  if (!path || path.startsWith('data:')) return null;
  const url = new URL(path.replaceAll('&amp;', '&'), pageUrl);
  return url.origin === base.origin ? url : null;
};
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
  if (landingRoutes.has(route) && !html.includes('id="intro"')) failures.push({ url: String(url), problem: 'Custom landing homepage is missing' });
  if (landingRoutes.has(route)) {
    // The footer privacy link must be same-origin (so it works on any host, including staging)
    // and must resolve to the page in the SAME language as the landing page it's linked from.
    const footerNav = html.match(/<nav aria-label="[^"]*"[^>]*>[\s\S]*?<\/nav>/i)?.[0] ?? '';
    const privacyHref = footerNav.match(/<a\b[^>]*\bhref="([^"]+)"/i)?.[1];
    if (!privacyHref) {
      failures.push({ url: String(url), problem: 'Footer privacy link not found' });
    } else {
      const privacyUrl = new URL(privacyHref.replaceAll('&amp;', '&'), url);
      if (privacyUrl.origin !== base.origin) {
        failures.push({ url: String(url), problem: `Footer privacy link is not same-origin: ${privacyHref}` });
      } else {
        const expectedLocale = landingLocale[route];
        const privacyHtml = await read(privacyUrl, /text\/html/i);
        if (privacyHtml !== null) {
          const lang = privacyHtml.match(/<html\b[^>]*\blang="([^"]+)"/i)?.[1]?.toLowerCase();
          if (lang !== expectedLocale) {
            failures.push({ url: String(privacyUrl), problem: `Privacy page lang "${lang}" does not match the "${expectedLocale}" landing page linking to it (${url})` });
          }
        }
      }
    }
  }
  const baseTag = html.match(/<base\b[^>]*>/i)?.[0];
  const assetBase = baseTag ? new URL(attr(baseTag, 'href') || '.', url) : url;
  let styles = 0;
  for (const [tag] of html.matchAll(/<(?:link|script)\b[^>]*>/gi)) {
    const isStyle = /\bstylesheet\b/i.test(attr(tag, 'rel') || '');
    const isIcon = /(?:^|\s)icon(?:\s|$)/i.test(attr(tag, 'rel') || '');
    const path = isStyle || isIcon ? attr(tag, 'href') : /^<script\b/i.test(tag) ? attr(tag, 'src') : null;
    if (!path) continue;
    if (isStyle) styles++;
    const asset = localAsset(path, assetBase);
    if (asset) assets.set(asset.href, isStyle ? 'css' : isIcon ? 'image' : 'js');
  }
  for (const [tag] of html.matchAll(/<img\b[^>]*>/gi)) {
    const asset = localAsset(attr(tag, 'src'), assetBase);
    if (asset) assets.set(asset.href, 'image');
  }
  if (!landingRoutes.has(route) && !styles) failures.push({ url: String(url), problem: 'Documentation has no linked stylesheet' });
}
for (const [url, kind] of assets) {
  const expected = kind === 'css' ? /text\/css/i : kind === 'js' ? /(?:javascript|ecmascript)/i : /^image\//i;
  const text = await read(url, expected);
  if (kind !== 'image' && text !== null && /^\s*</.test(text)) failures.push({ url, problem: 'HTML returned in place of a stylesheet or script' });
}
console.log(JSON.stringify({ baseUrl: base.href, pagesChecked: routes.length, assetsChecked: assets.size, failures }, null, 2));
process.exitCode = failures.length ? 1 : 0;
await close();
