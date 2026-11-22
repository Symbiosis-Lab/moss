/**
 * Playwright's `test`, with a bound on opening a page. Specs import `test`
 * (and anything else from `@playwright/test`) from here.
 *
 * On macOS, a Playwright WebKit sometimes never finishes opening a page: the
 * new web process waits on a network-process connection that never comes, so
 * `newPage` neither resolves nor fails and the test sits in its fixture setup
 * until its timeout. Most stalls hit a browser's first page, and every later
 * page in that browser stalls too; a fresh browser usually opens at once.
 *
 * - `browser` proves one page in a throwaway context when a worker starts, and
 *   relaunches (up to RELAUNCHES times) if that page does not open in time.
 * - `page` gives each test's page the same bound, tries once more in the same
 *   context (which recovers a browser that had opened pages before), and then
 *   fails at once with "newPage stalled" instead of at the test's timeout.
 *
 * Every stall and relaunch is written to stderr with the `[newpage-stall]`
 * prefix. MOSS_PW_NO_RELAUNCH=1 keeps the bound but never relaunches, for a run
 * whose green must not depend on a relaunch. `wedgeNextBrowser()` and
 * `injectStalls(n)` fake both kinds of stall for the guard's own spec
 * (playwright/newpage-stall.config.ts).
 */
import { test as base, type Browser, type BrowserContext, type Page } from '@playwright/test';

export * from '@playwright/test';

const PROOF_MS = 10_000;
const PAGE_MS = 15_000;
const CLOSE_MS = 8_000;
const RELAUNCHES = 2;

const noRelaunch = process.env.MOSS_PW_NO_RELAUNCH === '1';
let injected = 0;
let wedged: Browser | 'next' | undefined;

/** Every page open stalls in the next browser that opens one, as in a real stall. */
export const wedgeNextBrowser = () => { wedged = 'next'; };
/** The next `n` page opens stall, in whatever browser. */
export const injectStalls = (n: number) => { injected = n; };

const note = (msg: string) => process.stderr.write(`[newpage-stall] ${msg}\n`);

const within = <T>(work: Promise<T>, ms: number): Promise<T | undefined> =>
  Promise.race([work, new Promise<undefined>((r) => setTimeout(r, ms))]);

/** The context's next page, or undefined if it does not open within `ms`. */
async function openPage(context: BrowserContext, ms: number): Promise<Page | undefined> {
  if (wedged === 'next') wedged = context.browser() ?? undefined;
  if (injected > 0 || (wedged && wedged === context.browser())) {
    injected = Math.max(0, injected - 1);
    return within(new Promise<never>(() => {}), ms);
  }
  return within(context.newPage(), ms);
}

/** Whether `browser` opens a page in a throwaway context. A stalled browser takes about 5 s to close, so its close is bounded too. */
async function opensPage(browser: Browser): Promise<boolean> {
  const context = await browser.newContext();
  const ok = (await openPage(context, PROOF_MS)) !== undefined;
  await within(ok ? context.close() : browser.close(), CLOSE_MS);
  return ok;
}

export const test = base.extend<{}, {}>({
  browser: [async ({ browser, playwright, browserName, launchOptions, headless, channel }, use) => {
    let current = browser;
    for (let attempt = 0; !(await opensPage(current)); attempt++) {
      if (noRelaunch || attempt === RELAUNCHES) throw new Error(`${browserName} newPage stalled in a fresh browser (${attempt + 1} tries, ${PROOF_MS} ms each)`);
      note(`${browserName}: a new browser's first page did not open in ${PROOF_MS} ms; relaunching (${attempt + 1} of ${RELAUNCHES})`);
      current = await playwright[browserName].launch({ ...launchOptions, headless, channel });
    }
    await use(current);
    if (current !== browser) await within(current.close(), CLOSE_MS);
  }, { scope: 'worker', timeout: 120_000 }],

  page: async ({ context, browserName }, use) => {
    let page = await openPage(context, PAGE_MS);
    if (!page) {
      note(`${browserName}: newPage did not open in ${PAGE_MS} ms; trying once more`);
      page = await openPage(context, PAGE_MS);
    }
    if (!page) throw new Error(`${browserName} newPage stalled (2 tries, ${PAGE_MS} ms each)`);
    await use(page);
  },
});
