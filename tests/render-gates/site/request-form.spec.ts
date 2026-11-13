// A `:::form` stacks its fields in one column that never scrolls the page
// sideways, keeps every control inside the form and the button 44px tall, shows
// a focus ring a keyboard user can see, marks a refused field in a colour that
// reads as text, and keeps the honeypot out of sight and out of the tab order.
import { test, expect, type Page } from '@playwright/test';
import { gatePort } from '../../../playwright/gate-ports';
import { expectFocusOnOwnEdge, expectNothingPaintedOutside } from './field-focus-edge';

const url = `http://localhost:${gatePort('request-form')}/`;

/** Contrast of two CSS colours of any syntax, resolved through a canvas pixel. */
async function contrast(page: Page, fg: string, bg: string): Promise<number> {
  return page.evaluate(([a, b]) => {
    const px = (c: string) => {
      const cv = document.createElement('canvas');
      cv.width = cv.height = 1;
      const g = cv.getContext('2d')!;
      g.fillStyle = c;
      g.fillRect(0, 0, 1, 1);
      return Array.from(g.getImageData(0, 0, 1, 1).data.slice(0, 3));
    };
    const lum = ([r, g, b]: number[]) => {
      const f = (v: number) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
      return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
    };
    const [l1, l2] = [lum(px(a)), lum(px(b))].sort((x, y) => y - x);
    return (l1 + 0.05) / (l2 + 0.05);
  }, [fg, bg]);
}

