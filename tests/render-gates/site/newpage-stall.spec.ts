/**
 * The page-open guard (tests/support/browser-test.ts) with stalls injected:
 * the worker's first browser opens no page at all, so it must be relaunched;
 * then a test's own page stalls once, so it is tried again in the same context.
 */
import { test, expect, injectStalls, wedgeNextBrowser } from '../../support/browser-test';

test.describe.configure({ mode: 'serial' });

test.beforeAll(() => wedgeNextBrowser());

test('a browser that opens no page is relaunched', async ({ page }) => {
  await page.setContent('<p>opened</p>');
  await expect(page.locator('p')).toHaveText('opened');
  injectStalls(1);
});

test('a page that stalls once is opened on the second try', async ({ page }) => {
  await page.setContent('<p>opened</p>');
  await expect(page.locator('p')).toHaveText('opened');
});
