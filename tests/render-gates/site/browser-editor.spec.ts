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
 * Then an edit to a dated note changes the feed the page links as
 * `alternate`, and the preview still updates the page in place rather than
 * reloading it, since the page loads nothing from its feed.
 *
 * Then the address of a page just made, which the preview loads again with a
 * stamp on the frame's address once its first build ends: the right-click
 * hands that address to the menu the desktop opens, whose page entry copies it
 * without the stamp.
 *
 * Ports: playwright/gate-ports.ts, keys `browser-editor:ui` /
 * `browser-editor:preview`. Config: playwright/browser-editor.config.ts.
 */
import { test, expect, type Frame, type Page } from '../../support/browser-test';
import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { crc32, deflateSync } from 'node:zlib';
import { join } from 'node:path';
import { png } from '../../e2e/helpers/contents-ruler-site';
import { openEditor as openEditorFrames, openShell, useEditServe } from '../../e2e/helpers/edit-serve';
import { buildPreviewCtxEntries, type PreviewCtxPayload } from '../../../packages/moss-ui/src/ui/preview/preview-context-menu';

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
// Every kind of picture the preview's menu tells apart, at known body lines:
// a hero (fence on 1), a body picture (5), two pictures on one line (7),
// a grid that crops its cells (10, 12, 14) and a listing card's cover (17).
const PICTURES = ['---', 'title: Pictures', '---',
  ':::hero {image=wide.png max-height=200px}', '# Pictures', ':::', '',
  '![A room](room.png)', '',
  '![Twin|](sq.png) ![Pair|](wide.png)', '',
  ':::grid 3', '![One|](room.png)', '+++', '![Two|](wide.png)', '+++', '![Three|](room.png)', ':::', '',
  '![[picnotes/|style:summary]]', ''].join('\n');
const PICTURE_MENU = 'a right-click on a picture in the preview';

const PICTURE_GATE = 'a picture added, landed, replaced, renamed and moved shows in the preview each time';

/** A PNG of a diagonal gradient, large enough that its encode outlasts the build after it. */
function gradient(width: number, height: number, tint: number): Buffer {
  const rows = Buffer.alloc((width * 3 + 1) * height);
  for (let y = 0, at = 0; y < height; y++) {
    rows[at++] = 0;
    for (let x = 0; x < width; x++) {
      rows[at++] = (x + y) & 255;
      rows[at++] = tint;
      rows[at++] = (x ^ y) & 255;
    }
  }
  const chunk = (type: string, data: Buffer) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const frame = Buffer.alloc(body.length + 8);
    frame.writeUInt32BE(data.length, 0);
    body.copy(frame, 4);
    frame.writeUInt32BE(crc32(body), body.length + 4);
    return frame;
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 2, 0, 0, 0], 8);
  return Buffer.concat([Buffer.from('\x89PNG\r\n\x1a\n', 'latin1'), chunk('IHDR', header), chunk('IDAT', deflateSync(rows, { level: 1 })), chunk('IEND', Buffer.alloc(0))]);
}

const ONE_PIXEL_PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==', 'base64');

