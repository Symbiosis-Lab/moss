/**
 * The browser editor `moss edit --serve` serves: the sign-in link opens the
 * shell, the preview frame shows the site, the editor opens on the page, a
 * keystroke reaches the file and the preview follows it, and the "Open on
 * another device" panel names `--public-url` when there is no HTTPS origin.
 *
 * Then a PDF opened from its embed card draws in the file viewer. The source
 * route serves every file sandboxed, so a file in the folder cannot run
 * script on the origin that shows it; this pins that the browser's PDF viewer
 * still draws under that policy (Chromium: its viewer frame loads; WebKit
 * draws in place and exposes no frame, so the response and the viewer's
 * element are what it can check), and that the viewer stays once the preview
 * has followed to the PDF's own address.
 *
 * Then the editor follows the preview back: a page opened from the file tree
 * is shown without entering the preview's back/forward history, and a click
 * from it to the home page must still bring the editor to the home page's file.
 * And a click in the page while the editor shows a file that is no page (the
 * footer) leaves that file alone, while a double click opens the page's own
 * file at the place clicked. In a `:::grid` of the built page, a double click
 * on a column's second line puts the caret on that line, not the column's first.
 * A single click on text in a long page washes that line in the editor and
 * scrolls it into view; in a grid column it picks the click's own line.
 *
 * Then an event's `start` and `end` open the browser's own date and time
 * controls from the page properties, in the narrowest popover and in both
 * themes: leaving a value untouched keeps the file byte for byte however it
 * was written, and a chosen date and time reach the file in the form the
 * build draws.
 *
 * Then two new pages in a row in one folder, `Workshops`: the second is
 * `untitled 2.md`, its address (`workshops/untitled-2/`) spells neither name,
 * and no build has mapped it, so the editor opens it only if the address is
 * matched back to the file.
 *
 * Then a folder's own page that lists its folder by name carries no
 * missing-file mark, while an embed of a folder that does not exist does.
 *
 * Then a new page in a language folder, named with spaces and capitals: the
 * preview shows it at its address before it has any text, in no listing, and
 * after one typed word it is an ordinary page its folder lists.
 *
 * Then the create menu offers a page's version in another language only when
 * it has none: a version linked by `translationKey` under another file name
 * counts.
 *
 * Then a page renamed in the folder while the preview shows it: the preview
 * follows it to the folder's address, never `index.html`, because the frame's
 * address is what Copy page address copies.
 *
 * Ports: playwright/gate-ports.ts, keys `browser-editor:ui` /
 * `browser-editor:preview`. Config: playwright/browser-editor.config.ts.
 */
import { test, expect, type Frame, type Page } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gatePort } from '../../../playwright/gate-ports';
import { MOSS_BIN } from '../../e2e/helpers/scratch-site';

// Event pages, each linking to the next: the forms a person writes by hand,
// one the build rejects, and the empty field an event template leaves.
const EVENTS: Record<string, string> = {
  'talk.md': '---\ntitle: Talk\nstart: 2026-12-01\nend: "2026-12-01 21:00"\n---\nA talk. [Next](walk.md)\n',
  'walk.md': '---\ntitle: Walk\nstart: 2026-12-1\nend: 2026-12-02T10:00\n---\nA walk. [Next](fair.md)\n',
  'fair.md': '---\ntitle: Fair\nstart:\n---\nA fair.\n',
};

const ONE_PAGE_PDF =
  '%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n' +
  '3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n';

const CASE_PAGE = '---\ntitle: Case\n---\nA case.\n\n' +
  [1, 2, 3, 4, 5, 6].map((n) => `![[slide-0${n}.jpg|800x450]]\n\n`).join('') + '![[chart.jpg]]\n\n![](here.png)\n\nThe end.\n';
const ONE_PIXEL_PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==', 'base64');

let site = '';
let home = '';
let server: ChildProcess | undefined;
let output = '';

