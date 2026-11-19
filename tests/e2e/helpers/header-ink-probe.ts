/**
 * What a site header laid over a picture (`header=over`) must clear: each piece
 * of header text against the worst pixel under its box.
 *
 * The ground under each piece of text is the page as painted with the text made
 * transparent, so it includes the picture and the wash; the text's computed
 * colour is compared to the lightest and darkest pixel under its box.
 */
import { expect, type Page } from '@playwright/test';

const TEXT = 'header .site-name, header .nav-links a';

export interface Probe {
  label: string;
  color: string;
  lightest: number;
  darkest: number;
}

export async function probes(page: Page): Promise<Probe[]> {
  await page.evaluate(() => document.fonts.ready);
  // The opening picture loads eagerly; lazy ones may never load in view, and the lightbox's has no source.
  await page.waitForFunction(() => [...document.images].filter((i) => i.loading !== 'lazy' && !i.hidden).every((i) => i.complete && i.naturalWidth > 0));
  const items = await page.$$eval(TEXT, (els) =>
    els
      .map((el) => {
        const r = el.getBoundingClientRect();
        return { label: (el.textContent ?? '').trim(), color: getComputedStyle(el).color, x: r.x, y: r.y, w: r.width, h: r.height };
      })
      .filter((i) => i.w > 0 && i.h > 0),
  );
  expect(items.length, 'the header has a site name and nav links').toBeGreaterThan(1);
  await page.addStyleTag({ content: 'header * { color: transparent !important; text-shadow: none !important; }' });
  const out: Probe[] = [];
  for (const item of items) {
    const shot = await page.screenshot({ clip: { x: Math.floor(item.x), y: Math.floor(item.y), width: Math.ceil(item.w), height: Math.ceil(item.h) } });
    const [lightest, darkest] = await page.evaluate(async (b64) => {
      const bitmap = await createImageBitmap(await (await fetch(`data:image/png;base64,${b64}`)).blob());
      const canvas = document.createElement('canvas');
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      const ctx = canvas.getContext('2d')!;
      ctx.drawImage(bitmap, 0, 0);
      const px = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
      const lum = (c: number) => ((c /= 255) <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4));
      let hi = 0;
      let lo = 1;
      for (let i = 0; i < px.length; i += 4) {
        const l = 0.2126 * lum(px[i]) + 0.7152 * lum(px[i + 1]) + 0.0722 * lum(px[i + 2]);
        hi = Math.max(hi, l);
        lo = Math.min(lo, l);
      }
      return [hi, lo];
    }, shot.toString('base64'));
    out.push({ label: item.label, color: item.color, lightest, darkest });
  }
  return out;
}
