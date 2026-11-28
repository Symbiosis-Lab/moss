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
 * - `newPage(context)` or `newPage(browser, options)`, which the `page` fixture
 *   uses too, bounds a page open, tries once more (which recovers a browser that
 *   had opened pages before), and then fails at once with "newPage stalled"
 *   instead of at the test's timeout. A spec opening its own pages calls it in
 *   place of `.newPage()`.
 *
 * Every stall, retry and relaunch is written to stderr with the
 * `[newpage-stall]` prefix. MOSS_PW_NO_RECOVERY=1 keeps the bounds but never
 * retries or relaunches, for a run whose green must not depend on either. `wedgeNextBrowser()` and
 * `injectStalls(n)` fake both kinds of stall for the guard's own spec
 * (playwright/newpage-stall.config.ts).
 */
import { test as base, type Browser, type BrowserContext, type BrowserContextOptions, type Page } from '@playwright/test';

export * from '@playwright/test';

const PROOF_MS = 10_000;
const PAGE_MS = 15_000;
const CLOSE_MS = 8_000;
const RELAUNCHES = 2;

const noRecovery = process.env.MOSS_PW_NO_RECOVERY === '1';
let injected = 0;
let wedged: Browser | 'next' | undefined;

/** Every page open stalls in the next browser that opens one, as in a real stall. */
export const wedgeNextBrowser = () => { wedged = 'next'; };
/** The next `n` page opens stall, in whatever browser. */
export const injectStalls = (n: number) => { injected = n; };

const note = (msg: string) => process.stderr.write(`[newpage-stall] ${msg}\n`);

const within = <T>(work: Promise<T>, ms: number): Promise<T | undefined> =>
  Promise.race([work, new Promise<undefined>((r) => setTimeout(r, ms))]);

const browserOf = (from: Browser | BrowserContext) => ('pages' in from ? from.browser() : from);

/** A page from `from`, or undefined if it does not open within `ms`. */
function tryOpen(from: Browser | BrowserContext, options: BrowserContextOptions | undefined, ms: number): Promise<Page | undefined> {
  const browser = browserOf(from);
  if (wedged === 'next') wedged = browser ?? undefined;
  if (injected > 0 || (wedged && wedged === browser)) {
    injected = Math.max(0, injected - 1);
    return within(new Promise<never>(() => {}), ms);
  }
  return within('pages' in from ? from.newPage() : from.newPage(options), ms);
}

/** A new page from a context, or from a browser with these context options, bounded as the file header says. */
export async function newPage(from: Browser | BrowserContext, options?: BrowserContextOptions): Promise<Page> {
  const engine = browserOf(from)?.browserType().name() ?? 'browser';
  let page = await tryOpen(from, options, PAGE_MS);
  if (!page && !noRecovery) {
    note(`${engine}: newPage did not open in ${PAGE_MS} ms; trying once more`);
    page = await tryOpen(from, options, PAGE_MS);
  }
  if (!page) throw new Error(`${engine} newPage stalled (${noRecovery ? 1 : 2} tries, ${PAGE_MS} ms each)`);
  return page;
}

/** Whether `browser` opens a page in a throwaway context. A stalled browser takes about 5 s to close, so its close is bounded too. */
async function opensPage(browser: Browser): Promise<boolean> {
  const context = await browser.newContext();
  const ok = (await tryOpen(context, undefined, PROOF_MS)) !== undefined;
  await within(ok ? context.close() : browser.close(), CLOSE_MS);
  return ok;
}

export const test = base.extend<{}, {}>({
  browser: [async ({ browser, playwright, browserName, launchOptions, headless, channel }, use) => {
    let current = browser;
    for (let attempt = 0; !(await opensPage(current)); attempt++) {
      if (noRecovery || attempt === RELAUNCHES) throw new Error(`${browserName} newPage stalled in a fresh browser (${attempt + 1} tries, ${PROOF_MS} ms each)`);
      note(`${browserName}: a new browser's first page did not open in ${PROOF_MS} ms; relaunching (${attempt + 1} of ${RELAUNCHES})`);
      current = await playwright[browserName].launch({ ...launchOptions, headless, channel });
    }
    await use(current);
    if (current !== browser) await within(current.close(), CLOSE_MS);
  }, { scope: 'worker', timeout: 120_000 }],

  page: async ({ context }, use) => {
    await use(await newPage(context));
  },
});
