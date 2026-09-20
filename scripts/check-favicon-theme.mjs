#!/usr/bin/env node
// Verify the moss-mark favicon actually renders dark-on-light and light-on-dark,
// on the landing page AND the moss-built docs pages, in all three locales, in
// both chromium and webkit. This does not (cannot, via Playwright) inspect a
// real browser's tab-strip icon; instead it checks the mechanism that has to
// be right for that to happen: which icon URL each page's <link> resolves to
// under each OS color scheme, and that fetching and rasterizing THAT URL
// produces the right ink color. See site/.moss/theme/script.js and the inline
// script in site/index.html for why this is a runtime href swap rather than
// relying on the SVG's own embedded @media rule (Safari doesn't evaluate it).
import { pathToFileURL } from 'node:url';

if (!process.argv[2]) throw new Error('Usage: node scripts/check-favicon-theme.mjs <site-url>');
const base = new URL(process.argv[2]);
const moduleName = process.env.PLAYWRIGHT_MODULE || 'playwright';
const engines = await import(moduleName.startsWith('/') ? pathToFileURL(moduleName).href : moduleName);
const engineNames = (process.env.ENGINE || 'chromium,webkit').split(',');
const pages = [
  { path: '', name: 'landing-en' },
  { path: 'zh-hans/开始使用/', name: 'docs-zh-hans' },
  { path: 'zh-hant/開始使用/', name: 'docs-zh-hant' },
  { path: 'get-started/', name: 'docs-en' },
];
const expectRGB = { light: '0,0,0', dark: '255,255,255' };
const failures = [];
const results = [];

for (const engineName of engineNames) {
  const browser = await engines[engineName].launch();
  try {
    for (const scheme of ['light', 'dark']) {
      const context = await browser.newContext({ colorScheme: scheme });
      try {
        for (const { path, name } of pages) {
          const page = await context.newPage();
          try {
            await page.goto(new URL(path, base).href, { waitUntil: 'load' });
            await page.waitForTimeout(200); // let the matchMedia listener's initial apply() run
            const icons = await page.evaluate(() => [...document.querySelectorAll('link[rel~="icon"], link[rel="apple-touch-icon"]')].map(l => ({ rel: l.getAttribute('rel'), sizes: l.getAttribute('sizes'), href: l.getAttribute('href') })));
            const svgIcon = icons.find(i => i.href?.endsWith('.svg'));
            if (!svgIcon) { failures.push({ engine: engineName, scheme, page: name, problem: 'No SVG <link rel="icon"> found' }); continue; }
            if (scheme === 'dark' && !svgIcon.href.includes('favicon-dark')) failures.push({ engine: engineName, scheme, page: name, problem: `Dark scheme did not swap to a dark icon: ${svgIcon.href}` });
            if (scheme === 'light' && svgIcon.href.includes('favicon-dark')) failures.push({ engine: engineName, scheme, page: name, problem: `Light scheme resolved a dark icon: ${svgIcon.href}` });
            const dominant = await page.evaluate(async (href) => {
              const res = await fetch(href);
              const text = await res.text();
              const img = new Image();
              const url = URL.createObjectURL(new Blob([text], { type: 'image/svg+xml' }));
              await new Promise((resolve, reject) => { img.onload = resolve; img.onerror = reject; img.src = url; });
              const canvas = document.createElement('canvas');
              canvas.width = 64; canvas.height = 64;
              const ctx = canvas.getContext('2d');
              ctx.drawImage(img, 0, 0, 64, 64);
              const { data } = ctx.getImageData(0, 0, 64, 64);
              const counts = new Map();
              for (let i = 0; i < data.length; i += 4) {
                if (data[i + 3] < 200) continue; // skip transparent/anti-aliased edge pixels
                const key = `${data[i]},${data[i + 1]},${data[i + 2]}`;
                counts.set(key, (counts.get(key) || 0) + 1);
              }
              let best = null, bestCount = 0;
              for (const [key, count] of counts) if (count > bestCount) { best = key; bestCount = count; }
              return best;
            }, new URL(svgIcon.href, page.url()).href);
            results.push({ engine: engineName, scheme, page: name, href: svgIcon.href, dominantInkRGB: dominant });
            if (dominant !== expectRGB[scheme]) failures.push({ engine: engineName, scheme, page: name, problem: `Dominant ink ${dominant} for ${svgIcon.href}, expected ${expectRGB[scheme]} (${scheme} scheme)` });
          } finally {
            await page.close();
          }
        }
      } finally {
        await context.close();
      }
    }
  } finally {
    await browser.close();
  }
}

console.log(JSON.stringify({ base: base.href, results, failures }, null, 2));
if (failures.length === 0) {
  console.error('Note: this cannot verify that a real browser\'s tab UI repaints the icon at runtime (Playwright exposes no such API, in either engine) — only that the right file, with the right colors, is what the page hands the browser.');
}
process.exitCode = failures.length ? 1 : 0;
