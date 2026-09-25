#!/usr/bin/env node
// Visit every Get Started page in all three locales and report on every image,
// video, iframe, and embedded <moss-ui-demo> demo: whether it loaded, what size
// it rendered at, and whether the page's console/network surfaced anything.
// Run against plain compiled output as well as the deployed site, and in both
// chromium and webkit — a static-server 200 does not prove the browser painted it.
import { loadPlaywright, resolveBaseURL } from './landing-harness.mjs';

// Same as every other check: no URL serves the local build instead of
// requiring one. One or more URLs still run against exactly what was
// passed, unchanged — this script's own reason to take several at once
// (plain compiled output as well as the deployed site) is a second base to
// add, not a replacement for the harness's default.
const noArgs = process.argv.length <= 2;
const { baseURL, close } = noArgs ? await resolveBaseURL() : { baseURL: null, close: async () => {} };
const bases = (noArgs ? [baseURL] : process.argv.slice(2)).map(arg => new URL(arg));
const engines = await loadPlaywright();
const engineNames = (process.env.ENGINE || 'chromium,webkit').split(',');

// Each locale's Get Started section root. Pages inside are discovered by
// walking moss's own generated series-nav (prev/next), not hardcoded here,
// so a page added or removed from the collection is picked up automatically.
const roots = ['/get-started/', '/zh-hans/开始使用/', '/zh-hant/開始使用/'];

const attr = (tag, name) => tag.match(new RegExp(`\\b${name}\\s*=\\s*["']([^"']*)["']`, 'i'))?.[1];
const tagsWithClass = (html, cls) => [...html.matchAll(/<a\b[^>]*>/gi)].filter(t => (attr(t[0], 'class') || '').includes(cls)).map(t => t[0]);

async function discoverSeries(base, rootPath) {
  // The collection's own index page carries no moss-series-nav (only its
  // children do), so seed the walk from the first child link on the index,
  // then follow BOTH prev and next from there. moss-series-nav is a straight
  // chain (position 1 of N has no prev, N of N has no next) rather than a
  // ring, so a next-only walk silently drops every page before the seed —
  // exactly the failure mode that first shipped here, and the reason this is
  // a breadth-first walk over both directions instead of a one-way loop.
  const rootUrl = new URL(rootPath, base);
  const rootHtml = await (await fetch(rootUrl)).text();
  const article = rootHtml.match(/<article\b[^>]*>[\s\S]*?<\/article>/i)?.[0] || rootHtml;
  const seedHref = [...article.matchAll(/<a\b[^>]*class="wikilink"[^>]*href="([^"]+)"/gi)].map(m => m[1])[0];
  const pages = [rootUrl.href];
  if (!seedHref) return pages;
  const seed = new URL(seedHref, rootUrl).href;
  const visited = new Set([seed]);
  const queue = [seed];
  let guard = 0;
  while (queue.length && guard++ < 100) {
    const current = queue.shift();
    pages.push(current);
    const html = await (await fetch(current)).text();
    const nav = html.match(/<nav class="moss-series-nav">[\s\S]*?<\/nav>/i)?.[0];
    if (!nav) continue;
    for (const cls of ['moss-series-nav-prev', 'moss-series-nav-next']) {
      const tag = tagsWithClass(nav, cls)[0];
      const href = tag && attr(tag, 'href');
      if (!href) continue;
      const neighbor = new URL(href, current).href;
      if (!visited.has(neighbor)) { visited.add(neighbor); queue.push(neighbor); }
    }
  }
  return pages;
}

