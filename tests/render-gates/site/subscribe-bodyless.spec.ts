// The one-line `:::subscribe` form, through every state a reader can reach:
// idle, the button spinning while the post waits, the check and "check your
// email", the error line, the host's bot check and each way it ends, the
// members lead above the form, and a German page's words and scope. The
// returning reader's "Subscribed" is subscribed-state.spec.ts. Each state is
// also saved as a picture, so two builds can be compared pixel for pixel.
import { test, expect, newPage, type BrowserContext, type Page, type Route } from '../../support/browser-test';
import fs from 'node:fs';
import path from 'node:path';
import { buildScratchSite } from '../../e2e/helpers/scratch-site';
import { SUBSCRIBE_BODYLESS_GATE } from '../../e2e/helpers/subscribe-bodyless-site';

const root = buildScratchSite(SUBSCRIBE_BODYLESS_GATE);
const ORIGIN = 'https://notes.test';
const TYPES: Record<string, string> = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json', '.svg': 'image/svg+xml' };

type Post = { email: string; scope?: string; lang?: string; answers?: unknown };
type Answer = (route: Route, n: number) => Promise<void>;

/** What the form reported to the host's check page, in order. */
const reports: string[] = [];

/**
 * Serves the built site at ORIGIN. The nth subscribe POST goes to `answer`;
 * the host's check page posts `captcha` (a list of messages, in order) to the
 * page that opened it, or fails to load at all when `captcha` is 'blocked'.
 * Returns the POST bodies seen.
 */
async function serve(context: BrowserContext, answer: Answer, captcha: object[] | 'blocked' = []): Promise<Post[]> {
  const posts: Post[] = [];
  reports.length = 0;
  await context.route('**/*', async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    if (req.method() === 'POST' && url.pathname.endsWith('/subscribe')) {
      posts.push(req.postDataJSON());
      return answer(route, posts.length);
    }
    if (url.origin !== ORIGIN) return route.abort();
    if (url.pathname === '/captcha/report') {
      reports.push(req.postData() ?? '');
      return route.fulfill({ status: 204 });
    }
    if (url.pathname === '/captcha/') {
      if (captcha === 'blocked') return route.abort('blockedbyclient');
      const sends = captcha.map((m, i) => `setTimeout(() => parent.postMessage(${JSON.stringify(m)}, "*"), ${i * 200})`).join(';');
      return route.fulfill({ status: 200, headers: { 'content-type': 'text/html' }, body: `<script>${sends}</script>` });
    }
    if (url.pathname === '/_moss/account/session') {
      return route.fulfill({ status: 401, headers: { 'content-type': 'application/json' }, body: '{"signed_in":false}' });
    }
    let file = path.join(root, decodeURIComponent(url.pathname));
    if (url.pathname.endsWith('/')) file = path.join(file, 'index.html');
    if (!fs.existsSync(file)) return route.fulfill({ status: 404, body: '' });
    return route.fulfill({ status: 200, headers: { 'content-type': TYPES[path.extname(file)] ?? 'application/octet-stream' }, body: fs.readFileSync(file) });
  });
  return posts;
}

const json = (status: number, body: object) => (route: Route) =>
  route.fulfill({ status, headers: { 'content-type': 'application/json', 'access-control-allow-origin': '*' }, body: JSON.stringify(body) });
const OK = json(200, { subscribed: true, confirmationSent: true });
const CHECK = json(403, { need_captcha: true, captcha_url: `${ORIGIN}/captcha/` });
const always = (r: (route: Route) => Promise<void>): Answer => (route) => r(route);
const checkThen = (r: (route: Route) => Promise<void>): Answer => (route, n) => (n === 1 ? CHECK(route) : r(route));

const form = (page: Page) => page.locator('.moss-subscribe-form').first();
const state = (page: Page) => form(page).getAttribute('data-state');
const shown = (page: Page, selector: string) =>
  form(page).locator(selector).evaluate((el) => getComputedStyle(el).display !== 'none' && getComputedStyle(el).visibility !== 'hidden' && el.getBoundingClientRect().height > 0);

async function submit(page: Page, url = '/'): Promise<void> {
  await page.goto(ORIGIN + url, { waitUntil: 'load' });
  await form(page).locator('input[type=email]').fill('reader@example.com');
  await form(page).locator('button[type=submit]').click();
}

// With scripts off, the form posts natively. The host answers in JSON, so
// the reader lands on that body as a page: subscribed when no check is asked,
// and a bare `need_captcha` refusal when one is.
test('with scripts off the form posts the address itself, and the host answers in JSON', async ({ browser }) => {
  for (const [answer, says] of [[OK, 'confirmationSent'], [CHECK, 'need_captcha']] as const) {
    const context = await browser.newContext({ javaScriptEnabled: false });
    const page = await newPage(context);
    const posts = await serve(context, always(answer));
    await submit(page);
    await expect(page.locator('body')).toContainText(says);
    expect(posts).toEqual([expect.objectContaining({ email: 'reader@example.com' })]);
    await context.close();
  }
});

// An in-app browser that will not load the check's frame: after the quiet
// wait the form reports it, and the host lets the next request through.
test('a check frame the browser blocks still lets the reader subscribe', async ({ page, context }) => {
  test.setTimeout(60_000);
  const posts = await serve(context, checkThen(OK), 'blocked');
  await submit(page);
  await expect(page.locator('[role="dialog"]')).toBeHidden();
  expect(await state(page)).toBe('loading');
  await expect.poll(() => state(page), { timeout: 30_000 }).toBe('success');
  expect(reports).toEqual(['reason=frame']);
  expect(posts).toHaveLength(2);
});

