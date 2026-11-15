// A browser signed in to the site sees "Subscribed" in place of the subscribe
// form, from the first frame. The host sets a readable hint cookie beside its
// session; the form's inline script reads it before the body paints, sets
// `data-moss-subscribed` on the root, then asks /_moss/account/session, and
// only a 401 brings the form back. With no hint nothing is asked.
import { test, expect, type BrowserContext, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { buildScratchSite } from '../../e2e/helpers/scratch-site';
import { SUBSCRIBED_STATE_GATE } from '../../e2e/helpers/subscribed-state-site';

const root = buildScratchSite(SUBSCRIBED_STATE_GATE);
const ORIGIN = 'https://notes.test';
const HINT = { name: '__Host-moss_subscribed', value: '1', domain: 'notes.test', path: '/', secure: true, sameSite: 'Lax' as const };
const TYPES: Record<string, string> = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json', '.svg': 'image/svg+xml' };

/** Serves the built site at ORIGIN and answers the session question with `session`; returns the session requests seen. */
async function serve(context: BrowserContext, session: 200 | 401 = 200): Promise<string[]> {
  const asked: string[] = [];
  await context.route('**/*', async (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== ORIGIN) return route.abort();
    if (url.pathname === '/_moss/account/session') {
      asked.push(url.pathname);
      const headers: Record<string, string> = { 'content-type': 'application/json' };
      if (session === 401) headers['set-cookie'] = '__Host-moss_subscribed=; Max-Age=0; Path=/; Secure';
      return route.fulfill({ status: session, headers, body: JSON.stringify({ signed_in: session === 200 }) });
    }
    let file = path.join(root, decodeURIComponent(url.pathname));
    if (url.pathname.endsWith('/')) file = path.join(file, 'index.html');
    if (!fs.existsSync(file)) return route.fulfill({ status: 404, body: '' });
    return route.fulfill({ status: 200, headers: { 'content-type': TYPES[path.extname(file)] ?? 'application/octet-stream' }, body: fs.readFileSync(file) });
  });
  return asked;
}

/** Records, on the first frame, whether the root is marked and how the field is drawn. */
async function watchFirstFrame(page: Page): Promise<void> {
  await page.addInitScript(() => {
    requestAnimationFrame(() => {
      const field = document.querySelector('.moss-subscribe-form input[type="email"]');
      (window as unknown as { firstFrame: unknown }).firstFrame = {
        root: document.documentElement.hasAttribute('data-moss-subscribed'),
        field: field ? getComputedStyle(field).display : 'absent',
      };
    });
  });
}

const state = (page: Page) =>
  page.evaluate(() => {
    const form = document.querySelector('.moss-subscribe-form')!;
    const shown = (el: Element | null) => !!el && getComputedStyle(el).display !== 'none' && el.getBoundingClientRect().height > 0;
    return {
      root: document.documentElement.hasAttribute('data-moss-subscribed'),
      field: shown(form.querySelector('input[type="email"]')),
      button: shown(form.querySelector('.moss-btn-slot')),
      subscribed: shown(form.querySelector('.moss-subscribe-subscribed')),
      text: form.querySelector('.moss-subscribe-subscribed')!.textContent,
      hint: document.cookie.includes('__Host-moss_subscribed=1'),
    };
  });

for (const width of [390, 1280]) {
  test.describe(`at ${width}px`, () => {
    test.use({ viewport: { width, height: 900 } });

    test('with the hint, the first frame already shows "Subscribed" in place of the field and button', async ({ page, context }) => {
      await context.addCookies([HINT]);
      const asked = await serve(context);
      await watchFirstFrame(page);
      await page.goto(ORIGIN + '/', { waitUntil: 'load' });
      // Recorded once, at the first frame, which can come after `load`.
      await expect.poll(() => page.evaluate(() => (window as unknown as { firstFrame: unknown }).firstFrame)).toEqual({ root: true, field: 'none' });
      await expect.poll(() => asked.length).toBe(1);
      expect(await state(page)).toMatchObject({ root: true, field: false, button: false, subscribed: true, text: 'Subscribed' });
    });

    test('a 401 brings the form back and the hint is gone', async ({ page, context }) => {
      await context.addCookies([HINT]);
      await serve(context, 401);
      await page.goto(ORIGIN + '/', { waitUntil: 'load' });
      // The whole state, so a page that never showed "Subscribed" cannot pass before the 401 arrives.
      await expect.poll(() => state(page)).toMatchObject({ root: false, field: true, button: true, subscribed: false, hint: false });
    });

    test('no hint, no request, and the form; a tab coming back after a sign-in elsewhere turns to "Subscribed"', async ({ page, context }) => {
      const asked = await serve(context);
      await page.goto(ORIGIN + '/', { waitUntil: 'load' });
      await page.waitForTimeout(300);
      expect(asked).toEqual([]);
      expect(await state(page)).toMatchObject({ root: false, field: true, subscribed: false });
      await context.addCookies([HINT]);
      await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
      expect(await state(page)).toMatchObject({ root: true, field: false, subscribed: true });
      await expect.poll(() => asked.length).toBe(1);
    });
  });
}
