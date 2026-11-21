/**
 * The preview's progress line from a save to rest, in the browser shell
 * `moss edit --serve` serves: every change of its text is recorded, an empty
 * or missing line included, and each case must read "Updating…" up to the
 * paint, then "Updated", with nothing between and never left on "Updating…".
 *
 * Cases: a save with search on, a save that adds a video, a second save
 * inside 2.5 s of the first, a save no paint follows and then one whose build
 * spans the bar's 1.5 s fallback, a save whose build changes no output (the
 * preview is not reloaded, yet the line still ends on "Updated"), and a save
 * whose rebuild fails, which must end on the failure and not on "Updating…",
 * then the save that fixes it, which ends on "Updated". Each case's record is
 * attached to the test as `lines.json`.
 *
 * Ports: playwright/gate-ports.ts, keys `progress-line:ui` /
 * `progress-line:preview`. Config: playwright/progress-line.config.ts.
 */
import { test, expect, type Frame } from '@playwright/test';
import { copyFileSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { openShell, useEditServe } from '../../e2e/helpers/edit-serve';

const CLIP = resolve(dirname(fileURLToPath(import.meta.url)), '../../../crates/moss-build/tests/fixtures/snapshot-sites/video-embed-site/input/clip.mp4');
const CONFIG = 'schema_version = 4\n\n[site]\nsearch = true\n';

const serve = useEditServe('progress-line', (site) => {
  mkdirSync(join(site, '.moss'), { recursive: true });
  writeFileSync(join(site, '.moss', 'config.toml'), CONFIG);
  writeFileSync(join(site, 'index.md'), '---\ntitle: Garden\n---\nHello garden.\n\n[Beds](beds.md)\n');
  writeFileSync(join(site, 'beds.md'), '---\ntitle: Beds\n---\nBeds are dug in March.\n');
}, { MOSS_LOG_LEVEL: 'info' });

type Line = { t: number; text: string | null };

/** Starts recording the line in the shell; every distinct value, with its time. */
async function record(shell: Frame): Promise<void> {
  await shell.evaluate(() => {
    const w = window as unknown as { __lines: Line[] };
    w.__lines = [];
    const read = () => document.querySelector('.moss-status-line')?.textContent ?? null;
    const push = () => {
      const text = read();
      const last = w.__lines.at(-1);
      if (!last || last.text !== text) w.__lines.push({ t: Math.round(performance.now()), text });
    };
    push();
    new MutationObserver(push).observe(document.documentElement, { subtree: true, childList: true, characterData: true, attributes: true });
  });
}

/**
 * Opens and pins the panel the way a person does: hover the seam while a
 * save is in flight, then keep the panel open with its handle. The line is
 * drawn only in an open panel; a minimized one shows the hairline alone.
 */
async function pinPanel(shell: Frame, site: string): Promise<void> {
  const hit = shell.locator('.moss-seam-hit:not(.is-hidden)');
  const handle = shell.locator('.moss-surface-handle');
  // The peek closes when the save's build ends, which can beat the pin; save again.
  for (let attempt = 1; ; attempt++) {
    writeFileSync(join(site, 'beds.md'), `---\ntitle: Beds\n---\nBeds are dug in March, by hand (${attempt}).\n`);
    try {
      await hit.waitFor({ timeout: 30_000 });
      await hit.hover({ timeout: 5_000 });
      await handle.click({ timeout: 5_000 });
      await expect(handle).toHaveAttribute('aria-expanded', 'true', { timeout: 2_000 });
      break;
    } catch (e) {
      if (attempt === 3) throw e;
    }
  }
  await shell.page().mouse.move(0, 0); // the pin, not the hover, keeps it open
}

const lines = (shell: Frame) => shell.evaluate(() => (window as unknown as { __lines?: Line[] }).__lines ?? []);

/** Waits until the line has not changed for `quietMs`. */
async function settle(shell: Frame, quietMs = 5000): Promise<Line[]> {
  await expect.poll(async () => {
    const seen = await lines(shell);
    const now = await shell.evaluate(() => Math.round(performance.now()));
    return now - (seen.at(-1)?.t ?? 0);
  }, { timeout: 120_000, intervals: [500] }).toBeGreaterThan(quietMs);
  return lines(shell);
}

/** The record from the first "Updating…": it reaches "Updated" with no blank between, and does not end on "Updating…". */
function expectUpdatingThenUpdated(seen: Line[]): void {
  const from = seen.findIndex((l) => /^Updating/.test(l.text ?? ''));
  expect(from, JSON.stringify(seen)).toBeGreaterThanOrEqual(0);
  const to = seen.findIndex((l, i) => i > from && /^Updated/.test(l.text ?? ''));
  expect(to, JSON.stringify(seen)).toBeGreaterThan(from);
  for (const l of seen.slice(from, to)) expect(l.text ?? '', JSON.stringify(seen)).toMatch(/^Updating/);
  // A re-render the user did not start (media settling) never re-announces itself.
  const said = seen.filter((l) => l.text);
  expect(said.at(-1)!.text, JSON.stringify(seen)).toMatch(/^Updated/);
}

test.describe('progress line', () => {
  // The record and the server's log stay beside each run, pass or fail.
  test.afterEach(async ({}, testInfo) => {
    writeFileSync(testInfo.outputPath('server.log'), serve.output);
    for (const a of testInfo.attachments) if (a.name === 'lines.json' && a.body) writeFileSync(testInfo.outputPath('lines.json'), a.body);
  });

  test('a save with search on reads Updating… then Updated', async ({ page }, testInfo) => {
    const { shell, preview } = await openShell(page, serve, 'Hello garden.');
    await pinPanel(shell, serve.site);
    await record(shell);
    const mark = (await settle(shell)).length - 1;
    writeFileSync(join(serve.site, 'index.md'), '---\ntitle: Garden\n---\nHello garden, in spring.\n\n[Beds](beds.md)\n');
    await preview.waitForFunction(() => document.body?.innerText.includes('in spring'), null, { timeout: 30_000 });
    const seen = (await settle(shell)).slice(mark);
    await testInfo.attach('lines.json', { body: JSON.stringify(seen, null, 1), contentType: 'application/json' });
    expectUpdatingThenUpdated(seen);
  });

  test('a save that adds a video reads Updating… then Updated', async ({ page }, testInfo) => {
    const { shell, preview } = await openShell(page, serve, 'Hello garden.');
    await pinPanel(shell, serve.site);
    await record(shell);
    const mark = (await settle(shell)).length - 1;
    copyFileSync(CLIP, join(serve.site, 'clip.mp4'));
    writeFileSync(join(serve.site, 'index.md'), '---\ntitle: Garden\n---\nHello garden, on film.\n\n![[clip.mp4]]\n\n[Beds](beds.md)\n');
    await preview.waitForFunction(() => document.body?.innerText.includes('on film'), null, { timeout: 30_000 });
    const seen = (await settle(shell)).slice(mark);
    await testInfo.attach('lines.json', { body: JSON.stringify(seen, null, 1), contentType: 'application/json' });
    expectUpdatingThenUpdated(seen);
  });

  test('a second save inside 2.5 s reads Updating… until its own paint, then Updated', async ({ page }, testInfo) => {
    const { shell, preview } = await openShell(page, serve, 'Hello garden.');
    await pinPanel(shell, serve.site);
    await record(shell);
    const mark = (await settle(shell)).length - 1;
    writeFileSync(join(serve.site, 'index.md'), '---\ntitle: Garden\n---\nHello garden, once.\n\n[Beds](beds.md)\n');
    await page.waitForTimeout(1200);
    writeFileSync(join(serve.site, 'index.md'), '---\ntitle: Garden\n---\nHello garden, twice.\n\n[Beds](beds.md)\n');
    await preview.waitForFunction(() => document.body?.innerText.includes('twice'), null, { timeout: 30_000 });
    const seen = (await settle(shell)).slice(mark);
    await testInfo.attach('lines.json', { body: JSON.stringify(seen, null, 1), contentType: 'application/json' });
    expectUpdatingThenUpdated(seen);
  });

  test('a save no paint follows, then a save whose build runs past 1.5 s, reads Updating… to the second paint', async ({ page }, testInfo) => {
    const { shell, preview } = await openShell(page, serve, 'Hello garden.');
    await pinPanel(shell, serve.site);
    await record(shell);
    const mark = (await settle(shell)).length - 1;
    // The preview shows the home page, so a change to another page paints nothing.
    writeFileSync(join(serve.site, 'beds.md'), '---\ntitle: Beds\n---\nBeds are dug in May.\n');
    await page.waitForTimeout(500);
    copyFileSync(CLIP, join(serve.site, 'clip.mp4'));
    writeFileSync(join(serve.site, 'index.md'), '---\ntitle: Garden\n---\nHello garden, on film.\n\n![[clip.mp4]]\n\n[Beds](beds.md)\n');
    await preview.waitForFunction(() => document.body?.innerText.includes('on film'), null, { timeout: 30_000 });
    const seen = (await settle(shell)).slice(mark);
    await testInfo.attach('lines.json', { body: JSON.stringify(seen, null, 1), contentType: 'application/json' });
    expectUpdatingThenUpdated(seen);
  });

  test('a save whose build changes no output still ends on Updated', async ({ page }, testInfo) => {
    const { shell } = await openShell(page, serve, 'Hello garden.');
    await pinPanel(shell, serve.site);
    await record(shell);
    const index = join(serve.site, 'index.md');
    const body = readFileSync(index, 'utf8');
    const suppressed = () => serve.output.split('→ SUPPRESS').length - 1;
    // A trailing blank line changes the source and no page. The first such save
    // still reloads once, for the share cards written after the last build.
    const quietSave = async (extra: string): Promise<Line[]> => {
      const mark = (await settle(shell)).length;
      writeFileSync(index, body + extra);
      // No text appears in the preview to wait on; wait for the line itself.
      await expect.poll(async () => (await lines(shell)).length, { timeout: 30_000 }).toBeGreaterThan(mark);
      return (await settle(shell)).slice(mark - 1);
    };
    await quietSave('\n');
    const before = suppressed();
    const seen = await quietSave('\n\n');
    await testInfo.attach('lines.json', { body: JSON.stringify(seen, null, 1), contentType: 'application/json' });
    expect(suppressed(), 'the measured save took the no-reload path').toBeGreaterThan(before);
    expectUpdatingThenUpdated(seen);
  });

  test('a save whose rebuild fails does not end on Updating…', async ({ page }, testInfo) => {
    const { shell } = await openShell(page, serve, 'Hello garden.');
    await pinPanel(shell, serve.site);
    await record(shell);
    const mark = (await settle(shell)).length - 1;
    const config = join(serve.site, '.moss', 'config.toml');
    const good = readFileSync(config, 'utf8');
    writeFileSync(config, good + '\n[site\n');
    writeFileSync(join(serve.site, 'beds.md'), '---\ntitle: Beds\n---\nBeds are dug in April.\n');
    const seen = (await settle(shell, 8000)).slice(mark);
    await testInfo.attach('lines.json', { body: JSON.stringify(seen, null, 1), contentType: 'application/json' });
    // A config that fails to parse stops the build before its first tick, so the failure may replace the blank line directly.
    const failed = seen.findIndex((l) => /^Couldn't update: /.test(l.text ?? ''));
    expect(failed, JSON.stringify(seen)).toBeGreaterThan(0);
    for (const l of seen.slice(1, failed)) expect(l.text ?? '', JSON.stringify(seen)).toMatch(/^Updating/);
    expect(seen.at(-1)!.text ?? '', JSON.stringify(seen)).toMatch(/^Couldn't update: /);

    // The fix is the next save: the line leaves the failure for "Updated".
    const fixedMark = mark + seen.length;
    writeFileSync(config, good);
    writeFileSync(join(serve.site, 'beds.md'), '---\ntitle: Beds\n---\nBeds are dug in May.\n');
    // The line has been quiet since the failure, so settle alone would return at once.
    await expect.poll(async () => (await lines(shell)).length, { timeout: 30_000 }).toBeGreaterThan(fixedMark);
    const fixed = (await settle(shell, 8000)).slice(fixedMark);
    await testInfo.attach('fixed.json', { body: JSON.stringify(fixed, null, 1), contentType: 'application/json' });
    expectUpdatingThenUpdated(fixed);
  });
});
