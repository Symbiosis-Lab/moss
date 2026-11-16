/**
 * A picture's focus point in the editor `moss edit --serve` serves, in a real
 * browser. The picture is drawn below its line whether or not the caret is on
 * it, so placing the caret there moves no line of text. The dot is laid over
 * the picture and carries no words. Dragging it, or moving it with the arrow
 * keys, writes the position into the file in the form the build reads, and the
 * picture on the page is still loaded afterwards. Resting the pointer on the
 * path of a picture that is already drawn opens no card.
 *
 * Ports: playwright/gate-ports.ts, keys `focus-point:ui` / `focus-point:preview`.
 * Config: playwright/focus-point.config.ts. With SHOTS set to a directory, the
 * editor is photographed there in the light and dark schemes.
 */
import { test, expect, type Frame, type Page } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { deflateSync } from 'node:zlib';
import { gatePort } from '../../../playwright/gate-ports';
import { MOSS_BIN } from '../../e2e/helpers/scratch-site';

const EMBED = '![A room|](room.png)';
// The hero's picture is in a folder of its own, so it is found by the editor's resolver and not by sitting beside the page.
const HERO = ':::hero {image=art/room.png max-height=240px}';
const PAGE = ['---', 'title: Garden', '---', 'Opening words.', '', HERO, '# A room', ':::', '',
  ':::grid 2 {screen}', 'Words beside the picture.', '', EMBED, '+++', 'Second cell.', ':::', '', 'Closing words.', ''].join('\n');
/** Six pictures in a three-column grid: where a picture drawn below each cell's line would show a jump in a narrow column. */
const CELLS = [1, 2, 3, 4, 5, 6].map((n) => `![Room ${n}|](room.png)`);
const GALLERY = ['---', 'title: Garden', '---', 'Opening words.', '', ':::grid 3', CELLS.join('\n+++\n'), ':::', '', 'Closing words.', ''].join('\n');

/** A portrait PNG with a gradient, so a wide frame crops it and the crop is visible. */
function png(width: number, height: number): Buffer {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (b: Buffer) => { let c = 0xffffffff; for (const x of b) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type: string, data: Buffer) => {
    const body = Buffer.concat([Buffer.from(type, 'latin1'), data]);
    const out = Buffer.alloc(body.length + 8);
    out.writeUInt32BE(data.length, 0);
    body.copy(out, 4);
    out.writeUInt32BE(crc(body), body.length + 4);
    return out;
  };
  const head = Buffer.alloc(13);
  head.writeUInt32BE(width, 0);
  head.writeUInt32BE(height, 4);
  head.set([8, 2, 0, 0, 0], 8);
  const rows = Buffer.alloc((width * 3 + 1) * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      rows.set([Math.round((x / width) * 255), Math.round((y / height) * 255), 140], y * (width * 3 + 1) + 1 + x * 3);
    }
  }
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', head), chunk('IDAT', deflateSync(rows)), chunk('IEND', Buffer.alloc(0))]);
}

let site = '';
let home = '';
let server: ChildProcess | undefined;
let output = '';

