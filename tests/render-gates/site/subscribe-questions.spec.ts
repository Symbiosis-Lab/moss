// A `:::subscribe` that asks questions: its fields stack inside the form with
// no sideways scroll; sending it with required answers missing marks them and
// lists them under the button without posting; a complete form posts its
// answers beside the address, spins while it waits, then shows "check your
// email" with the address, or the error line, or the host's bot check; a
// signed-in reader sees "Subscribed" alone. What each edition's form posts is
// what the build's _moss/commerce.json tells the service to expect.
import { test, expect, type BrowserContext, type Page, type Route } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { buildScratchSite } from '../../e2e/helpers/scratch-site';
import { SUBSCRIBE_QUESTIONS_GATE } from '../../e2e/helpers/subscribe-questions-site';

const root = buildScratchSite(SUBSCRIBE_QUESTIONS_GATE);
const ORIGIN = 'https://notes.test';
const TYPES: Record<string, string> = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json', '.svg': 'image/svg+xml' };

type Field = { name: string; type: string; required: boolean; options?: string[] };
type Post = { email: string; scope: string; answers?: Record<string, string | string[]> };

/** `subscribe.fields` of one edition, as the build published it for the service. */
function published(scope: string): Field[] {
  const commerce = JSON.parse(fs.readFileSync(path.join(root, '_moss', 'commerce.json'), 'utf8'));
  const fields = commerce.subscribe?.fields?.[scope];
  if (!fields) throw new Error(`commerce.json publishes no subscribe.fields for scope "${scope}": ${JSON.stringify(commerce.subscribe)}`);
  return fields;
}

/** The post carries only keys the service will accept for its scope, every required one among them. */
function expectAccepted(post: Post): void {
  const fields = published(post.scope);
  const names = fields.map((f) => f.name);
  const keys = Object.keys(post.answers ?? {});
  expect(names).toEqual(expect.arrayContaining(keys));
  expect(keys).toEqual(expect.arrayContaining(fields.filter((f) => f.required).map((f) => f.name)));
  for (const f of fields) {
    const v = post.answers?.[f.name];
    if (v !== undefined && f.options && f.type !== 'text') for (const pick of [v].flat()) expect(f.options).toContain(pick);
  }
}

