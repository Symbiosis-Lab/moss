// The page's theme is the colour scheme the browser draws with.
//
// site.css sets `color-scheme` on the root from `data-theme`, so form
// controls, scrollbars and an SVG picture's own `prefers-color-scheme` rule
// follow the page's toggle rather than the OS setting. Chromium and Firefox
// pass the scheme into an SVG picture. WebKit gives a picture the OS setting,
// which is why the guide tells a theme to recolour a logo in CSS instead; if
// that half ever fails, WebKit has changed and the guide sentence can go.
//
// The scheme reaches an embedded page too. A frame whose document declares no
// scheme is a light document: on a light page it stays transparent, on a dark
// page the browser gives it an opaque white canvas so its dark text can be
// read, whatever the OS setting.
import { test, expect, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { mossBuildAssets } from '../../support/crate-paths';

const CSS = fs.readFileSync(path.join(mossBuildAssets(), 'css', 'site.css'), 'utf8');
const SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><style>rect{fill:#f00}@media (prefers-color-scheme:dark){rect{fill:#00f}}</style><rect width="40" height="40"/></svg>';

const PAGE_GREEN = [0, 170, 0];

const page = (theme: 'light' | 'dark') => `<!doctype html><html lang="en" data-theme="${theme}"><head><meta charset="utf-8">
<style>${CSS}</style><style>body{margin:0;background:#0a0 !important}</style></head><body>
<img id="mark" src="data:image/svg+xml,${encodeURIComponent(SVG)}" width="40" height="40" alt="" style="display:block">
<iframe class="moss-embed" data-type="iframe" srcdoc="&lt;!doctype html&gt;&lt;body&gt;&lt;/body&gt;" style="display:block;width:100px;height:100px;aspect-ratio:auto;border:0;margin:0"></iframe>
</body></html>`;

/** The red, green and blue of the pixel drawn at (x, y). */
async function pixel(p: Page, x: number, y: number): Promise<number[]> {
  const shot = (await p.screenshot({ clip: { x, y, width: 1, height: 1 } })).toString('base64');
  return p.evaluate(async (b64) => {
    const img = new Image();
    img.src = `data:image/png;base64,${b64}`;
    await img.decode();
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 1;
    const context = canvas.getContext('2d')!;
    context.drawImage(img, 0, 0);
    return [...context.getImageData(0, 0, 1, 1).data].slice(0, 3);
  }, shot);
}

for (const os of ['light', 'dark'] as const) {
  for (const theme of ['light', 'dark'] as const) {
    test(`an OS set to ${os} and a page set to ${theme}`, async ({ browser, browserName }) => {
      const ctx = await browser.newContext({ viewport: { width: 200, height: 200 }, colorScheme: os });
      const p = await ctx.newPage();
      await p.setContent(page(theme));
      expect(await p.evaluate(() => getComputedStyle(document.documentElement).colorScheme)).toBe(theme);

      await p.locator('#mark').evaluate((img: HTMLImageElement) => img.decode());
      const [red, , blue] = await pixel(p, 20, 20);
      expect(blue > red ? 'dark' : 'light').toBe(browserName === 'webkit' ? os : theme);

      const frame = await p.locator('iframe').boundingBox();
      const drawn = await pixel(p, frame!.x + 50, frame!.y + 50);
      await ctx.close();
      expect(drawn).toEqual(theme === 'dark' ? [255, 255, 255] : PAGE_GREEN);
    });
  }
}