const serve = useEditServe('browser-editor', (site, testInfo) => {
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
  // Only the picture menu's tests: their five rows make the open file tree
  // tall enough that the others' editor is too short to keep a revealed line in view.
  if (testInfo.titlePath.includes(PICTURE_MENU)) {
    writeFileSync(join(site, 'pictures.md'), PICTURES);
    writeFileSync(join(site, 'room.png'), png(60, 90, [150, 110, 90]));
    writeFileSync(join(site, 'wide.png'), png(120, 60, [90, 120, 150]));
    writeFileSync(join(site, 'sq.png'), png(60, 60, [120, 150, 90]));
    mkdirSync(join(site, 'picnotes'));
    writeFileSync(join(site, 'picnotes', 'first.md'), '---\ntitle: First\ncover: ../room.png\n---\nA first note.\n');
  }
  if (testInfo.title === PICTURE_GATE) {
    mkdirSync(join(site, 'pictures'));
    writeFileSync(join(site, 'pictures', 'door.png'), gradient(800, 600, 40));
    writeFileSync(join(site, 'album.md'), '---\ntitle: Album\n---\nAn album.\n\n![Door](pictures/door.png)\n\n![Later](pictures/later.png)\n\nLast line.\n');
  }
  writeFileSync(join(site, 'about.md'), '---\ntitle: About\ntranslationKey: about\n---\nAbout the garden. [Talk](talk.md)\n');
  writeFileSync(join(site, 'zh-hans', '关于.md'), '---\ntitle: 关于\ntranslationKey: about\n---\n关于花园。\n');
  // A published address gives the site its feed, which every page links as
  // `alternate`, and a dated note is in that feed.
  mkdirSync(join(site, '.moss'));
  writeFileSync(join(site, '.moss', 'config.toml'), '[site]\nurl = "https://garden.example.org"\n');
  mkdirSync(join(site, 'notes'));
  writeFileSync(join(site, 'notes', 'first-frost.md'), '---\ntitle: First frost\ndate: 2026-10-01\n---\nThe beds are covered.\n');
}, {
  // The new-page test reads the rename's own request from the log.
  MOSS_LOG_LEVEL: 'info',
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
const openEditor = (page: Page) => openEditorFrames(page, serve, 'Hello garden');

test('the browser editor boots, saves, opens a sandboxed PDF in the file viewer, and follows the preview home', async ({ page, browserName }) => {
  const { shell, editor, preview } = await openEditor(page);
  await editor.waitForSelector('.cm-content', { timeout: 30_000 });
  await editor.click('.cm-line:has-text("Hello garden")');
  await page.keyboard.type(' Typed in a browser.');
  await expect.poll(() => readFileSync(join(serve.site, 'index.md'), 'utf8'), { timeout: 15_000 }).toContain('Typed in a browser.');
  await preview.waitForFunction(() => document.body?.innerText.includes('Typed in a browser'), null, { timeout: 30_000 });

  await shell.click('#moss-device-button');
  await shell.waitForFunction(() => document.querySelector('dialog')?.textContent?.includes('--public-url'), null, { timeout: 5_000 });
  await shell.press('dialog', 'Escape');

  const pdfResponse = page.waitForResponse((r) => r.url().includes('/__moss/source/doc.pdf'), { timeout: 15_000 });
  // The preview follows to the PDF's own address, and the shell asks what backs it.
  const backing = page.waitForResponse(
    (r) => r.url().endsWith('/resolve_page_source') && (r.request().postData() ?? '').includes('doc.pdf'),
    // Its clock starts before the click and the viewer's mount, which take up to 15 s of their own on a loaded machine.
    { timeout: 30_000 },
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
    await expect.poll(() => readFileSync(join(serve.site, 'Workshops', 'untitled 2.md'), 'utf8'), { timeout: 15_000 }).toContain('Second new page.');
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
    expect(readFileSync(join(serve.site, name), 'utf8')).toBe(EVENTS[name]);
  };
  await goTo('Talk', 'A talk.');
  await untouched('talk.md', { start: ['2026-12-01', ''], end: ['2026-12-01', '21:00'] });
  await goTo('Next', 'A walk.');
  await untouched('walk.md', { start: ['', ''], end: ['2026-12-02', '10:00'] });

  // The template's empty field: a date, then a time, then the date alone again.
  await goTo('Next', 'A fair.');
  const fair = (start: string) => `---\ntitle: Fair\n${start}---\nA fair.\n`;
  const file = () => readFileSync(join(serve.site, 'fair.md'), 'utf8');
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

test("the shell follows the site's theme and page colour", async ({ page }) => {
  const { shell, preview } = await openShell(page, serve, 'Hello garden.');
  const pageColor = () => shell.evaluate(() => document.documentElement.style.getPropertyValue('--moss-page-color'));
  const siteColor = () => preview.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--moss-color-bg').trim());
  await expect.poll(pageColor, { timeout: 10_000 }).toBe(await siteColor());
  const before = await pageColor();
  const flipped = (await shell.evaluate(() => document.documentElement.dataset.theme)) === 'dark' ? 'light' : 'dark';
  await preview.click('.nav-theme-btn');
  await expect.poll(() => shell.evaluate(() => document.documentElement.dataset.theme), { timeout: 10_000 }).toBe(flipped);
  await expect.poll(pageColor, { timeout: 10_000 }).not.toBe(before);
  expect(await pageColor()).toBe(await siteColor());
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
  // Read through the selector in one step: a build landing re-renders the breadcrumb's children, and a
  // style read off a node it has just replaced is empty.
  const colour = (selector: string) => editor.evaluate((s) => getComputedStyle(document.querySelector(s)!).color, selector);

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
  await expect(button).toHaveCSS('cursor', 'pointer');
  await button.click();
  await expect.poll(caretLine).toContain('slide-01.jpg');
  // A key that is not a modifier: WebKit shows the focus ring only after one.
  await page.keyboard.press('ArrowRight');
  await button.focus();
  expect(await button.evaluate((el) => getComputedStyle(el).outlineStyle)).not.toBe('none');
  // A build landing between the focus and the key rebuilds the button; the key must still reach it.
  const focused = await button.elementHandle();
  writeFileSync(join(serve.site, 'en', 'writing', 'other.md'), '---\ntitle: Other\n---\nStill nothing missing.\n');
  await expect.poll(() => focused!.evaluate((el) => el.isConnected), { timeout: 15_000 }).toBe(false);
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
  const file = () => readFileSync(join(serve.site, 'en', 'writing', 'case.md'), 'utf8');
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
  const file = join(serve.site, 'zh-hans', 'Quiet Harbour Notes.md');
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
  expect(serve.output, 'the rename asked for its own build').toContain('Rebuild asked by move');
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

  renameSync(join(serve.site, 'about.md'), join(serve.site, 'about-us.md'));
  // Past the sweep's 20 s, for a rename the file watcher delivers late.
  await expect.poll(() => new URL(preview.url()).pathname, { timeout: 40_000 }).toMatch(/^\/about-us\//);
  expect(new URL(preview.url()).pathname).toBe('/about-us/');
  await expect(preview.locator('body')).toContainText('About the garden', { timeout: 30_000 });
});

test('an edit to a page in the feed updates it in place, not by a reload', async ({ page }) => {
  const { preview } = await openEditor(page);
  // The first build is promoted about 20 s after the server starts, and the
  // preview refreshes for that too: the marker below is set after it.
  await expect.poll(() => serve.output, { timeout: 60_000, message: 'the first build promoted' }).toMatch(/promoted generation \S+ to current/);
  await preview.evaluate(() => location.assign('/notes/first-frost/'));
  // The home page lists the note's text too: wait for the note's own address.
  await preview.waitForFunction(() => location.pathname === '/notes/first-frost/' && document.readyState === 'complete' && document.body.innerText.includes('The beds are covered'), null, { timeout: 30_000 });
  await expect(preview.locator('link[rel="alternate"][href="/rss.xml"]')).toHaveCount(1);
  // A reload makes a new window; an update in place keeps this one.
  await preview.evaluate(() => ((window as unknown as { kept: boolean }).kept = true));

  writeFileSync(join(serve.site, 'notes', 'first-frost.md'), readFileSync(join(serve.site, 'notes', 'first-frost.md'), 'utf8') + '\nThe well is closed.\n');
  await preview.waitForFunction(() => document.body?.innerText.includes('The well is closed'), null, { timeout: 30_000 });
  expect(await preview.evaluate(async () => (await (await fetch('/rss.xml')).text()).includes('The well is closed')), 'the edit changed the feed').toBe(true);
  expect(await preview.evaluate(() => (window as unknown as { kept?: boolean }).kept), 'the page was reloaded').toBe(true);
});

// Red only in WebKit without the fix: Chromium's in-place refresh succeeds mid-navigation, WebKit's fails and falls back to a reload.
// A site script leaves the page with no click to read (search, the series keys, forms).
const LEAVINGS: [string, (preview: Frame) => Promise<unknown>][] = [
  ['a clicked page', (preview) => preview.click('a:has-text("To the Chinese pages")')],
  ['a page a site script opens', (preview) => preview.evaluate(() => location.assign('/zh-hans/'))],
];
for (const [what, leave] of LEAVINGS) test(`a build landing while ${what} loads does not send the preview back`, async ({ page }) => {
  const { preview } = await openEditor(page);
  await expect.poll(() => serve.output, { timeout: 60_000, message: 'the first build promoted' }).toMatch(/promoted generation \S+ to current/);
  // Hold the clicked page's response so the build below always lands mid-navigation.
  let release!: () => void;
  const held = new Promise<void>((resolve) => (release = resolve));
  await page.route('**/zh-hans/**', async (route) => {
    await held;
    await route.continue().catch(() => {}); // a navigation the frame cancelled has nothing to continue
  });
  const refreshes: string[] = [];
  page.on('console', (m) => m.text().startsWith('[preview-refresh]') && refreshes.push(m.text()));

  await leave(preview);
  // Both the page being left and the page being loaded change, so the shell refreshes whichever it watches.
  for (const file of ['index.md', join('zh-hans', 'index.md')]) {
    writeFileSync(join(serve.site, file), readFileSync(join(serve.site, file), 'utf8') + '\nA held line.\n');
  }
  await expect.poll(() => refreshes.length, { timeout: 30_000, message: 'the build refreshed the preview' }).toBeGreaterThan(0);
  release();

  await expect.poll(() => new URL(preview.url()).pathname, { timeout: 15_000 }).toBe('/zh-hans/');
  await expect(preview.locator('body')).toContainText('你好，花园。', { timeout: 15_000 });
});

// The places explorer writes its view into the address at every pan's end; that loads nothing, so the next build still shows.
test('a build after a site script rewrites the address in place still refreshes the page', async ({ page }) => {
  const { preview } = await openEditor(page);
  await expect.poll(() => serve.output, { timeout: 60_000, message: 'the first build promoted' }).toMatch(/promoted generation \S+ to current/);
  await expect(preview.locator('body')).toContainText('Hello garden', { timeout: 15_000 });
  await preview.evaluate(() => history.replaceState(null, '', '?z=4'));
  writeFileSync(join(serve.site, 'index.md'), readFileSync(join(serve.site, 'index.md'), 'utf8') + '\nA line after the pan.\n');
  await expect(preview.locator('body')).toContainText('A line after the pan', { timeout: 15_000 });
});

test('the address copied from a page just made carries no reload stamp', async ({ page }) => {
  const { shell, editor, preview } = await openEditor(page);
  await preview.click('a:has-text("To the Chinese pages")');
  await expect.poll(() => editor.evaluate(() => decodeURIComponent(location.search)), { timeout: 15_000 }).toContain('zh-hans/index.md');
  await editor.click('#editor-create-btn');
  // The preview waits at the new page's address and, once its first build ends, loads it again with the stamp.
  await expect.poll(() => preview.url(), { timeout: 40_000 }).toMatch(/\/zh-hans\/untitled\/\?.*_t=/);
  await expect(shell.locator('#moss-preview-overlay')).not.toHaveClass(/visible/, { timeout: 30_000 });

  // The browser editor opens no menu of its own, so the payload is read where the desktop reads it.
  const rightClick = shell.evaluate(() => new Promise<PreviewCtxPayload>((resolve) =>
    window.addEventListener('message', (e) => e.data?.type === 'moss-context-menu' && resolve(e.data))));
  await preview.click('body', { button: 'right', position: { x: 5, y: 5 } });

  const copied: string[] = [];
  const payload = await rightClick;
  expect(payload.page).toContain('_t=');
  const entries = buildPreviewCtxEntries(payload, {
    siteUrl: null, canEdit: false, pageFile: null, showInternal: false,
    openTarget: () => {}, openStyle: () => {}, openExternal: () => {},
    copy: (text) => copied.push(text),
  });
  const copyPage = entries.find((e) => 'action' in e && e.label === 'Copy page address') as { action(): void };
  copyPage.action();
  expect(copied).toEqual(['/zh-hans/untitled/']);
});

test(PICTURE_GATE, async ({ page }) => {
  test.setTimeout(240_000);
  const { editor, preview } = await openEditor(page);
  await editor.click('.seg.bc[data-path=""]');
  await editor.click('.seg[data-path="album.md"]');
  await preview.waitForFunction(() => document.body?.innerText.includes('An album'), null, { timeout: 30_000 });
  await editor.waitForSelector('.cm-line:has-text("Last line.")', { timeout: 30_000 });

  const notFound: string[] = [];
  page.on('response', (r) => {
    if (r.status() === 404 && /harbour|quay|later/.test(r.url())) notFound.push(new URL(r.url()).pathname);
  });
  /**
   * The preview's picture whose address holds `name`: its drawn size, its `<source>` count and what it loaded.
   * Null while the preview reloads for a build that just landed: the page a person sees is replaced whole, so the next read answers.
   */
  const picture = (name: string) =>
    preview.evaluate((name) => {
      const img = [...document.querySelectorAll('img')].find((i) => (i.getAttribute('src') ?? '').includes(name));
      if (!img) return null;
      if (!img.complete || !img.naturalWidth) img.scrollIntoView({ block: 'center' });
      return { width: img.naturalWidth, height: img.naturalHeight, sources: img.closest('picture')?.querySelectorAll('source').length ?? 0, loaded: img.currentSrc };
    }, name).catch((error: Error) => {
      if (/Execution context was destroyed/.test(error.message)) return null;
      throw error;
    });
  const shown = (name: string) => expect.poll(async () => (await picture(name))?.width ?? 0, { timeout: 60_000, message: `the preview draws ${name}` }).toBeGreaterThan(0);

  await test.step('a picture added to the page from the insert bar', async () => {
    await editor.click('.cm-line:has-text("Last line.")');
    await page.keyboard.press('End');
    await page.keyboard.press('Enter');
    await page.keyboard.press('Enter');
    await expect.poll(() => readFileSync(join(serve.site, 'album.md'), 'utf8'), { timeout: 15_000 }).toMatch(/Last line\.\n\n/);
    const chooser = page.waitForEvent('filechooser', { timeout: 10_000 });
    await editor.click('.insert-bar button[data-cmd="media"]');
    await (await chooser).setFiles({ name: 'harbour.png', mimeType: 'image/png', buffer: gradient(3600, 2400, 120) });
    await shown('harbour');
    // A build that seals while the picture still encodes is the one that dropped the
    // sizes, so they are read from the first paint until the encode has staged its files.
    const encoded = () => existsSync(serve.buildDir) && readdirSync(serve.buildDir, { recursive: true, encoding: 'utf8' }).some((f) => /harbour[^/]*\.webp$/.test(f));
    for (const until = Date.now() + 120_000; ; ) {
      const [p, done] = [await picture('harbour'), encoded()];
      if (p) expect(p.sources, 'a build made while the picture encodes dropped its sizes').toBeGreaterThan(0);
      if (done) break;
      expect(Date.now(), 'the encode stages the picture').toBeLessThan(until);
      await page.waitForTimeout(250);
    }
    await expect
      .poll(async () => { const p = await picture('harbour'); return p !== null && p.sources > 0 && /\.webp/.test(p.loaded) && p.width > 0; }, { timeout: 120_000, message: 'the added picture ends with its responsive sizes' })
      .toBe(true);
  });

  await test.step('a picture the page already names lands in its folder', async () => {
    // Until now the page named a file that was not there.
    notFound.splice(0, notFound.length, ...notFound.filter((path) => !path.includes('later')));
    writeFileSync(join(serve.site, 'pictures', 'later.png'), gradient(900, 1200, 200));
    await shown('later');
  });

  await test.step('a picture is replaced under its own name', async () => {
    writeFileSync(join(serve.site, 'pictures', 'door.png'), gradient(600, 1200, 90));
    await expect.poll(async () => { const p = await picture('door'); return p && p.width ? p.height / p.width : 0; }, { timeout: 60_000, message: 'the preview draws the new door' }).toBe(2);
  });

  await test.step('a picture renamed, then moved, from the file list', async () => {
    const folder = readdirSync(serve.site, { recursive: true, encoding: 'utf8' }).find((f) => f.endsWith('harbour.png') && !f.startsWith('.moss'))!.replace(/\/?harbour\.png$/, '');
    const levels = folder ? folder.split('/') : [];
    for (let depth = 1; depth <= levels.length; depth++) await editor.click(`.seg[data-path="${levels.slice(0, depth).join('/')}"]:not(.bc)`);
    const inFolder = (name: string) => (folder ? `${folder}/${name}` : name);
    await editor.click(`.seg[data-path="${inFolder('harbour.png')}"]`, { button: 'right' });
    await editor.locator('[role="menuitem"]:has-text("Rename"), :text-is("Rename")').first().click();
    const name = editor.locator('input.moss-tree-rename-input');
    await name.fill('quay');
    await name.press('Enter');
    await shown('quay');

    await editor.locator(`.seg[data-path="${inFolder('quay.png')}"]`).dragTo(editor.locator('.seg[data-path="events"]:not(.bc)').first());
    await shown('events/quay');
  });

  expect(notFound, 'the preview answered 404 for a picture that is in the folder').toEqual([]);
});

test.describe(PICTURE_MENU, () => {
  /** The editor beside the pictures page, with the shell's clipboard and browser opener recorded. */
  async function onPictures(page: Page) {
    const frames = await openEditor(page);
    await frames.shell.evaluate(() => {
      const w = window as unknown as { __copied: string[]; __opened: string[] };
      w.__copied = [];
      w.__opened = [];
      Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (text: string) => void w.__copied.push(text) } });
      window.open = (url) => (w.__opened.push(String(url)), null);
    });
    await frames.preview.evaluate(() => location.assign('/pictures/'));
    await frames.preview.waitForSelector('section.moss-hero img', { timeout: 30_000 });
    await expect(frames.editor.locator('.cm-content')).toContainText('![A room](room.png)', { timeout: 15_000 });
    return frames;
  }

  /** Right-click at a point of `selector` in the preview; the menu's row labels, in order. */
  async function menuOn(f: { shell: Frame; preview: Frame }, selector: string, position = { x: 6, y: 6 }) {
    await f.preview.locator(selector).first().click({ button: 'right', position });
    const menu = f.shell.locator('.ctx-menu[role="menu"]');
    await expect(menu).toBeVisible();
    return menu.locator('.ctx-item .ctx-label').allTextContents();
  }
  const row = (shell: Frame, label: string) => shell.locator('.ctx-menu .ctx-item', { hasText: label }).first().click();
  const tail = ['Copy picture address', 'Copy page address', 'Open in browser'];
  const selectedText = (editor: Frame) => editor.evaluate(() => getSelection()?.toString() ?? '');
  const caretLine = (editor: Frame) => editor.evaluate(() => {
    const at = getSelection()?.anchorNode;
    return (at instanceof Element ? at : at?.parentElement)?.closest('.cm-line')?.textContent ?? '';
  });
  const revealed = (editor: Frame) => editor.evaluate(() =>
    [...document.querySelectorAll('.seg.bc, [aria-selected="true"]')].map((e) => e.getAttribute('data-path') ?? e.textContent ?? ''));

  test('a body picture: each row once, each doing what it says, by keyboard too', async ({ page }) => {
    const { shell, editor, preview } = await onPictures(page);
    const body = 'figure.moss-image img';
    expect(await menuOn({ shell, preview }, body)).toEqual(['Replace picture…', 'Edit description', 'Show in file list', 'Edit this page', ...tail]);

    // The menu has the keyboard: arrows move through it and Escape closes it.
    await expect(shell.locator('.ctx-menu .ctx-item').first()).toBeFocused();
    await page.keyboard.press('ArrowDown');
    await expect(shell.locator('.ctx-menu .ctx-item').nth(1)).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(shell.locator('.ctx-menu')).toHaveCount(0);

    await menuOn({ shell, preview }, body);
    await row(shell, 'Edit description');
    await expect.poll(() => selectedText(editor)).toBe('A room');

    await menuOn({ shell, preview }, body);
    await row(shell, 'Edit this page');
    await expect.poll(() => caretLine(editor)).toContain('![A room](room.png)');

    await menuOn({ shell, preview }, body);
    await row(shell, 'Show in file list');
    await expect.poll(() => revealed(editor)).toContain('room.png');

    await menuOn({ shell, preview }, body);
    await row(shell, 'Copy picture address');
    await menuOn({ shell, preview }, body);
    await row(shell, 'Copy page address');
    await menuOn({ shell, preview }, body);
    await row(shell, 'Open in browser');
    const recorded = await shell.evaluate(() => {
      const w = window as unknown as { __copied: string[]; __opened: string[] };
      return { __copied: w.__copied, __opened: w.__opened };
    });
    expect(recorded.__copied).toEqual([expect.stringMatching(/^\/room[^/]*\.(png|webp)$/), '/pictures/']);
    expect(recorded.__opened).toEqual([expect.stringMatching(/\/pictures\/$/)]);

    await menuOn({ shell, preview }, body);
    const chooser = page.waitForEvent('filechooser');
    await row(shell, 'Replace picture…');
    await (await chooser).setFiles({ name: 'door.png', mimeType: 'image/png', buffer: png(40, 40, [60, 60, 60]) });
    await expect(editor.locator('.cm-content')).toContainText('![A room](door.png)', { timeout: 15_000 });
  });

  test('a picture a grid crops offers its focus point, and the row puts the keyboard on it', async ({ page }) => {
    const { shell, editor, preview } = await onPictures(page);
    expect(await menuOn({ shell, preview }, '.moss-grid-card img')).toEqual(['Replace picture…', 'Set focus point', 'Edit description', 'Show in file list', 'Edit this page', ...tail]);
    await row(shell, 'Set focus point');
    await expect.poll(() => editor.evaluate(() => document.activeElement?.classList.contains('cm-focus-point-dot') ?? false), { timeout: 15_000 }).toBe(true);
  });

  test("a hero's picture: no description row, and its focus point", async ({ page }) => {
    const { shell, editor, preview } = await onPictures(page);
    expect(await menuOn({ shell, preview }, 'section.moss-hero')).toEqual(['Replace picture…', 'Set focus point', 'Show in file list', 'Edit this page', ...tail]);
    await row(shell, 'Set focus point');
    await expect.poll(() => editor.evaluate(() => document.activeElement?.classList.contains('cm-focus-point-dot') ?? false), { timeout: 15_000 }).toBe(true);
  });

  test('of two pictures on one line, the row acts on the one clicked', async ({ page }) => {
    const { shell, editor, preview } = await onPictures(page);
    expect(await menuOn({ shell, preview }, '.moss-gallery-item >> nth=1')).toEqual(['Replace picture…', 'Edit description', 'Show in file list', 'Edit this page', ...tail]);
    await row(shell, 'Edit description');
    await expect.poll(() => selectedText(editor)).toBe('Pair');
    await menuOn({ shell, preview }, '.moss-gallery-item >> nth=0');
    await row(shell, 'Show in file list');
    await expect.poll(() => revealed(editor)).toContain('sq.png');
  });

  test("a listing card's cover is another page's: no picture rows, and its edit row opens that page", async ({ page }) => {
    const { shell, editor, preview } = await onPictures(page);
    expect(await menuOn({ shell, preview }, '.moss-card img')).toEqual(['Edit first page', 'Copy link address', ...tail]);
    await row(shell, 'Edit first page');
    await expect(editor.locator('.cm-content')).toContainText('A first note.', { timeout: 15_000 });
  });
});
