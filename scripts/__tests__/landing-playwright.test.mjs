// loadPlaywright finds Playwright in this checkout on its own: `playwright`
// first, then `@playwright/test`, which re-exports the same browsers. Only
// PLAYWRIGHT_MODULE overrides it, and a miss names where it looked.
import { it } from 'node:test';
import assert from 'node:assert/strict';
import { copyFile, mkdir, mkdtemp, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

async function fixture(t) {
  const repo = await realpath(await mkdtemp(join(tmpdir(), 'playwright-resolver-')));
  t.after(() => rm(repo, { recursive: true, force: true }));
  await mkdir(join(repo, 'scripts'), { recursive: true });
  await copyFile(new URL('../landing-harness.mjs', import.meta.url), join(repo, 'scripts/landing-harness.mjs'));
  const loader = await import(pathToFileURL(join(repo, 'scripts/landing-harness.mjs')));
  const previous = process.env.PLAYWRIGHT_MODULE;
  t.after(() => {
    if (previous === undefined) delete process.env.PLAYWRIGHT_MODULE;
    else process.env.PLAYWRIGHT_MODULE = previous;
  });
  delete process.env.PLAYWRIGHT_MODULE;
  const install = async (name, chromium) => {
    const pkg = join(repo, 'node_modules', name);
    await mkdir(pkg, { recursive: true });
    await writeFile(join(pkg, 'package.json'), JSON.stringify({ name, type: 'module', main: 'index.mjs' }));
    await writeFile(join(pkg, 'index.mjs'), `export const chromium = ${JSON.stringify(chromium)};`);
    return join(pkg, 'index.mjs');
  };
  return { repo, install, ...loader };
}

it('loads @playwright/test when the checkout has no playwright package', async t => {
  const f = await fixture(t);
  await f.install('@playwright/test', 'test-package');
  assert.equal((await f.loadPlaywright()).chromium, 'test-package');
});

it('prefers playwright over @playwright/test', async t => {
  const f = await fixture(t);
  await f.install('@playwright/test', 'test-package');
  await f.install('playwright', 'playwright');
  assert.equal((await f.loadPlaywright()).chromium, 'playwright');
});

it('uses PLAYWRIGHT_MODULE over the checkout', async t => {
  const f = await fixture(t);
  await f.install('playwright', 'checkout');
  const elsewhere = await realpath(await mkdtemp(join(tmpdir(), 'playwright-override-')));
  t.after(() => rm(elsewhere, { recursive: true, force: true }));
  await writeFile(join(elsewhere, 'index.mjs'), 'export const chromium = "override";');
  process.env.PLAYWRIGHT_MODULE = join(elsewhere, 'index.mjs');
  assert.equal((await f.loadPlaywright()).chromium, 'override');
});

it('names the checkout it searched when nothing is installed', async t => {
  const f = await fixture(t);
  await assert.rejects(f.loadPlaywright(), error => {
    assert.match(error.message, /searched/);
    assert.ok(error.message.includes(f.repo), error.message);
    assert.match(error.message, /PLAYWRIGHT_MODULE/);
    return true;
  });
});