// One server a test: its sign-in link opens once.
test.beforeEach(async () => {
  output = '';
  site = realpathSync(mkdtempSync(join(tmpdir(), 'moss-browser-editor-site-')));
  home = realpathSync(mkdtempSync(join(tmpdir(), 'moss-browser-editor-home-')));
  mkdirSync(join(home, 'moss'));
  writeFileSync(join(site, 'index.md'), '---\ntitle: Garden\n---\nHello garden.\n\n![[doc.pdf]]\n\n[Talk](talk.md)\n\n[To the workshops](/workshops/)\n\n[To the case](en/writing/case.md)\n\n[About](about.md)\n\n[To the Chinese pages](/zh-hans/)\n\n' +
    ':::grid 2\nBeds are dug in March.\n\nSeeds go in by April.\n+++\nWater comes from the well.\n\nThe well is closed in winter.\n:::\n');
  for (const [name, text] of Object.entries(EVENTS)) writeFileSync(join(site, name), text);
  writeFileSync(join(site, 'doc.pdf'), ONE_PAGE_PDF);
  writeFileSync(join(site, 'footer.md'), 'Tended by hand.\n');
  // Long enough that its last lines start outside both panes.
  writeFileSync(join(site, 'almanac.md'), '---\ntitle: Almanac\n---\n' +
    Array.from({ length: 80 }, (_, i) => `Entry ${i + 1} of the almanac.\n\n`).join('') +
    ':::grid 2\nFrost comes early.\n\nSnow stays late.\n+++\nCranes pass over.\n\nThe pond freezes last.\n:::\n');
  mkdirSync(join(site, 'events'));
  mkdirSync(join(site, 'Workshops'));
  writeFileSync(join(site, 'Workshops', 'index.md'), '---\ntitle: Workshops\n---\nAll workshops.\n');
  writeFileSync(join(site, 'events', 'reading-circle.md'), '---\ntitle: Reading circle\n---\nWe read together.\n\n[Back home](/)\n');
  mkdirSync(join(site, 'meetings'));
  writeFileSync(join(site, 'meetings', 'meetings.md'), '---\ntitle: Meetings\n---\nOurs:\n\n![[meetings/|style:summary]]\n\nGone:\n\n![[nowhere/|style:summary]]\n');
  writeFileSync(join(site, 'meetings', 'first.md'), '---\ntitle: First\n---\nThe first meeting.\n');
  // A page two folders down, seven of whose pictures are not in the folder,
  // and a second page with two.
  mkdirSync(join(site, 'en', 'writing'), { recursive: true });
  mkdirSync(join(site, 'drafts'));
  writeFileSync(join(site, 'en', 'writing', 'case.md'), CASE_PAGE);
  writeFileSync(join(site, 'en', 'writing', 'here.png'), ONE_PIXEL_PNG);
  writeFileSync(join(site, 'en', 'writing', 'other.md'), '---\ntitle: Other\n---\nNothing missing.\n');
  writeFileSync(join(site, 'drafts', 'd.md'), '---\ntitle: D\n---\n![](gone-a.png)\n\n![](pics/gone-b.png)\n');
  mkdirSync(join(site, 'zh-hans'));
  writeFileSync(join(site, 'zh-hans', 'index.md'), '---\ntitle: 花园\n---\n你好，花园。\n');
  writeFileSync(join(site, 'about.md'), '---\ntitle: About\ntranslationKey: about\n---\nAbout the garden. [Talk](talk.md)\n');
  writeFileSync(join(site, 'zh-hans', '关于.md'), '---\ntitle: 关于\ntranslationKey: about\n---\n关于花园。\n');
  // MOSS_HOME and the search root keep the binary from finding, or handing
  // the folder to, an installed desktop app.
  server = spawn(MOSS_BIN, ['edit', site, '--serve', '--no-browser', '--port', String(gatePort('browser-editor:ui'))], {
    env: {
      ...process.env,
      MOSS_HOME: join(home, 'moss'),
      MOSS_DESKTOP_SEARCH_ROOT: home,
      MOSS_PREVIEW_PORT_BASE: String(gatePort('browser-editor:preview')),
      // The new-page test reads the rename's own request from the log.
      MOSS_LOG_LEVEL: 'info',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  server.stdout?.on('data', (b) => (output += b));
  server.stderr?.on('data', (b) => (output += b));
});

test.afterEach(async () => {
  if (server && server.exitCode === null) {
    server.kill('SIGTERM');
    await new Promise((r) => server!.once('exit', r));
  }
  rmSync(site, { recursive: true, force: true });
  rmSync(home, { recursive: true, force: true });
});

/**
 * Open the file tree's list from the root crumb and wait for the panel to
 * finish growing. Its rows move while it grows, so a click made then can press
 * one row and release on another, which the tree takes as a click on no row.
 */
async function openTreeRoot(editor: Frame) {
  await editor.click('.seg.bc[data-path=""]');
  await editor.waitForFunction(() => document.querySelector('.nav-surface')?.getAnimations().length === 0);
}

/** Sign in, wait for the home page in the preview, and open the editor beside it. */
async function openEditor(page: Page) {
  await expect.poll(() => output.match(/Edit at (\S+)/)?.[1], { timeout: 60_000, message: `the sign-in link; server said:\n${output}` }).toBeTruthy();
  const link = output.match(/Edit at (\S+)/)![1];

  await page.goto(link);
  await page.waitForURL((u) => u.pathname === '/', { timeout: 15_000 });
  // Each frame is taken from its element: the editor's URL gains the open page as a query.
  const frameOf = async (id: string) => (await (await page.waitForSelector(`#${id}`, { state: 'attached', timeout: 15_000 })).contentFrame())!;
  const shell = await frameOf('moss-shell-frame');
  const editor = await frameOf('moss-editor-frame');
  const preview = (await (await shell.waitForSelector('#moss-preview-iframe', { state: 'attached', timeout: 15_000 })).contentFrame())!;
  await preview.waitForFunction(() => document.body?.innerText.includes('Hello garden'), null, { timeout: 30_000 });

  await shell.click('#moss-edit-button');
  await expect(page.locator('#moss-frames')).toHaveAttribute('data-editor', 'split', { timeout: 10_000 });
  return { shell, editor, preview };
}

test('the browser editor boots, saves, opens a sandboxed PDF in the file viewer, and follows the preview home', async ({ page, browserName }) => {
  const { shell, editor, preview } = await openEditor(page);
  await editor.waitForSelector('.cm-content', { timeout: 30_000 });
  await editor.click('.cm-line:has-text("Hello garden")');
  await page.keyboard.type(' Typed in a browser.');
  await expect.poll(() => readFileSync(join(site, 'index.md'), 'utf8'), { timeout: 15_000 }).toContain('Typed in a browser.');
  await preview.waitForFunction(() => document.body?.innerText.includes('Typed in a browser'), null, { timeout: 30_000 });

  await shell.click('#moss-device-button');
  await shell.waitForFunction(() => document.querySelector('dialog')?.textContent?.includes('--public-url'), null, { timeout: 5_000 });
  await shell.press('dialog', 'Escape');

  const pdfResponse = page.waitForResponse((r) => r.url().includes('/__moss/source/doc.pdf'), { timeout: 15_000 });
  // The preview follows to the PDF's own address, and the shell asks what backs it.
  const backing = page.waitForResponse(
    (r) => r.url().endsWith('/resolve_page_source') && (r.request().postData() ?? '').includes('doc.pdf'),
    { timeout: 15_000 },
  );
  // Every PDF viewer the editor mounts or removes from here on, read as the test's last step.
  await editor.evaluate(() => {
    const seen = ((window as unknown as { pdfViewers: { mounted: number; removed: number } }).pdfViewers = { mounted: 0, removed: 0 });
    const viewers = (nodes: NodeList): number =>
      [...nodes].filter((n) => n instanceof Element && (n.matches('embed.moss-file-viewer__pdf') || n.querySelector('embed.moss-file-viewer__pdf'))).length;
    new MutationObserver((records) => {
      for (const r of records) {
        seen.mounted += viewers(r.addedNodes);
        seen.removed += viewers(r.removedNodes);
      }
    }).observe(document, { childList: true, subtree: true });
  });
  await editor.click('.cm-media-embed-card', { modifiers: ['ControlOrMeta'] });
  const viewer = await editor.waitForSelector('embed.moss-file-viewer__pdf', { timeout: 15_000 });
  const pdf = await pdfResponse;
  expect(pdf.status()).toBe(200);
  expect((await pdf.allHeaders())['content-security-policy']).toMatch(/^sandbox;/);
  // The answer is the PDF itself, so the editor stays on the viewer it opened:
  // the same element, neither replaced by a pane nor mounted again.
  expect((await (await backing).json()).source_path).toMatch(/doc\.pdf$/);
  // The editor records the answer in its own address as it applies it, and a
  // second load of a file already in its viewer would replace the viewer in
  // the same turn: once the address is there, so is the outcome.
  await expect.poll(() => editor.evaluate(() => location.search), { timeout: 10_000 }).toContain('doc.pdf');
  expect(await viewer.evaluate((el) => el.isConnected), 'the viewer the click opened is still on screen').toBe(true);
  if (browserName === 'chromium') {
    // The built page embeds the same PDF, so the preview has a viewer frame of
    // its own; only one under the editor's embed counts.
    const underTheEmbed = (f: Frame | null): boolean => !!f && (f.url().includes('/__moss/source/doc.pdf') || underTheEmbed(f.parentFrame()));
    await expect
      .poll(() => page.frames().some((f) => f.url().startsWith('chrome-extension://') && f.url().endsWith('/index.html') && underTheEmbed(f)), {
        timeout: 15_000,
        message: "the PDF viewer frame under the editor's embed",
      })
      .toBe(true);
  }
  expect(
    await editor.evaluate(() => (window as unknown as { pdfViewers: unknown }).pdfViewers),
    'the editor mounted one PDF viewer and never took it down',
  ).toEqual({ mounted: 1, removed: 0 });

  await test.step('the editor follows the preview home from a page opened in the file tree', async () => {
    const openFile = () => editor.evaluate(() => decodeURIComponent(location.search));
    await openTreeRoot(editor);
    await editor.click('.seg[data-path="events"]:not(.bc)');
    await editor.click('.seg[data-path="events/reading-circle.md"]');
    await expect.poll(openFile, { timeout: 15_000 }).toContain('reading-circle.md');
    await preview.waitForFunction(() => document.body?.innerText.includes('We read together'), null, { timeout: 30_000 });
    await preview.click('a:has-text("Back home")');
    await preview.waitForFunction(() => document.body?.innerText.includes('Hello garden'), null, { timeout: 30_000 });
    await expect.poll(openFile, { timeout: 15_000, message: 'the file the editor shows once the preview is home' }).toContain('index.md');
    await expect(editor.locator('.cm-content')).toContainText('Hello garden');
  });

  const caretLine = () => editor.evaluate(() => {
    const at = getSelection()?.anchorNode;
    return (at instanceof Element ? at : at?.parentElement)?.closest('.cm-line')?.textContent ?? '';
  });

  await test.step('a double click in the page brings the editor back from another file', async () => {
    // The footer is no page of its own: the preview stays on the home page.
    // The footer asks before it opens; answered "don't ask again" here.
    await editor.evaluate(() => localStorage.setItem('moss.specialFile.skipConfirm.footer', '1'));
    // The root crumb toggles the list: open it only when it is closed.
    if ((await editor.locator('.seg[data-path="footer.md"]').count()) === 0) await openTreeRoot(editor);
    await editor.click('.seg[data-path="footer.md"]');
    // The editor's own address names a file only when the preview follows to it, so the text is read.
    await expect(editor.locator('.cm-content')).toContainText('Tended by hand', { timeout: 15_000 });
    await page.waitForTimeout(500); // the settle after the footer reveal

    // One click points at nothing in the footer's text.
    await preview.click('p:has-text("Hello garden")');
    await page.waitForTimeout(700);
    expect(await editor.locator('.cm-reveal-flash').count(), 'lines washed in the footer file').toBe(0);
    await expect(editor.locator('.cm-content')).toContainText('Tended by hand');

    await preview.dblclick('p:has-text("Hello garden")');
    await expect(editor.locator('.cm-content'), 'the file the editor shows after the double click').toContainText('Hello garden', { timeout: 15_000 });
    await expect.poll(caretLine, { timeout: 15_000, message: 'the line the caret is on' }).toContain('Hello garden');
  });

  await test.step("a double click on a grid column's second line puts the caret on that line", async () => {
    // The build marks a column with its range and each line in it with its own line.
    const secondLine = preview.locator('.moss-grid-card[data-source-range]').nth(1).locator('p[data-source-line]').nth(1);
    await expect(secondLine).toHaveText('The well is closed in winter.');
    await page.waitForTimeout(500); // page clicks wait while the editor settles on the file the last step loaded
    await secondLine.dblclick();
    await expect.poll(caretLine, { timeout: 15_000, message: "the line the caret is on in the grid's second column" }).toContain('The well is closed in winter');
  });

  await test.step('a second new page in a folder opens in the editor', async () => {
    const openFile = () => editor.evaluate(() => decodeURIComponent(location.search));
    // Reached as a visitor would: the editor follows the preview into the folder.
    await preview.click('a:has-text("To the workshops")');
    await expect.poll(openFile, { timeout: 15_000 }).toContain('Workshops/index.md');
    await editor.hover('#editor-create-btn');
    await expect(editor.locator('#editor-create-btn')).toHaveAttribute('data-tooltip', /Workshops/);
    await editor.click('#editor-create-btn');
    await expect.poll(openFile, { timeout: 15_000 }).toContain('Workshops/untitled.md');
    // The preview follows to the new page's address and the shell asks what backs it.
    const backing = page.waitForResponse(
      (r) => r.url().endsWith('/resolve_page_source') && (r.request().postData() ?? '').includes('workshops/untitled-2'),
      { timeout: 30_000 },
    );
    await editor.click('#editor-create-btn');
    await expect.poll(openFile, { timeout: 15_000 }).toContain('Workshops/untitled 2.md');
    expect((await (await backing).json()).source_path, 'the file behind the new address').toMatch(/Workshops\/untitled 2\.md$/);
    await expect(editor.locator('body')).not.toContainText('No page to edit');
    await editor.click('.cm-content');
    await page.keyboard.type('Second new page.');
    await expect.poll(() => readFileSync(join(site, 'Workshops', 'untitled 2.md'), 'utf8'), { timeout: 15_000 }).toContain('Second new page.');
  });
});

test('a click on text in the page washes its line in the editor and brings it into view', async ({ page }) => {
  const { editor, preview } = await openEditor(page);
  await editor.waitForSelector('.cm-content', { timeout: 30_000 });
  if ((await editor.locator('.seg[data-path="almanac.md"]').count()) === 0) await openTreeRoot(editor);
  await editor.click('.seg[data-path="almanac.md"]');
  await preview.waitForFunction(() => document.body?.innerText.includes('Entry 1 of the almanac'), null, { timeout: 30_000 });
  await expect(editor.locator('.cm-content')).toContainText('Entry 1 of the almanac', { timeout: 15_000 });
  await page.waitForTimeout(700); // outlasts the longest hold in editor-link.ts SETTLE_MS, during which page clicks are ignored

  // Every washed line, and whether the editor's own scroll area shows all of it.
  const washed = () => editor.evaluate(() =>
    [...document.querySelectorAll('.cm-reveal-flash')].map((line) => {
      const at = line.getBoundingClientRect();
      const area = line.closest('.cm-scroller')!.getBoundingClientRect();
      return { text: line.textContent, shown: at.top >= area.top && at.bottom <= area.bottom };
    }));

  // The editor draws only the lines near its scroll position.
  expect(await editor.locator('.cm-line:has-text("Entry 70 of the almanac.")').count(), 'the far line before the click').toBe(0);
  await preview.locator('p:has-text("Entry 70 of the almanac.")').click();
  await expect.poll(washed, { timeout: 10_000, message: 'the washed lines after a click on a far paragraph' })
    .toEqual([{ text: 'Entry 70 of the almanac.', shown: true }]);

  await preview.locator('p:has-text("Entry 3 of the almanac.")').click();
  await expect.poll(washed, { timeout: 10_000, message: 'the washed lines after a click back near the top' })
    .toEqual([{ text: 'Entry 3 of the almanac.', shown: true }]);

  // Inside a grid column the click picks its own line, not the column.
  await preview.locator('.moss-grid-card p:has-text("The pond freezes last.")').click();
  await expect.poll(washed, { timeout: 10_000, message: 'the washed lines after a click inside a grid column' })
    .toEqual([{ text: 'The pond freezes last.', shown: true }]);
});

test("an event's start and end open date and time controls that write what the build reads", async ({ page }, testInfo) => {
  const { editor, preview } = await openEditor(page);
  const pop = editor.locator('.pop');
  const date = pop.locator('input[type="date"]');
  const time = pop.locator('input[type="time"]');
  const chip = (key: string) => editor.locator(`.chip-bar .chip[data-key="${key}"]`);
  const shown = async () => [await date.inputValue(), await time.inputValue()];
  const goTo = async (link: string, body: string) => {
    await preview.click(`a:has-text("${link}")`);
    await editor.waitForSelector(`.cm-line:has-text("${body}")`, { timeout: 30_000 });
  };
  const open = async (key: string) => {
    await chip(key).click();
    await expect(date).toBeFocused();
    await expect(time).toHaveAttribute('aria-label', 'Time');
  };
  const leaves: Record<string, () => Promise<unknown>> = {
    Enter: () => page.keyboard.press('Enter'),
    Escape: () => page.keyboard.press('Escape'),
    'a click outside': () => editor.click('.cm-content'),
  };

  // Every written form opens as the build reads it, and leaving it alone, by
  // any door, leaves the file as it was.
  const untouched = async (name: string, values: Record<string, string[]>) => {
    for (const [key, value] of Object.entries(values)) {
      for (const [how, leave] of Object.entries(leaves)) {
        await open(key);
        expect(await shown(), `${name} ${key}`).toEqual(value);
        await leave();
        await expect(pop, `${name} ${key} closes by ${how}`).toHaveCount(0);
      }
    }
    // A save would have landed by now: the editor writes a changed field at once.
    await page.waitForTimeout(1_000);
    expect(readFileSync(join(site, name), 'utf8')).toBe(EVENTS[name]);
  };
  await goTo('Talk', 'A talk.');
  await untouched('talk.md', { start: ['2026-12-01', ''], end: ['2026-12-01', '21:00'] });
  await goTo('Next', 'A walk.');
  await untouched('walk.md', { start: ['', ''], end: ['2026-12-02', '10:00'] });

  // The template's empty field: a date, then a time, then the date alone again.
  await goTo('Next', 'A fair.');
  const fair = (start: string) => `---\ntitle: Fair\n${start}---\nA fair.\n`;
  const file = () => readFileSync(join(site, 'fair.md'), 'utf8');
  await open('start');
  expect(await shown()).toEqual(['', '']);
  await date.fill('2026-12-05');
  await expect.poll(file, { timeout: 15_000 }).toBe(fair('start: 2026-12-05\n'));
  await time.fill('19:30');
  await expect.poll(file, { timeout: 15_000 }).toBe(fair('start: 2026-12-05 19:30\n'));
  await preview.waitForSelector('time.moss-event-time[datetime="2026-12-05T19:30"]', { timeout: 30_000 });

  // Both controls sit inside the popover at its narrowest, uncut, in both themes.
  for (const theme of ['light', 'dark']) {
    await editor.evaluate((t) => (document.documentElement.dataset.theme = t), theme);
    const box = await pop.evaluate((el) => {
      const inner = (e: Element) => e.getBoundingClientRect();
      const [d, t] = [...el.querySelectorAll('input')];
      return {
        width: Math.round(inner(el).width),
        inside: inner(d).left >= inner(el).left && inner(t).right <= inner(el).right && inner(d).right <= inner(t).left,
        cut: d.scrollWidth > d.clientWidth || t.scrollWidth > t.clientWidth,
      };
    });
    expect(box, theme).toEqual({ width: box.width, inside: true, cut: false });
    expect(box.width, 'the popover stays within its width limits').toBeLessThanOrEqual(320);
    await pop.screenshot({ path: testInfo.outputPath(`event-time-${theme}.png`) });
  }
  await editor.evaluate(() => delete document.documentElement.dataset.theme);

  // Deleting one part of the date leaves it half-typed, which is not a
  // cleared date: the field stays in the file.
  await date.focus();
  await page.keyboard.press('Backspace');
  await page.waitForTimeout(1_000);
  expect(file()).toBe(fair('start: 2026-12-05 19:30\n'));
  await date.fill('2026-12-05');

  await time.fill('');
  await expect.poll(file, { timeout: 15_000 }).toBe(fair('start: 2026-12-05\n'));

  // Clearing the date removes the field, and its chip goes with the popover.
  await date.fill('');
  await expect.poll(file, { timeout: 15_000 }).toBe(fair(''));
  await page.keyboard.press('Escape');
  await expect(pop).toHaveCount(0);
  await expect(chip('start')).toHaveCount(0);

  // Adding the field suggests today and writes nothing until it is accepted.
  await editor.click('.chip-unified-btn');
  await editor.click('.omenu .orow-add:has-text("Start")');
  await expect(date).toBeFocused();
  const today = await date.inputValue();
  expect(today).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  await page.waitForTimeout(1_000);
  expect(file()).toBe(fair(''));
  await page.keyboard.press('Enter');
  await expect.poll(file, { timeout: 15_000 }).toBe(fair(`start: ${today}\n`));
});

test("a folder's own page listing its folder by name is not marked missing", async ({ page }) => {
  const { editor } = await openEditor(page);
  await openTreeRoot(editor);
  await editor.click('.seg[data-path="meetings"]:not(.bc)');
  await editor.click('.seg[data-path="meetings/meetings.md"]');
  const marked = (target: string) => editor.locator(`.cm-line:has-text("![[${target}/") .cm-problem--blocks`);
  // The missing folder's mark shows the resolver has answered for this page.
  await expect(marked('nowhere')).not.toHaveCount(0, { timeout: 30_000 });
  await expect(marked('meetings')).toHaveCount(0);
});

/** Follow the home page's link to the page two folders down, so the breadcrumb
 *  re-seats on it, and wait for the build's count to reach its row. */
async function openCasePage(page: Page) {
  const frames = await openEditor(page);
  const { editor, preview } = frames;
  await editor.waitForSelector('.cm-content', { timeout: 30_000 });
  await preview.click('a:has-text("To the case")');
  await expect(editor.locator('.seg.bc-current .tree-preflight-count')).toHaveText('7', { timeout: 30_000 });
  return frames;
}

/** A picture of the editor in each colour scheme, kept when `GATE_SHOTS` names a folder. */
async function shots(page: Page, name: string, browserName: string) {
  const dir = process.env.GATE_SHOTS;
  if (!dir) return;
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.locator('#moss-editor-frame').screenshot({ path: join(dir, `${name}-${browserName}-${scheme}.png`) });
  }
  await page.emulateMedia({ colorScheme: 'light' });
}

test("a page's problem count shows once, and steps through its problems", async ({ page, browserName }) => {
  const { editor } = await openCasePage(page);
  const count = (path: string) => editor.locator(`.seg[data-path="${path}"] .tree-preflight-count`);
  const colour = (selector: string) => editor.locator(selector).first().evaluate((el) => getComputedStyle(el).color);

  // The breadcrumb: the page's number, and no number on any folder above it;
  // each folder's chevron wears the number's colour instead.
  for (const folder of ['', 'en', 'en/writing']) {
    await expect(editor.locator(`.seg.bc[data-path="${folder}"]`)).toHaveCount(1);
    await expect(count(folder)).toHaveCount(0);
    expect(await colour(`.seg.bc[data-path="${folder}"] > .chev`)).toBe(await colour('.seg.bc-current .tree-preflight-count'));
  }
  await shots(page, 'count-breadcrumb', browserName);

  // The number is a button: a click, then the keyboard, each go one problem on.
  const caretLine = () => editor.evaluate(() => {
    const node = getSelection()?.anchorNode;
    return (node instanceof Element ? node : node?.parentElement)?.closest('.cm-line')?.textContent ?? '';
  });
  const button = editor.locator('.seg.bc-current button.tree-preflight-count');
  await expect(button).toHaveAccessibleName('7 unresolved references. Go to the next');
  expect(await button.evaluate((el) => getComputedStyle(el).cursor)).toBe('pointer');
  await button.click();
  await expect.poll(caretLine).toContain('slide-01.jpg');
  // A key that is not a modifier: WebKit shows the focus ring only after one.
  await page.keyboard.press('ArrowRight');
  await button.focus();
  expect(await button.evaluate((el) => getComputedStyle(el).outlineStyle)).not.toBe('none');
  await page.keyboard.press('Enter');
  await expect.poll(caretLine).toContain('slide-02.jpg');

  // The tree: a closed folder carries its total, an open one leaves it to its rows.
  await editor.click('.seg.bc[data-path=""]');
  await expect(count('drafts')).toHaveText('2');
  await editor.locator('.seg[data-path="drafts"]').scrollIntoViewIfNeeded();
  await shots(page, 'count-tree-closed', browserName);
  await editor.click('.seg[data-path="drafts"] > .chev');
  await expect(count('drafts/d.md')).toHaveText('2');
  await expect(count('drafts')).toHaveCount(0);
  await shots(page, 'count-tree-open', browserName);
});

test('a missing picture says what was looked for, and its card offers the ways out', async ({ page, browserName }) => {
  const { editor } = await openCasePage(page);
  const file = () => readFileSync(join(site, 'en', 'writing', 'case.md'), 'utf8');
  const card = editor.locator('.cm-reference-card');
  const actions = () => card.locator('.cm-reference-card-actions button').allTextContents();
  const mark = (name: string) => editor.locator(`.cm-problem--blocks:has-text("${name}")`);

  // Resting on the path: the reason, and nothing to press.
  await mark('slide-03.jpg').hover();
  await expect(card).toHaveText("No file named slide-03.jpg in your folder. The site can't be published without it.", { timeout: 10_000 });
  await expect(card.locator('button, input')).toHaveCount(0);
  await shots(page, 'missing-hover', browserName);
  await page.mouse.move(0, 0);

  // The card opened from the keyboard: choose a file or remove, and no widths.
  const mod = process.platform === 'darwin' ? 'Meta' : 'Control';
  await mark('slide-01.jpg').click();
  await page.keyboard.press(`${mod}+k`);
  await expect(card).toHaveClass(/cm-reference-card--editable/);
  expect(await actions()).toEqual(['Choose a file…', 'Remove']);
  // No preview frame for a file that is not there.
  await expect(card.locator('.cm-asset-hover')).toBeHidden();
  await shots(page, 'missing-card', browserName);
  // The buttons' words can be read on a dark card.
  await page.emulateMedia({ colorScheme: 'dark' });
  const contrast = await card.locator('.cm-reference-card-actions button').first().evaluate((el) => {
    const luminance = (rgb: string): number => {
      const [r, g, b] = rgb.match(/[\d.]+/g)!.slice(0, 3).map((v) => {
        const c = Number(v) / 255;
        return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
      });
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    let backdrop = 'rgb(255, 255, 255)';
    for (let e: HTMLElement | null = el as HTMLElement; e; e = e.parentElement) {
      // The scheme has just changed: read where each colour is going, not where it is.
      e.style.transition = 'none';
      const bg = getComputedStyle(e).backgroundColor;
      if (bg !== 'rgba(0, 0, 0, 0)' && bg !== 'transparent') { backdrop = bg; break; }
    }
    const [hi, lo] = [luminance(getComputedStyle(el).color), luminance(backdrop)].sort((a, b) => b - a);
    return (hi + 0.05) / (lo + 0.05);
  });
  expect(contrast, 'a card button is too faint to read in dark').toBeGreaterThanOrEqual(4.5);
  await page.emulateMedia({ colorScheme: 'light' });

  // Remove takes the line; one undo brings it back.
  await card.locator('button:has-text("Remove")').click();
  await expect.poll(file, { timeout: 15_000 }).not.toContain('slide-01.jpg');
  await editor.locator('.cm-content').focus();
  await page.keyboard.press(`${mod}+z`);
  await expect.poll(file, { timeout: 15_000 }).toBe(CASE_PAGE);

  // A click on the path starts the hover timer; the card opened right after
  // it keeps its buttons once that timer has run out. (This host cannot write
  // a reference form, so it offers no "Use <file>" row.)
  await mark('chart.jpg').click();
  await page.keyboard.press(`${mod}+k`);
  await page.waitForTimeout(400);
  await expect(card).toHaveClass(/cm-reference-card--editable/);
  expect(await actions()).toEqual(['Choose a file…', 'Remove']);
  await page.keyboard.press('Escape');

  // A picture that is there keeps its widths, and the row stays inside the card.
  await editor.locator('.cm-line', { hasText: 'here.png' }).click({ position: { x: 60, y: 10 } });
  await page.keyboard.press(`${mod}+k`);
  await expect(card).toHaveClass(/cm-reference-card--editable/);
  expect(await actions()).toEqual(['Replace…', '25%', '50%', '75%', '100%', 'Reset']);
  const row = await card.locator('.cm-reference-card-actions').evaluate((r) => ({ wide: r.scrollWidth, room: r.clientWidth }));
  expect(row.wide, 'the buttons run past the card').toBeLessThanOrEqual(row.room);
  await shots(page, 'found-picture-card', browserName);
});

test('a new page in a language folder shows in the preview before it has text, and is an ordinary page after one word', async ({ page }, testInfo) => {
  const { shell, editor, preview } = await openEditor(page);
  const openFile = () => editor.evaluate(() => decodeURIComponent(location.search));
  const file = join(site, 'zh-hans', 'Quiet Harbour Notes.md');
  const address = '/zh-hans/quiet-harbour-notes/';
  const folderLists = () => preview.evaluate(async (at) => (await (await fetch('/zh-hans/')).text()).includes(at), address);

  await preview.click('a:has-text("To the Chinese pages")');
  await expect.poll(openFile, { timeout: 15_000 }).toContain('zh-hans/index.md');
  await editor.click('#editor-create-btn');
  await expect.poll(openFile, { timeout: 15_000 }).toContain('zh-hans/untitled.md');
  await editor.click('#editor-heading');
  await page.keyboard.type('Quiet Harbour Notes');
  await page.keyboard.press('Enter');
  await expect.poll(() => existsSync(file), { timeout: 15_000 }).toBe(true);
  expect(readFileSync(file, 'utf8').trim(), 'the new page has no text yet').toBe('');

  // Shown at its address with the site's design, the waiting screen gone, the editor still on it.
  // Under the sweep's 20 s: the rename asks for its own build, whatever the file watcher delivers.
  await expect.poll(() => new URL(preview.url()).pathname, { timeout: 10_000 }).toBe(address);
  expect(output, 'the rename asked for its own build').toContain('Rebuild asked by move');
  await expect(preview.locator('h1')).toContainText('Quiet Harbour Notes', { timeout: 30_000 });
  await expect(shell.locator('#moss-preview-overlay')).not.toHaveClass(/visible/, { timeout: 30_000 });
  await expect(editor.locator('body')).not.toContainText('No page to edit');
  expect(await folderLists(), 'an empty page is in no listing').toBe(false);
  await page.screenshot({ path: testInfo.outputPath('new-page-before-text.png') });

  await editor.click('.cm-content');
  await page.keyboard.type('Word');
  await expect.poll(() => readFileSync(file, 'utf8'), { timeout: 15_000 }).toContain('Word');
  await expect(preview.locator('article, main').first()).toContainText('Word', { timeout: 30_000 });
  await expect.poll(folderLists, { timeout: 30_000, message: 'the folder lists the page once it has text' }).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('new-page-after-one-word.png') });
});

test('the create menu offers another language only for a page with no version in it', async ({ page }) => {
  const { editor, preview } = await openEditor(page);
  const openFile = () => editor.evaluate(() => decodeURIComponent(location.search));
  const rows = async () => {
    await editor.click('#editor-create-caret');
    await editor.locator('.ctx-menu .ctx-label').first().waitFor({ timeout: 10_000 });
    const labels = await editor.locator('.ctx-menu .ctx-label').allTextContents();
    await page.keyboard.press('Escape');
    return labels;
  };

  // Its Chinese version is linked by key under another file name.
  await preview.click('a:has-text("About")');
  await expect.poll(openFile, { timeout: 15_000 }).toContain('about.md');
  expect(await rows()).not.toContain('简体中文 version of this page');

  await preview.click('a:has-text("Talk")');
  await expect.poll(openFile, { timeout: 15_000 }).toContain('talk.md');
  expect(await rows()).toContain('简体中文 version of this page');
});

test('a page renamed in the folder while it is shown is followed to its folder address', async ({ page }) => {
  const { preview } = await openEditor(page);
  await preview.click('a:has-text("About")');
  await expect(preview.locator('body')).toContainText('About the garden', { timeout: 30_000 });

  renameSync(join(site, 'about.md'), join(site, 'about-us.md'));
  // Past the sweep's 20 s, for a rename the file watcher delivers late.
  await expect.poll(() => new URL(preview.url()).pathname, { timeout: 40_000 }).toMatch(/^\/about-us\//);
  expect(new URL(preview.url()).pathname).toBe('/about-us/');
  await expect(preview.locator('body')).toContainText('About the garden', { timeout: 30_000 });
});
