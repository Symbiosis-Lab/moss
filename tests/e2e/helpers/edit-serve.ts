/**
 * One `moss edit --serve` a test, for the render gates that drive the browser
 * editor: a scratch site and home, the server, its sign-in link, and the frames
 * the shell nests. Ports come from playwright/gate-ports.ts, keys
 * `<gate>:ui` and `<gate>:preview`.
 */
import { test, expect, type Frame, type Page, type TestInfo } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gatePort } from '../../../playwright/gate-ports';
import { MOSS_BIN } from './scratch-site';

export interface EditServe {
  readonly site: string;
  readonly home: string;
  /** Everything the server has printed so far. */
  readonly output: string;
  /** The site's build output, outside the folder: `MossPaths::build_dir` for a folder under the temporary directory. */
  readonly buildDir: string;
}

/** Registers the per-test server; `writeSite` fills the site folder before it starts. */
export function useEditServe(gate: string, writeSite: (site: string, testInfo: TestInfo) => void, env: Record<string, string> = {}): EditServe {
  const state = {
    site: '', home: '', output: '',
    get buildDir() { return join(tmpdir(), 'moss-folder-stores', 'caches', createHash('sha256').update(this.site).digest('hex'), 'build'); },
  };
  let server: ChildProcess | undefined;

  test.beforeEach(async ({}, testInfo) => {
    state.output = '';
    state.site = realpathSync(mkdtempSync(join(tmpdir(), `moss-${gate}-site-`)));
    state.home = realpathSync(mkdtempSync(join(tmpdir(), `moss-${gate}-home-`)));
    mkdirSync(join(state.home, 'moss'));
    writeSite(state.site, testInfo);
    // MOSS_HOME and the search root keep the binary from finding, or handing
    // the folder to, an installed desktop app. TMPDIR is Node's: with it unset,
    // Rust's temporary directory is another one, so moss would not see the site
    // as temporary and would keep its stores in the real app data.
    server = spawn(MOSS_BIN, ['edit', state.site, '--serve', '--no-browser', '--port', String(gatePort(`${gate}:ui`))], {
      env: {
        ...process.env,
        MOSS_HOME: join(state.home, 'moss'),
        MOSS_DESKTOP_SEARCH_ROOT: state.home,
        MOSS_PREVIEW_PORT_BASE: String(gatePort(`${gate}:preview`)),
        TMPDIR: tmpdir(),
        ...env,
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    server.stdout?.on('data', (b) => (state.output += b));
    server.stderr?.on('data', (b) => (state.output += b));
  });

  test.afterEach(async () => {
    if (server && server.exitCode === null) {
      server.kill('SIGTERM');
      await new Promise((r) => server!.once('exit', r));
    }
    rmSync(state.site, { recursive: true, force: true });
    rmSync(state.home, { recursive: true, force: true });
  });

  return state;
}

/** The frame of the iframe `#id` in `from`; the editor's URL gains the open page as a query, so frames are taken from their elements. */
export async function frameOf(from: Page | Frame, id: string): Promise<Frame> {
  return (await (await from.waitForSelector(`#${id}`, { state: 'attached', timeout: 15_000 })).contentFrame())!;
}

/** Sign in, and wait for the page showing `ready` in the preview. */
export async function openShell(page: Page, serve: EditServe, ready: string, viewport?: { width: number; height: number }): Promise<{ shell: Frame; preview: Frame }> {
  if (viewport) await page.setViewportSize(viewport);
  await expect.poll(() => serve.output.match(/Edit at (\S+)/)?.[1], { timeout: 60_000, message: `the sign-in link; server said:\n${serve.output}` }).toBeTruthy();
  await page.goto(serve.output.match(/Edit at (\S+)/)![1]);
  await page.waitForURL((u) => u.pathname === '/', { timeout: 15_000 });
  const shell = await frameOf(page, 'moss-shell-frame');
  const preview = await frameOf(shell, 'moss-preview-iframe');
  await preview.waitForFunction((text) => document.body?.innerText.includes(text), ready, { timeout: 30_000 });
  return { shell, preview };
}

/** `openShell`, then the editor opened beside the preview. */
export async function openEditor(page: Page, serve: EditServe, ready: string, viewport?: { width: number; height: number }): Promise<{ shell: Frame; editor: Frame; preview: Frame }> {
  const { shell, preview } = await openShell(page, serve, ready, viewport);
  const editor = await frameOf(page, 'moss-editor-frame');
  await shell.click('#moss-edit-button');
  await expect(page.locator('#moss-frames')).toHaveAttribute('data-editor', 'split', { timeout: 10_000 });
  return { shell, editor, preview };
}
