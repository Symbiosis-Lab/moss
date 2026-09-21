/**
 * Playwright config for the nav chrome under vertical writing — the font
 * selector morph (theme.ts `initFontPanel`) and the masthead breadcrumb fold
 * (nav/masthead-fold.ts + nav/breadcrumb-fold.ts), both of which measure or
 * position along a physical axis that vertical-rl rotates.
 *
 * Runs in BOTH chromium and webkit for the same reason nav-island's config
 * does: vertical-writing sites are read on Safari, and getComputedStyle's
 * writing-mode resolution plus flex alignment for out-of-flow children have
 * engine-specific history.
 *
 * Harness: playwright/fixtures/vertical-nav-chrome/live-css.html —
 * production site.css + vertical.css + theme.ts, under
 * body[data-typesetting="vertical"].
 *
 * Run via: pnpm run test:render-gates vertical-nav-chrome
 */
import './localhost-no-proxy';
import { defineConfig, devices } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export default defineConfig({
  testDir: '../tests/render-gates/site',
  testMatch: /vertical-nav-chrome\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: 'list',
  outputDir: '../target/test-tmp/playwright-vertical-nav-chrome',
  use: {
    baseURL: 'http://localhost:5405',
    reducedMotion: 'reduce',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
  webServer: {
    command: 'npx vite --config playwright/vite.vertical-nav-chrome-harness.config.ts',
    cwd: repoRoot,
    port: 5405,
    reuseExistingServer: false,
    timeout: 60000,
  },
});
