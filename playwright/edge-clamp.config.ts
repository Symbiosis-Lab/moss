/**
 * Playwright config for the edge-clamp gate.
 *
 * moss floats three small surfaces over a page — the selection popover, the
 * link-preview card, and the nav hover hint — and each is positioned in JS from
 * an anchor's rect. Whether the result is actually on screen is a question only
 * an engine can answer: jsdom returns zero for every rect and every offset
 * dimension, so a unit test can pin the arithmetic (and
 * `crates/moss-build/src/js-src/site/__tests__/viewport.test.ts` does) but
 * cannot notice that the arithmetic was fed a 0×0 box. That is not
 * hypothetical — it is how a popover
 * that ran off the left edge of a phone shipped under a green suite.
 *
 * Runs in BOTH chromium and webkit, and at BOTH a phone width and a desktop
 * width, because the failures are width-dependent by nature and moss sites are
 * read on Safari and iOS.
 *
 * Harness: playwright/fixtures/edge-clamp/live-css.html — production site.css
 * plus the production placement modules, served through vite.
 *
 * Run via: pnpm run test:render-gates edge-clamp
 * Test-artifact policy: outputDir inside repo under target/test-tmp/ (gitignored).
 */
import './localhost-no-proxy';
import { defineConfig, devices } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export default defineConfig({
  testDir: '../tests/render-gates/site',
  testMatch: /edge-clamp\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: 'list',
  outputDir: '../target/test-tmp/playwright-edge-clamp',
  use: {
    baseURL: 'http://localhost:5405',
    reducedMotion: 'reduce',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
  webServer: {
    command: 'npx vite --config playwright/vite.edge-clamp-harness.config.ts',
    cwd: repoRoot,
    port: 5405,
    reuseExistingServer: !process.env.CI,
    timeout: 60000,
  },
});