async function auditPage(page, url) {
  const consoleErrors = [];
  const failedRequests = [];
  const responses = new Map();
  const onConsole = (msg) => { if (msg.type() === 'error') consoleErrors.push(msg.text()); };
  const onPageError = (err) => consoleErrors.push(String(err));
  const onRequestFailed = (req) => failedRequests.push({ url: req.url(), failure: req.failure()?.errorText });
  const onResponse = (res) => responses.set(res.url(), res);
  page.on('console', onConsole);
  page.on('pageerror', onPageError);
  page.on('requestfailed', onRequestFailed);
  page.on('response', onResponse);
  try {
    await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30000 });
    // Scroll the whole page in steps so `loading="lazy"` media gets a chance to trigger.
    await page.evaluate(async () => {
      const step = 400;
      for (let y = 0; y < document.body.scrollHeight; y += step) {
        window.scrollTo(0, y);
        await new Promise((resolve) => setTimeout(resolve, 60));
      }
      window.scrollTo(0, 0);
    });
    await page.waitForLoadState('networkidle', { timeout: 15000 }).catch(() => {});
    await page.waitForTimeout(500);
    const media = await page.evaluate(() => {
      const describe = (el) => {
        const rect = el.getBoundingClientRect();
        const cs = getComputedStyle(el);
        const src = el.currentSrc || el.src || el.getAttribute('src') || null;
        return {
          tag: el.tagName.toLowerCase(),
          src,
          loading: el.getAttribute('loading'),
          naturalWidth: 'naturalWidth' in el ? el.naturalWidth : null,
          naturalHeight: 'naturalHeight' in el ? el.naturalHeight : null,
          videoWidth: 'videoWidth' in el ? el.videoWidth : null,
          videoHeight: 'videoHeight' in el ? el.videoHeight : null,
          complete: 'complete' in el ? el.complete : null,
          rect: { width: Math.round(rect.width), height: Math.round(rect.height) },
          display: cs.display,
          visibility: cs.visibility,
          opacity: cs.opacity,
        };
      };
      const nodes = [...document.querySelectorAll('article img, article video, article iframe, moss-ui-demo')];
      return nodes.map(describe);
    });
    for (const item of media) {
      if (!item.src) continue;
      const absolute = new URL(item.src, url).href;
      const res = responses.get(absolute);
      item.httpStatus = res ? res.status() : null;
      item.contentType = res ? res.headers()['content-type'] || null : null;
      const contentLength = res?.headers()['content-length'];
      item.contentLength = contentLength ? Number(contentLength) : null;
    }
    // WebKit fires a spurious "cancelled" requestfailed for a lazy iframe it
    // ends up re-requesting and loading successfully right after (observed on
    // /get-started/from-matters/, which embeds the same demo twice) — real
    // only if no later response for that same URL ever succeeded.
    const trulyFailed = failedRequests.filter(f => !f.url.includes('/beacon') && responses.get(f.url)?.ok() !== true);
    return { url, media, consoleErrors, failedRequests: trulyFailed };
  } finally {
    page.off('console', onConsole);
    page.off('pageerror', onPageError);
    page.off('requestfailed', onRequestFailed);
    page.off('response', onResponse);
  }
}

const report = { bases: bases.map(String), engines: engineNames, pages: [] };
const failures = [];

// Root cause (see the commit/report this script ships with): the Get Started
// gifs were never compressed — up to 8.4MB each, ~23MB on one page — so on a
// realistic mobile connection they can take over a minute to finish loading,
// which reads as "the screenshots don't work" even though every byte
// eventually arrives. This budget is deliberately generous (it does not
// force a "loads in N seconds on 3G" target, which would need visible
// quality loss on the densest recording) but it does catch a regression back
// to an unoptimized, multi-megabyte source.
const MAX_GIF_BYTES = 4.5 * 1024 * 1024;
const MAX_PAGE_GIF_BYTES = 12 * 1024 * 1024;
// Console errors/failed requests unrelated to media (e.g. the moss-ui-demo
// iframe's own `/__moss/token` preview-detection probe, expected to 404 on
// a static deployment) are still recorded in the raw report above, but they
// are not this script's concern and would otherwise make it permanently red
// for a reason that has nothing to do with images, video, or iframes.
const isMediaRelated = (text) => /\.(gif|png|jpe?g|webp|mp4|webm|mov)\b|\/ui\/|\/assets\/(animations|guides)\//i.test(text);