test.beforeEach(async () => {
  output = '';
  site = realpathSync(mkdtempSync(join(tmpdir(), 'moss-focus-point-site-')));
  home = realpathSync(mkdtempSync(join(tmpdir(), 'moss-focus-point-home-')));
  mkdirSync(join(home, 'moss'));
  writeFileSync(join(site, 'index.md'), PAGE);
  writeFileSync(join(site, 'room.png'), png(600, 900));
  mkdirSync(join(site, 'art'));
  writeFileSync(join(site, 'art', 'room.png'), png(600, 900));
  server = spawn(MOSS_BIN, ['edit', site, '--serve', '--no-browser', '--port', String(gatePort('focus-point:ui'))], {
    env: {
      ...process.env,
      MOSS_HOME: join(home, 'moss'),
      MOSS_DESKTOP_SEARCH_ROOT: home,
      MOSS_PREVIEW_PORT_BASE: String(gatePort('focus-point:preview')),
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

async function openEditor(page: Page, { width = 2400, pictures = 2 } = {}): Promise<{ editor: Frame; preview: Frame }> {
  // The default is wide enough that the preview lays the page out for a desktop: at phone width a hero shows its picture whole, and a picture shown whole has no dot.
  await page.setViewportSize({ width, height: 1300 });
  await expect.poll(() => output.match(/Edit at (\S+)/)?.[1], { timeout: 60_000, message: `the sign-in link; server said:\n${output}` }).toBeTruthy();
  await page.goto(output.match(/Edit at (\S+)/)![1]);
  await page.waitForURL((u) => u.pathname === '/', { timeout: 15_000 });
  const frameOf = async (id: string) => (await (await page.waitForSelector(`#${id}`, { state: 'attached', timeout: 15_000 })).contentFrame())!;
  const shell = await frameOf('moss-shell-frame');
  const editor = await frameOf('moss-editor-frame');
  const preview = (await (await shell.waitForSelector('#moss-preview-iframe', { state: 'attached', timeout: 15_000 })).contentFrame())!;
  await preview.waitForFunction(() => document.body?.innerText.includes('Opening words'), null, { timeout: 30_000 });
  await shell.click('#moss-edit-button');
  await expect(page.locator('#moss-frames')).toHaveAttribute('data-editor', 'split', { timeout: 10_000 });
  await editor.waitForSelector('.cm-content', { timeout: 30_000 });
  // Every picture is drawn and sized before anything is measured.
  await expect.poll(() => editor.evaluate(() => [...document.querySelectorAll<HTMLImageElement>('.cm-lp-image-row img')].filter((i) => i.complete && i.naturalWidth > 0).length), { timeout: 30_000 }).toBe(pictures);
  return { editor, preview };
}

const line = (editor: Frame, text: string) => editor.locator('.cm-line', { hasText: text }).first();
/** Each line's distance from the top of the document, by its text. */
const lineTops = (editor: Frame) => editor.evaluate(() => {
  const top = document.querySelector('.cm-content')!.getBoundingClientRect().top;
  return [...document.querySelectorAll('.cm-content .cm-line')].map((l) => [l.textContent, Math.round(l.getBoundingClientRect().top - top)]);
});
const dot = (editor: Frame) => editor.locator('.cm-focus-point-dot');
/** Put the caret on the line reading `text` and wait for the dot on the picture drawn below that line. */
async function select(editor: Frame, text: string): Promise<void> {
  await line(editor, text).click();
  await expect.poll(() => editor.evaluate(() => document.querySelector('.cm-focus-point-dot')?.closest('.cm-lp-image-row')?.previousElementSibling?.textContent ?? null), { timeout: 10_000 }).toBe(text);
}
const pagePictures = (preview: Frame) => preview.evaluate(() =>
  [...document.querySelectorAll<HTMLImageElement>('img[src*="room"]')].map((i) => ({ loaded: i.complete && i.naturalWidth > 0, crop: i.style.objectPosition })));

async function shoot(page: Page, name: string, browserName: string): Promise<void> {
  const dir = process.env.SHOTS;
  if (!dir) return;
  mkdirSync(dir, { recursive: true });
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(dir, `${name}-${browserName}-${scheme}.png`) });
  }
  await page.emulateMedia({ colorScheme: 'light' });
}

test('placing the caret on a picture moves no line, and the dot carries no words', async ({ page, browserName }) => {
  const { editor } = await openEditor(page);
  await line(editor, 'Closing words.').click();
  const away = await lineTops(editor);
  await shoot(page, 'caret-away', browserName);

  for (const text of [EMBED, HERO]) {
    await select(editor, text);
    expect(await lineTops(editor), `caret on ${text}`).toEqual(away);
    expect(await editor.locator('.cm-focus-point').innerText()).toBe('');
    await shoot(page, text === EMBED ? 'caret-on-embed' : 'caret-on-hero', browserName);
  }
  await line(editor, 'Closing words.').click();
  await expect(dot(editor)).toHaveCount(0);
  expect(await lineTops(editor), 'caret gone from the pictures').toEqual(away);
  // The only text in the document is the page's own source.
  expect((await lineTops(editor)).map(([text]) => text).filter(Boolean)).toEqual(PAGE.split('\n').slice(3).filter(Boolean));
  expect(await editor.locator('.cm-content :not(.cm-line):not(.cm-line *)').allInnerTexts().then((t) => t.join('').trim())).toBe('');
});

for (const width of [900, 2400]) {
  test(`the caret moves no line through a six-picture grid in a ${width}px window`, async ({ page }) => {
    writeFileSync(join(site, 'index.md'), GALLERY);
    const { editor, preview } = await openEditor(page, { width, pictures: 6 });
    // The page answers for each picture's frame, so it has to be the gallery, not the page the site started with. A frame is read from the layout, so a picture below the fold need not have loaded.
    await expect.poll(() => pagePictures(preview).then((p) => p.length), { timeout: 60_000 }).toBe(6);
    await line(editor, 'Closing words.').click();
    const away = await lineTops(editor);
    for (const cell of CELLS) {
      // The grid crops every picture to a square, so each cell gets its dot.
      await select(editor, cell);
      expect(await lineTops(editor), `caret on ${cell}`).toEqual(away);
    }
  });
}

test('dragging the dot writes a position the build reads, and the picture stays on the page', async ({ page, browserName }) => {
  const { editor, preview } = await openEditor(page);
  await select(editor, EMBED);
  const from = (await dot(editor).boundingBox())!;
  const pic = (await editor.locator('.cm-focus-point').boundingBox())!;
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(pic.x + pic.width * 0.3, pic.y + pic.height * 0.8, { steps: 6 });
  await shoot(page, 'dragging', browserName);
  await page.mouse.up();

  await expect.poll(() => readFileSync(join(site, 'index.md'), 'utf8'), { timeout: 15_000 }).toMatch(/!\[A room\|focus:\d+%,\d+%\|\]\(room\.png\)/);
  const written = /focus:(\d+)%,(\d+)%/.exec(readFileSync(join(site, 'index.md'), 'utf8'))!;
  await expect.poll(() => pagePictures(preview), { timeout: 60_000 }).toContainEqual({ loaded: true, crop: `${written[1]}% ${written[2]}%` });
  // The rebuilt page has had time to replace the picture: both are still there, and loaded.
  await page.waitForTimeout(1500);
  const after = await pagePictures(preview);
  expect(after).toHaveLength(2);
  expect(after.every((p) => p.loaded), JSON.stringify(after)).toBe(true);
  expect(await lineTops(editor).then((t) => t.length)).toBeGreaterThan(8);
});

test('the keyboard reaches the dot, moves it, and writes on leaving it', async ({ page }) => {
  const { editor } = await openEditor(page);
  await select(editor, HERO);
  await page.keyboard.press('Alt+Enter');
  await expect(dot(editor)).toBeFocused();
  // A visible ring: the focused dot is outlined.
  expect(await dot(editor).evaluate((el) => getComputedStyle(el).outlineStyle)).not.toBe('none');
  for (let i = 0; i < 3; i++) await page.keyboard.press('Shift+ArrowLeft');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await expect.poll(() => readFileSync(join(site, 'index.md'), 'utf8'), { timeout: 15_000 }).toContain(':::hero {image="art/room.png|focus:20%,51%" max-height=240px}');
});

for (const text of [EMBED, HERO]) {
  test(`resting the pointer on the path of a drawn picture opens no card: ${text}`, async ({ page }) => {
    const { editor } = await openEditor(page);
    const row = line(editor, text);
    const x = await row.evaluate((el) => {
      const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        const at = n.textContent!.indexOf('room.png');
        if (at < 0) continue;
        const range = document.createRange();
        range.setStart(n, at + 2);
        range.setEnd(n, at + 3);
        return range.getBoundingClientRect().left - el.getBoundingClientRect().left;
      }
      return -1;
    });
    expect(x).toBeGreaterThan(0);
    await row.hover({ position: { x: x + 2, y: 8 } });
    await page.mouse.move(0, 0, { steps: 1 });
    await row.hover({ position: { x: x + 3, y: 8 } });
    await page.waitForTimeout(800);
    expect(await editor.locator('.cm-reference-card').count()).toBe(0);
    // An embed's path does have a card: the key opens it at the same spot. A hero's picture has none.
    if (text !== EMBED) return;
    await row.click({ position: { x: x + 3, y: 8 } });
    await page.keyboard.press('ControlOrMeta+k');
    await expect(editor.locator('.cm-reference-card')).toHaveCount(1);
  });
}