for (const scheme of ['light', 'dark'] as const) {
  for (const width of [390, 1280]) {
    test.describe(`${scheme} at ${width}px`, () => {
      test.use({ viewport: { width, height: 900 }, colorScheme: scheme });
      const shot = (page: Page, name: string) => page.screenshot({ path: `target/test-tmp/subscribe-bodyless-${name}-${test.info().project.name}-${scheme}-${width}.png`, fullPage: true });

      test('idle: one email field with its placeholder and the button', async ({ page, context }) => {
        await serve(context, always(OK));
        await page.goto(ORIGIN + '/', { waitUntil: 'load' });
        await expect(form(page).locator('input[type=email]')).toHaveAttribute('placeholder', 'your@email.com');
        await expect(form(page).locator('.moss-btn__label')).toHaveText('Subscribe');
        await shot(page, 'idle');
      });

      test('the button spins while the post waits, then shows the check and "check your email"', async ({ page, context }) => {
        let release!: () => void;
        const held = new Promise<void>((r) => (release = r));
        const posts = await serve(context, async (route) => {
          await held;
          await OK(route);
        });
        await submit(page);
        await expect.poll(() => state(page)).toBe('loading');
        expect(await shown(page, '.moss-btn__spinner')).toBe(true);
        await shot(page, 'sending');
        release();
        await expect.poll(() => state(page)).toBe('success');
        await expect(form(page).locator('[data-copy="check-email"]')).toBeVisible();
        expect(await shown(page, '.moss-btn__check')).toBe(true);
        expect(posts).toEqual([expect.objectContaining({ email: 'reader@example.com', lang: 'en' })]);
        expect(posts[0].answers).toBeUndefined();
        await shot(page, 'ok');
      });

      test('a refused post shows the error line and keeps the address', async ({ page, context }) => {
        await serve(context, always(json(500, { error: 'down' })));
        await submit(page);
        await expect.poll(() => state(page)).toBe('error');
        await expect(form(page).locator('[data-copy="error"]')).toHaveText('Something went wrong. Try again?');
        await expect(form(page).locator('input[type=email]')).toHaveValue('reader@example.com');
        await shot(page, 'fail');
      });

      test('a 2xx that does not say subscribed is an error, not a subscription', async ({ page, context }) => {
        await serve(context, always(json(200, {})));
        await submit(page);
        await expect.poll(() => state(page)).toBe('error');
      });

      test('the bot check opens while the button spins; passing it posts again and subscribes', async ({ page, context }) => {
        const posts = await serve(context, checkThen(OK), [{ moss_captcha: 'interactive' }, { moss_captcha: 'passed' }]);
        await submit(page);
        await expect(page.locator('[role="dialog"]')).toBeVisible();
        expect(await state(page)).toBe('loading');
        await shot(page, 'captcha');
        await expect.poll(() => state(page)).toBe('success');
        await expect(page.locator('[role="dialog"]')).toHaveCount(0);
        expect(posts).toHaveLength(2);
      });

      test('passing the check and then being refused shows the error line', async ({ page, context }) => {
        await serve(context, checkThen(json(500, { error: 'down' })), [{ moss_captcha: 'passed' }]);
        await submit(page);
        await expect.poll(() => state(page)).toBe('error');
      });

      test('a check that cannot finish is reported, and the reader goes through', async ({ page, context }) => {
        const posts = await serve(context, checkThen(OK), [{ moss_captcha: 'interactive' }, { moss_captcha: 'failed' }]);
        await submit(page);
        await expect.poll(() => state(page)).toBe('success');
        await expect(page.locator('[role="dialog"]')).toHaveCount(0);
        expect(reports).toEqual(['reason=frame']);
        expect(posts).toHaveLength(2);
      });

      test('closing the check by a click beside it brings the form back as it was, with nothing more sent', async ({ page, context }) => {
        const posts = await serve(context, always(CHECK), [{ moss_captcha: 'interactive' }]);
        await submit(page);
        await expect(page.locator('[role="dialog"]')).toBeVisible();
        await page.mouse.click(4, 4);
        await expect.poll(() => state(page)).toBe('idle');
        await expect(form(page).locator('input[type=email]')).toHaveValue('reader@example.com');
        expect(posts).toHaveLength(1);
      });

      test('a members page leads the form with who the page is for, and that line names the field', async ({ page, context }) => {
        await serve(context, always(OK));
        await page.goto(ORIGIN + '/letters/', { waitUntil: 'load' });
        const field = form(page).locator('input[type=email]');
        const lead = await field.getAttribute('aria-labelledby');
        expect(lead).toBeTruthy();
        const leadId = lead!.split(' ')[0];
        await expect(page.locator(`[id="${leadId}"]`)).toHaveText('Subscribers only.');
        const [leadBox, formBox] = await Promise.all([page.locator(`[id="${leadId}"]`).boundingBox(), form(page).boundingBox()]);
        expect(leadBox!.y).toBeLessThan(formBox!.y);
        await shot(page, 'members');
      });

      test("a German page's form speaks German and posts its page's scope and language", async ({ page, context }) => {
        const posts = await serve(context, always(OK));
        await page.goto(ORIGIN + '/de/', { waitUntil: 'load' });
        await expect(form(page).locator('.moss-btn__label')).toHaveText('Abonnieren');
        await expect(form(page).locator('input[type=email]')).toHaveAttribute('placeholder', 'ihre@email.de');
        const scope = await form(page).locator('input[name=scope]').getAttribute('value');
        await form(page).locator('input[type=email]').fill('reader@example.com');
        await form(page).locator('button[type=submit]').click();
        await expect.poll(() => state(page)).toBe('success');
        await expect(form(page).locator('[data-copy="check-email"]')).toHaveText('Bitte prüfen Sie Ihre E-Mail.');
        expect(posts[0]).toMatchObject({ lang: 'de', scope });
        await shot(page, 'de');
      });
    });
  }
}
