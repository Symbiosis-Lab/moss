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
 * element are what it can check).
 *
 * Ports: playwright/gate-ports.ts, keys `browser-editor:ui` /
 * `browser-editor:preview`. Config: playwright/browser-editor.config.ts.
 */
import { test, expect } from '@playwright/test';
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
  await editor.click('.cm-media-embed-card', { modifiers: ['ControlOrMeta'] });
  await editor.waitForSelector('embed.moss-file-viewer__pdf', { timeout: 15_000 });
  const pdf = await pdfResponse;
  expect(pdf.status()).toBe(200);
  expect((await pdf.allHeaders())['content-security-policy']).toMatch(/^sandbox;/);
  if (browserName === 'chromium') {
    await expect
      .poll(() => page.frames().some((f) => f.url().startsWith('chrome-extension://') && f.url().endsWith('/index.html')), {
        timeout: 15_000,
        message: 'the PDF viewer frame',
      })
      .toBe(true);
  }
});
