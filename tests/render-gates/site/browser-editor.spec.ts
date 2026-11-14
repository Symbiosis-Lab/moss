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
 * Ports: playwright/gate-ports.ts, keys `browser-editor:ui` /
 * `browser-editor:preview`. Config: playwright/browser-editor.config.ts.
 */
import { test, expect, type Frame, type Page } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
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
  writeFileSync(join(site, 'index.md'), '---\ntitle: Garden\n---\nHello garden.\n\n![[doc.pdf]]\n\n[Talk](talk.md)\n\n[To the workshops](/workshops/)\n\n[About](about.md)\n\n[To the Chinese pages](/zh-hans/)\n\n' +
    ':::grid 2\nBeds are dug in March.\n\nSeeds go in by April.\n+++\nWater comes from the well.\n\nThe well is closed in winter.\n:::\n');
  for (const [name, text] of Object.entries(EVENTS)) writeFileSync(join(site, name), text);
  writeFileSync(join(site, 'doc.pdf'), ONE_PAGE_PDF);
  writeFileSync(join(site, 'footer.md'), 'Tended by hand.\n');
  mkdirSync(join(site, 'events'));
  mkdirSync(join(site, 'Workshops'));
  writeFileSync(join(site, 'Workshops', 'index.md'), '---\ntitle: Workshops\n---\nAll workshops.\n');
  writeFileSync(join(site, 'events', 'reading-circle.md'), '---\ntitle: Reading circle\n---\nWe read together.\n\n[Back home](/)\n');
  mkdirSync(join(site, 'meetings'));
  writeFileSync(join(site, 'meetings', 'meetings.md'), '---\ntitle: Meetings\n---\nOurs:\n\n![[meetings/|style:summary]]\n\nGone:\n\n![[nowhere/|style:summary]]\n');
  writeFileSync(join(site, 'meetings', 'first.md'), '---\ntitle: First\n---\nThe first meeting.\n');
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
      // The new-page test reads the watch's start and the rename's own request from the log.
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

test('a new page in a language folder shows in the preview before it has text, and is an ordinary page after one word', async ({ page }, testInfo) => {
  const { shell, editor, preview } = await openEditor(page);
  const openFile = () => editor.evaluate(() => decodeURIComponent(location.search));
  const file = join(site, 'zh-hans', 'Quiet Harbour Notes.md');
  const address = '/zh-hans/quiet-harbour-notes/';
  const folderLists = () => preview.evaluate(async (at) => (await (await fetch('/zh-hans/')).text()).includes(at), address);

  await preview.click('a:has-text("To the Chinese pages")');
  await expect.poll(openFile, { timeout: 15_000 }).toContain('zh-hans/index.md');
  // The editor is usable about a second before the watch starts, and a rename
  // made in that second has no rebuild worker to ask.
  await expect.poll(() => output, { timeout: 30_000 }).toContain('File watcher active');
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