for (const base of bases) {
  const pageUrls = new Set();
  for (const root of roots) {
    for (const url of await discoverSeries(base, root)) pageUrls.add(url);
  }
  for (const engineName of engineNames) {
    const browser = await engines[engineName].launch();
    try {
      for (const url of pageUrls) {
        const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
        try {
          const result = await auditPage(page, url);
          result.engine = engineName;
          report.pages.push(result);
          for (const item of result.media) {
            const isImg = item.tag === 'img';
            const isVideo = item.tag === 'video';
            const isDemo = item.tag === 'moss-ui-demo';
            const hidden = item.display === 'none' || item.visibility === 'hidden' || Number(item.opacity) === 0;
            if (hidden) {
              failures.push({ engine: engineName, url, src: item.src, tag: item.tag, problem: 'Rendered but not visible (display/visibility/opacity)' });
              continue;
            }
            if (item.rect.width === 0 || item.rect.height === 0) {
              failures.push({ engine: engineName, url, src: item.src, tag: item.tag, problem: `Zero-size render box (${item.rect.width}x${item.rect.height})` });
              continue;
            }
            if (isImg) {
              if (item.httpStatus !== null && item.httpStatus !== 200) failures.push({ engine: engineName, url, src: item.src, problem: `HTTP ${item.httpStatus}` });
              else if (!item.complete || item.naturalWidth === 0 || item.naturalHeight === 0) failures.push({ engine: engineName, url, src: item.src, problem: `Image did not decode (complete=${item.complete}, naturalWidth=${item.naturalWidth}, naturalHeight=${item.naturalHeight})` });
              if (/\.gif$/i.test(item.src) && item.contentLength && item.contentLength > MAX_GIF_BYTES) {
                failures.push({ engine: engineName, url, src: item.src, problem: `Screenshot gif is ${(item.contentLength / 1024 / 1024).toFixed(2)}MB, over the ${(MAX_GIF_BYTES / 1024 / 1024).toFixed(1)}MB budget` });
              }
            }
            if (isVideo && (item.videoWidth === 0 || item.videoHeight === 0)) {
              failures.push({ engine: engineName, url, src: item.src, problem: `Video has no decoded frame (videoWidth=${item.videoWidth}, videoHeight=${item.videoHeight})` });
            }
            if (isDemo) {
              // <moss-ui-demo> owns an internal iframe; a zero-size or hidden
              // custom element itself is caught by the checks above.
            }
          }
          for (const err of result.consoleErrors) if (isMediaRelated(err)) failures.push({ engine: engineName, url, problem: `Console error: ${err}` });
          for (const req of result.failedRequests) if (isMediaRelated(req.url)) failures.push({ engine: engineName, url, problem: `Failed request: ${req.url} (${req.failure})` });
          // A page's screenshots load roughly concurrently once a scroll brings
          // them all near-viewport (native `loading="lazy"` isn't one-at-a-time),
          // so the real budget users feel is the page's TOTAL gif payload, not
          // any single file — measured directly: on a throttled ~400kbps profile
          // the pre-fix "Get Started" page (~21MB of gifs) was still not fully
          // loaded after 185+ seconds. This does not chase a "fast on 3G" target
          // (out of reach without visible quality loss on the densest recording);
          // it catches a regression back toward that total.
          const pageGifBytes = result.media.filter(m => m.tag === 'img' && /\.gif$/i.test(m.src || '')).reduce((sum, m) => sum + (m.contentLength || 0), 0);
          if (pageGifBytes > MAX_PAGE_GIF_BYTES) failures.push({ engine: engineName, url, problem: `Page's total screenshot gif payload is ${(pageGifBytes / 1024 / 1024).toFixed(2)}MB, over the ${(MAX_PAGE_GIF_BYTES / 1024 / 1024).toFixed(1)}MB budget` });
        } finally {
          await page.close();
        }
      }
    } finally {
      await browser.close();
    }
  }
}

console.log(JSON.stringify({ ...report, failures }, null, 2));
process.exitCode = failures.length ? 1 : 0;
await close();