for (const scheme of ['light', 'dark'] as const) {
  for (const width of [390, 1280]) {
    test.describe(`${scheme} at ${width}px`, () => {
      test.use({ viewport: { width, height: 900 }, colorScheme: scheme });

      test('fields stack in one column inside the form and the page does not scroll sideways', async ({ page }) => {
        await page.goto(url, { waitUntil: 'load' });
        const m = await page.evaluate(() => {
          const form = document.querySelector('.moss-form form') as HTMLElement;
          const fr = form.getBoundingClientRect();
          const boxes = [...form.querySelectorAll('.moss-form__field')].map((f) => f.getBoundingClientRect());
          const controls = [...form.querySelectorAll('input:not([type=hidden]):not([name=_hp]), select, textarea')].map((c) => c.getBoundingClientRect());
          const btn = (form.querySelector('.moss-btn') as HTMLElement).getBoundingClientRect();
          return {
            scrollWidth: document.documentElement.scrollWidth,
            clientWidth: document.documentElement.clientWidth,
            formLeft: fr.left, formRight: fr.right,
            tops: boxes.map((b) => b.top),
            outside: controls.filter((c) => c.left < fr.left - 0.5 || c.right > fr.right + 0.5).length,
            btnHeight: btn.height,
          };
        });
        expect(m.scrollWidth).toBeLessThanOrEqual(m.clientWidth);
        expect(m.formRight).toBeLessThanOrEqual(m.clientWidth);
        expect(m.tops).toEqual([...m.tops].sort((a, b) => a - b));
        expect(new Set(m.tops).size).toBe(m.tops.length);
        expect(m.outside).toBe(0);
        // One line of text tall on a mouse screen, 44px on a phone: the shared button rule.
        expect(m.btnHeight).toBeGreaterThanOrEqual(width < 768 ? 44 : 24);
        await page.screenshot({ path: `target/test-tmp/request-form-${scheme}-${width}.png`, fullPage: true });
      });

      test('keyboard focus draws a ring and never lands on the honeypot', async ({ page }) => {
        await page.goto(url, { waitUntil: 'load' });
        await page.locator('main, body').first().focus().catch(() => {});
        await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
        const seen: string[] = [];
        for (let i = 0; i < 30; i++) {
          await page.keyboard.press('Tab');
          const read = () => page.evaluate(() => {
            const el = document.activeElement as HTMLElement;
            const cs = getComputedStyle(el);
            // A date field's calendar button takes focus inside the field and draws its own ring.
            return { name: (el as HTMLInputElement).name || el.tagName, inForm: !!el.closest('.moss-form'), visible: el.matches(':focus-visible'), ring: (cs.outlineStyle !== 'none' && parseFloat(cs.outlineWidth) >= 2) || (el.classList.contains('moss-input') && cs.boxShadow !== 'none') };
          });
          // A field's edge transitions in; WebKit reports none for the frame it starts.
          let s = await read();
          if (s.visible && !s.ring) { await page.waitForTimeout(350); s = await read(); }
          if (!s.inForm) continue;
          seen.push(s.name);
          if (s.visible) expect(s.ring, `${s.name} shows a focus ring`).toBe(true);
        }
        expect(seen).toContain('your_name');
        expect(seen).toContain('anything_we_should_know'); // Safari's Tab skips radio buttons
        expect(seen).not.toContain('_hp');
        const hp = await page.locator('.moss-form__hp').boundingBox();
        expect(hp!.width).toBeLessThanOrEqual(1);
      });

      test('a click into the text area shows focus on its own edge, under a theme focus ring', async ({ page }) => {
        await page.goto(url, { waitUntil: 'load' });
        await expectFocusOnOwnEdge(page, page.locator('.moss-form textarea'), 'text area');
      });

      test('a click into an underline field lights its underline and draws no box, under a theme focus ring', async ({ page }) => {
        await page.goto(url, { waitUntil: 'load' });
        const field = page.locator('.moss-form input.moss-input:not(.moss-input--boxed)').first();
        await field.click();
        expect(await field.evaluate((el) => getComputedStyle(el).outlineStyle), 'no outline, whatever the theme says').toBe('none');
        expect(await field.evaluate((el) => getComputedStyle(el).boxShadow), 'the underline takes the accent').toContain('inset');
        await expectNothingPaintedOutside(page, field, 'underline field');
      });

      test('under forced colors a focused field keeps its edge inside itself, under a theme focus ring', async ({ page, browserName }) => {
        test.skip(browserName !== 'chromium', 'forced-colors emulation is Chromium only');
        await page.emulateMedia({ forcedColors: 'active' });
        await page.goto(url, { waitUntil: 'load' });
        for (const [what, field, width] of [
          ['underline field', page.locator('.moss-form input.moss-input:not(.moss-input--boxed)').first(), '2px'],
          ['text area', page.locator('.moss-form textarea'), '1px'],
        ] as const) {
          await field.scrollIntoViewIfNeeded();
          const before = await field.boundingBox();
          await field.click();
          const s = await field.evaluate((el) => { const cs = getComputedStyle(el); return { style: cs.outlineStyle, width: cs.outlineWidth, offset: cs.outlineOffset }; });
          expect(s, `${what}: a ring drawn inside the field`).toEqual({ style: 'solid', width, offset: '-2px' });
          expect(await field.boundingBox(), `${what}: focus moves nothing`).toEqual(before);
        }
      });

      test('a refused field is marked and its message reads as text', async ({ page }) => {
        await page.goto(url, { waitUntil: 'load' });
        await page.evaluate(() => {
          document.querySelector('#booking-email')!.setAttribute('aria-invalid', 'true');
          const err = document.querySelector('#booking-email-error') as HTMLElement;
          err.textContent = 'Check this value.';
          err.hidden = false;
        });
        await page.waitForTimeout(400); // the field's border colour transitions
        const m = await page.evaluate(() => {
          const input = document.querySelector('#booking-email') as HTMLInputElement;
          const err = document.querySelector('#booking-error') as HTMLElement | null ?? document.querySelector('#booking-email-error') as HTMLElement;
          const bg = getComputedStyle(document.body).backgroundColor;
          return {
            border: getComputedStyle(input).borderBlockEndColor,
            normal: getComputedStyle(document.querySelector('#booking-your_name') as Element).borderBlockEndColor,
            errColor: getComputedStyle(err).color,
            bg,
            visible: err.getBoundingClientRect().height > 0,
          };
        });
        expect(m.visible).toBe(true);
        expect(m.border).not.toBe(m.normal);
        expect(await contrast(page, m.errColor, m.bg)).toBeGreaterThanOrEqual(4.5);
      });
    });
  }
}
