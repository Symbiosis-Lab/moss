#!/usr/bin/env node
// The header (the wordmark and the language picker) stays readable over whatever scrolls under it: on the
// close's dark, where its text has turned white, it stands on the dark too, and it turns back as the reader
// scrolls up out of the close. Contrast is measured as WCAG's, the header's text colour against what is
// behind it with the text hidden (its own ground included), on a phone and on desktop, in every locale.
import { loadPlaywright, resolveBaseURL } from './landing-harness.mjs';

const { chromium, webkit } = await loadPlaywright();
const { baseURL, close } = await resolveBaseURL(process.argv[2]);
const assert = (ok, message) => { if (!ok) throw new Error(message); };
const AA = 4.5;

// once the leg, the film's crossfade and the header's own turn have all come to rest
const settled = (page, scene) => page.evaluate(async (scene) => {
  const t0 = performance.now();
  for (let last = ''; performance.now() - t0 < 40000; await new Promise((r) => setTimeout(r, 300))) {
    const s = __state(), nav = getComputedStyle(document.documentElement).getPropertyValue('--nav').trim();
    const now = JSON.stringify([s.shown, !!s.leg, s.xf, scrollY, nav]);
    if (now === last && s.shown === scene && !s.leg && (nav === '0' || nav === '1')) return;
    last = now;
  }
  throw new Error(`never came to rest on scene ${scene + 1}: ${JSON.stringify(__state())}`);
}, scene);

async function headerContrast(page) {
  const hide = await page.addStyleTag({ content: '.brand, .brand *, .language-picker, .language-picker * { color: transparent !important; } .brand svg { visibility: hidden !important; }' });
  await page.waitForTimeout(50);
  const shot = (await page.screenshot()).toString('base64');
  await hide.evaluate((node) => node.remove());
  return page.evaluate(async (b64) => {
    const img = new Image(); img.src = `data:image/png;base64,${b64}`; await img.decode();
    const k = img.width / innerWidth, canvas = document.createElement('canvas');
    canvas.width = img.width; canvas.height = img.height;
    const g = canvas.getContext('2d'); g.drawImage(img, 0, 0);
    const lin = (v) => { v /= 255; return v <= .03928 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4; };
    const lum = (r, gr, b) => .2126 * lin(r) + .7152 * lin(gr) + .0722 * lin(b);
    return [...document.querySelectorAll('.brand .moss-wordmark, .language-picker .nav-lang-current, .language-picker a')].map((el) => {
      const box = el.getBoundingClientRect(), [r, gr, b] = getComputedStyle(el).color.match(/[\d.]+/g).map(Number), text = lum(r, gr, b);
      const d = g.getImageData(Math.round(box.left * k), Math.round(box.top * k), Math.max(1, Math.round(box.width * k)), Math.max(1, Math.round(box.height * k))).data;
      const ratios = [];
      for (let i = 0; i < d.length; i += 4) { const ground = lum(d[i], d[i + 1], d[i + 2]); ratios.push((Math.max(text, ground) + .05) / (Math.min(text, ground) + .05)); }
      ratios.sort((x, y) => x - y);
      return { text: el.textContent.trim(), contrast: +ratios[Math.floor(ratios.length * .02)].toFixed(2) };
    });
  }, shot);
}
const readable = (where, items) => { for (const item of items) assert(item.contrast >= AA, `${where}: the header's "${item.text}" is at ${item.contrast}:1 over what is behind it`); };

try {
  for (const [engine, name, options, paths] of [
    [chromium, 'phone', { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true }, ['', 'zh-hans/', 'zh-hant/']],
    [webkit, 'phone', { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true }, ['']],
    [chromium, 'desktop', { viewport: { width: 1440, height: 900 } }, ['', 'zh-hant/']],
  ]) {
    const browser = await engine.launch();
    try {
      for (const path of paths) {
        const where = `${engine.name()} ${name} /${path}`;
        const page = await browser.newPage(options);
        await page.goto(new URL(path, baseURL).href);
        await page.waitForFunction(() => document.documentElement.dataset.ready === '1', null, { timeout: 60000 });
        await page.evaluate(() => scrollTo({ top: __restY(3), behavior: 'instant' }));
        await settled(page, 3);
        // the close's own copy scrolled up under the header, as far as the page goes
        await page.evaluate(() => scrollTo({ top: document.documentElement.scrollHeight, behavior: 'instant' }));
        await settled(page, 4);
        const turned = await page.evaluate(() => ({ dark: document.documentElement.classList.contains('nav-dark'), xf: __state().xf, y: scrollY }));
        assert(turned.dark, `${where}: the header did not turn dark over the close: ${JSON.stringify(turned)}`);
        readable(`${where} on the close`, await headerContrast(page));
        // and back up out of it
        await page.evaluate(() => scrollTo({ top: __restY(3), behavior: 'instant' }));
        await settled(page, 3);
        assert(await page.evaluate(() => !document.documentElement.classList.contains('nav-dark')), `${where}: the header stayed dark after the reader left the close`);
        readable(`${where} back on scene 4`, await headerContrast(page));
        await page.close();
      }
      console.log(`${engine.name()}: ${name}: the header is readable over the close and turns back above it (${paths.map((p) => `/${p}`).join(' ')})`);
    } finally { await browser.close(); }
  }
} finally { await close(); }