/** Serves the built site at ORIGIN; the subscribe POST goes to `answer`, and the session question is answered `session`. Returns the POST bodies seen. */
async function serve(context: BrowserContext, answer: (route: Route) => Promise<void>, session: 200 | 401 = 401): Promise<Post[]> {
  const posts: Post[] = [];
  await context.route('**/*', async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    if (req.method() === 'POST' && url.pathname.endsWith('/subscribe')) {
      posts.push(req.postDataJSON());
      return answer(route);
    }
    if (url.origin !== ORIGIN) return route.abort();
    // The modal shows only once the host's check page says it needs the visitor.
    if (url.pathname === '/captcha/') {
      return route.fulfill({ status: 200, headers: { 'content-type': 'text/html' }, body: '<script>parent.postMessage({ moss_captcha: "interactive" }, "*")</script>' });
    }
    if (url.pathname === '/_moss/account/session') {
      return route.fulfill({ status: session, headers: { 'content-type': 'application/json' }, body: JSON.stringify({ signed_in: session === 200 }) });
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

async function fill(page: Page): Promise<void> {
  const form = page.locator('.moss-subscribe-form');
  await form.locator('input[type=email]').fill('reader@example.com');
  await form.getByLabel('Your name').fill('Ana');
  await form.getByLabel('A friend').check();
  await form.getByLabel('Other').check();
  await form.getByLabel('Where else').fill('A podcast');
}

const state = (page: Page) => page.locator('.moss-subscribe-form').getAttribute('data-state');

for (const scheme of ['light', 'dark'] as const) {
  for (const width of [390, 1280]) {
    test.describe(`${scheme} at ${width}px`, () => {
      test.use({ viewport: { width, height: 900 }, colorScheme: scheme });

      test('fields stack inside the form and the page does not scroll sideways', async ({ page, context }) => {
        await serve(context, json(200, {}));
        await page.goto(`${ORIGIN}/`, { waitUntil: 'load' });
        const m = await page.evaluate(() => {
          const form = document.querySelector('.moss-subscribe-form') as HTMLElement;
          const fr = form.getBoundingClientRect();
          const tops = [...form.querySelectorAll('.moss-form__field')].map((f) => f.getBoundingClientRect().top);
          const controls = [...form.querySelectorAll('input:not([type=hidden]), textarea')].map((c) => c.getBoundingClientRect());
          return {
            scroll: document.documentElement.scrollWidth - document.documentElement.clientWidth,
            tops,
            outside: controls.filter((c) => c.width && (c.left < fr.left - 0.5 || c.right > fr.right + 0.5)).length,
          };
        });
        expect(m.scroll).toBeLessThanOrEqual(0);
        expect(m.tops).toEqual([...m.tops].sort((a, b) => a - b));
        expect(m.outside).toBe(0);
      });

      test('missing required answers are marked and listed, and nothing is sent', async ({ page, context }) => {
        const posts = await serve(context, json(200, {}));
        await page.goto(`${ORIGIN}/`, { waitUntil: 'load' });
        await page.locator('.moss-subscribe-form input[type=email]').fill('reader@example.com');
        await page.locator('.moss-subscribe-form button[type=submit]').click();
        await expect(page.locator('.moss-form__status a')).toHaveCount(2);
        await expect(page.locator('.moss-subscribe-form [aria-invalid="true"]').first()).toBeVisible();
        await expect(page.getByLabel('Your name')).toBeFocused();
        expect(posts).toHaveLength(0);
        await page.screenshot({ path: `target/test-tmp/subscribe-questions-errors-${scheme}-${width}.png`, fullPage: true });
      });

      test('a complete form posts its answers, spins while it waits, then shows the address to check', async ({ page, context }) => {
        let release!: () => void;
        const held = new Promise<void>((r) => (release = r));
        const posts = await serve(context, async (route) => {
          await held;
          await json(200, { subscribed: true, confirmationSent: true })(route);
        });
        await page.goto(`${ORIGIN}/`, { waitUntil: 'load' });
        await fill(page);
        await page.locator('.moss-subscribe-form button[type=submit]').click();
        await expect.poll(() => state(page)).toBe('loading');
        await page.screenshot({ path: `target/test-tmp/subscribe-questions-sending-${scheme}-${width}.png`, fullPage: true });
        release();
        await expect.poll(() => state(page)).toBe('success');
        await expect(page.locator('.moss-subscribe-status__to')).toHaveText('reader@example.com');
        expect(posts).toEqual([expect.objectContaining({
          email: 'reader@example.com',
          answers: { your_name: 'Ana', where_did_you_hear_about_us: ['A friend', 'Other'], where_did_you_hear_about_us_other: 'A podcast' },
        })]);
        expectAccepted(posts[0]);
        await page.screenshot({ path: `target/test-tmp/subscribe-questions-ok-${scheme}-${width}.png`, fullPage: true });
      });

      test('a refused send shows the error line and keeps the answers', async ({ page, context }) => {
        await serve(context, json(500, { error: 'down' }));
        await page.goto(`${ORIGIN}/`, { waitUntil: 'load' });
        await fill(page);
        await page.locator('.moss-subscribe-form button[type=submit]').click();
        await expect.poll(() => state(page)).toBe('error');
        await expect(page.getByLabel('Your name')).toHaveValue('Ana');
        await page.screenshot({ path: `target/test-tmp/subscribe-questions-fail-${scheme}-${width}.png`, fullPage: true });
      });

      test("the English edition's form posts its own scope and the questions published for it", async ({ page, context }) => {
        const posts = await serve(context, json(200, { subscribed: true, confirmationSent: true }));
        await page.goto(`${ORIGIN}/en/`, { waitUntil: 'load' });
        const form = page.locator('.moss-subscribe-form');
        await form.locator('input[type=email]').fill('reader@example.com');
        await form.getByLabel('Your name').fill('Ana');
        await form.getByLabel('Events').check();
        await form.locator('button[type=submit]').click();
        await expect.poll(() => state(page)).toBe('success');
        expect(posts).toHaveLength(1);
        expect(posts[0].scope).toBe('en');
        expect(posts[0].answers).toEqual({ your_name: 'Ana', topics: ['Events'] });
        expectAccepted(posts[0]);
      });

      test('a signed-in reader sees "Subscribed" and none of the questions', async ({ page, context }) => {
        await context.addCookies([{ name: '__Host-moss_subscribed', value: '1', domain: 'notes.test', path: '/', secure: true, sameSite: 'Lax' }]);
        await serve(context, json(200, {}), 200);
        await page.goto(`${ORIGIN}/`, { waitUntil: 'load' });
        await expect(page.locator('.moss-subscribe-subscribed')).toBeVisible();
        expect(await page.locator('.moss-subscribe-form .moss-form__field').count()).toBeGreaterThan(1);
        for (const field of await page.locator('.moss-subscribe-form .moss-form__field').all()) await expect(field).toBeHidden();
        await expect(page.locator('.moss-subscribe-form button[type=submit]')).toBeHidden();
        await page.screenshot({ path: `target/test-tmp/subscribe-questions-subscribed-${scheme}-${width}.png`, fullPage: true });
      });

      test('the host asking for its bot check opens it', async ({ page, context }) => {
        await serve(context, json(403, { need_captcha: true, captcha_url: `${ORIGIN}/captcha/` }));
        await page.goto(`${ORIGIN}/`, { waitUntil: 'load' });
        await fill(page);
        await page.locator('.moss-subscribe-form button[type=submit]').click();
        await expect(page.locator('[role="dialog"]')).toBeVisible();
        await page.screenshot({ path: `target/test-tmp/subscribe-questions-captcha-${scheme}-${width}.png`, fullPage: true });
      });
    });
  }
}
