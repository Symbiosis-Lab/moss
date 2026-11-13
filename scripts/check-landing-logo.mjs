#!/usr/bin/env node
// The mark at the top of every page must be visible on the ground it sits on,
// in the theme the page actually shows. A page's theme is the reader's stored
// choice first and the OS setting second, so the two can disagree: every page
// in the build's sitemap is opened under both OS settings with no stored
// choice, a light one and a dark one, at phone and desktop widths.
// The mark's main ink must stand at least 3:1 against the computed background
// behind it. The ground is read from computed style; the ink is read from a
// screenshot of the mark, because an <img> exposes no colour (an SVG file can
// repaint itself from the OS setting, which is what this check is for). One
// screenshot is taken per distinct picture, not per page.
// Set SCREENSHOT_DIR to keep those screenshots.
import { mkdir, writeFile } from 'node:fs/promises';
import { loadPlaywright, resolveBaseURL } from './landing-harness.mjs';

const MIN_CONTRAST = 3;
const MARK = 'img.site-logo, .brand .moss-mark svg';
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const engines = await loadPlaywright();
const engineNames = (process.env.ENGINE || 'chromium,webkit').split(',');
const screenshotDir = process.env.SCREENSHOT_DIR || null;
if (screenshotDir) await mkdir(screenshotDir, { recursive: true });

const sitemap = await (await fetch(new URL('sitemap.xml', baseURL))).text();
const paths = [...sitemap.matchAll(/<loc>([^<]+)<\/loc>/g)].map((m) => decodeURI(new URL(m[1]).pathname));
if (paths.length === 0) throw new Error(`no page listed in ${baseURL}sitemap.xml`);

// What the page says about its mark: the picture drawn, its box and the first opaque ground behind it.
const describe = async () => {
  const mark = document.querySelector(MARK_SELECTOR);
  if (!mark) return null;
  // An image has no box of its own until its file arrives, and a file swapped in by `content` arrives after `src`.
  if (mark.getBoundingClientRect().width === 0 && document.readyState !== 'complete') await new Promise((done) => addEventListener('load', done));
  const box = mark.getBoundingClientRect();
  let ground = null;
  for (let el = mark; el && !ground; el = el.parentElement) {
    const [r, g, b, a = 1] = getComputedStyle(el).backgroundColor.match(/[\d.]+/g).map(Number);
    if (a === 1) ground = [r, g, b];
  }
  const style = getComputedStyle(mark);
  return {
    theme: document.documentElement.getAttribute('data-theme'),
    picture: mark.tagName === 'IMG' ? `${mark.currentSrc} ${style.content}` : `inline ${style.color}`,
    visible: box.width > 0 && box.height > 0 && style.visibility === 'visible',
    ground: ground || [255, 255, 255],
  };
};

// The mark's main ink: the most common colour in its screenshot that is not the ground.
const mainInk = async (reader, png, ground) => reader.evaluate(async ({ b64, ground }) => {
  const bitmap = await createImageBitmap(await (await fetch(`data:image/png;base64,${b64}`)).blob());
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const context = canvas.getContext('2d');
  context.drawImage(bitmap, 0, 0);
  const { data } = context.getImageData(0, 0, bitmap.width, bitmap.height);
  const counts = new Map();
  for (let i = 0; i < data.length; i += 4) {
    if (Math.abs(data[i] - ground[0]) + Math.abs(data[i + 1] - ground[1]) + Math.abs(data[i + 2] - ground[2]) < 6) continue;
    const key = `${data[i]},${data[i + 1]},${data[i + 2]}`;
    counts.set(key, (counts.get(key) || 0) + 1);
  }
  const top = [...counts].sort((a, b) => b[1] - a[1])[0];
  return top ? top[0].split(',').map(Number) : null;
}, { b64: png.toString('base64'), ground });

const luminance = (rgb) => rgb.map((v) => v / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
  .reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
const contrast = (a, b) => { const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x); return (hi + 0.05) / (lo + 0.05); };

const failures = [];
const pictures = [];
let opened = 0;
const readerBrowser = await engines.chromium.launch();
const reader = await readerBrowser.newPage();
try {
  for (const engineName of engineNames) {
    const browser = await engines[engineName].launch();
    try {
      for (const width of [1280, 390]) for (const os of ['light', 'dark']) for (const stored of [null, 'light', 'dark']) {
        const cell = `${engineName} ${width}px OS ${os}, stored ${stored || 'none'}`;
        const context = await browser.newContext({ colorScheme: os, viewport: { width, height: 800 } });
        if (stored) await context.addInitScript((theme) => { try { localStorage.setItem('moss-theme', theme); } catch {} }, stored);
        const page = await context.newPage();
        const measured = new Map();
        try {
          for (const path of paths) {
            await page.goto(new URL(path.slice(1), baseURL).href, { waitUntil: 'domcontentloaded' });
            opened++;
            const mark = await page.evaluate(`const MARK_SELECTOR = ${JSON.stringify(MARK)}; (${describe})()`);
            if (!mark) { failures.push(`${cell} ${path}: no mark found (${MARK})`); continue; }
            if (!mark.visible) { failures.push(`${cell} ${path}: the mark has no visible box`); continue; }
            const key = `${mark.picture} on ${mark.ground}`;
            if (!measured.has(key)) {
              await page.waitForLoadState('load'); // the picture itself has arrived
              const png = await page.locator(MARK).first().screenshot();
              const ink = await mainInk(reader, png, mark.ground);
              const ratio = ink ? contrast(ink, mark.ground) : 1;
              measured.set(key, { ink, ratio });
              pictures.push({ cell, theme: mark.theme, picture: mark.picture, ground: mark.ground.join(','), ink: ink?.join(',') ?? 'none', contrast: +ratio.toFixed(2), firstPage: path });
              if (screenshotDir) await writeFile(`${screenshotDir}/${cell.replace(/[^\w]+/g, '-')}-${pictures.length}.png`, png);
            }
            const { ink, ratio } = measured.get(key);
            if (ratio < MIN_CONTRAST) failures.push(`${cell} ${path}: theme ${mark.theme || 'none'}, mark ink ${ink?.join(',') ?? 'none'} on ${mark.ground.join(',')} is ${ratio.toFixed(2)}:1, under ${MIN_CONTRAST}:1`);
          }
        } finally {
          await context.close();
        }
      }
    } finally {
      await browser.close();
    }
  }
} finally {
  await readerBrowser.close();
  await close();
}

console.log(JSON.stringify({ base: baseURL, pages: paths.length, opened, pictures, failures }, null, 2));
if (failures.length) console.error(`check-landing-logo: ${failures.length} of ${opened} page views show a mark that does not stand out from its ground.`);
process.exitCode = failures.length ? 1 : 0;
