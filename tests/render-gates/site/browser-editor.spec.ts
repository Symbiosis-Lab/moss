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
 * Ports: playwright/gate-ports.ts, keys `browser-editor:ui` /
 * `browser-editor:preview`. Config: playwright/browser-editor.config.ts.
 */
import { test, expect, type Frame } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gatePort } from '../../../playwright/gate-ports';
import { MOSS_BIN } from '../../e2e/helpers/scratch-site';

const ONE_PAGE_PDF =
  '%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n' +
  '3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n';

let site = '';
let home = '';
let server: ChildProcess | undefined;
let output = '';

test.beforeAll(async () => {
  site = realpathSync(mkdtempSync(join(tmpdir(), 'moss-browser-editor-site-')));
  home = realpathSync(mkdtempSync(join(tmpdir(), 'moss-browser-editor-home-')));
  mkdirSync(join(home, 'moss'));
  writeFileSync(join(site, 'index.md'), '---\ntitle: Garden\n---\nHello garden.\n\n![[doc.pdf]]\n');
  writeFileSync(join(site, 'doc.pdf'), ONE_PAGE_PDF);
  // MOSS_HOME and the search root keep the binary from finding, or handing
  // the folder to, an installed desktop app.
  server = spawn(MOSS_BIN, ['edit', site, '--serve', '--no-browser', '--port', String(gatePort('browser-editor:ui'))], {
    env: {
      ...process.env,
      MOSS_HOME: join(home, 'moss'),
      MOSS_DESKTOP_SEARCH_ROOT: home,
      MOSS_PREVIEW_PORT_BASE: String(gatePort('browser-editor:preview')),
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  server.stdout?.on('data', (b) => (output += b));
  server.stderr?.on('data', (b) => (output += b));
});

test.afterAll(async () => {
  if (server && server.exitCode === null) {
    server.kill('SIGTERM');
    await new Promise((r) => server!.once('exit', r));
  }
  rmSync(site, { recursive: true, force: true });
  rmSync(home, { recursive: true, force: true });
});

test('the browser editor boots, saves, and opens a sandboxed PDF in the file viewer', async ({ page, browserName }) => {
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
});
