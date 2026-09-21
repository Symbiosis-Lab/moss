/**
 * Playwright config for the floating nav island's LAYOUT guard (ADR-049).
 *
 * Two of the island's promises are geometry, and geometry is the one thing
 * neither a Rust test nor jsdom can answer:
 *
 *   1. The trail is ONE line at any width — it never wraps and never overflows.
 *      That is what the measured fold buys, and a breakpoint would only appear
 *      to buy.
 *   2. Every label in the sections panel starts at the same x, current row or
 *      not. The first attempt marked the current row with an in-flow `content:
 *      "●"`, which shifted its own label sideways; the gutter rule is the fix
 *      and this is the assertion that would have caught the bug.
 *
 * Runs in BOTH chromium and webkit: moss sites are read on Safari and on iOS,
 * and the trail leans on `:has()`, `min()` inside `calc()`, and flex baseline
 * alignment, all of which have engine-specific history.
 *
 * Harness: playwright/fixtures/nav-island/live-css.html — production site.css
 * plus the production `nav-island.ts`, with the island markup the Rust emitter
 * produces (pinned by `nav_island_harness_matches_the_emitter`).
 *
 * Run via: pnpm run test:render-gates nav-island
 * Test-artifact policy: outputDir inside repo under target/test-tmp/ (gitignored).
 */
import './localhost-no-proxy';
import { defineConfig, devices } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export default defineConfig({
  testDir: '../tests/render-gates/site',
  testMatch: /nav-island\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: 'list',
  outputDir: '../target/test-tmp/playwright-nav-island',
  use: {
    baseURL: 'http://localhost:5404',
    reducedMotion: 'reduce',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
  webServer: {
    command: 'npx vite --config playwright/vite.nav-island-harness.config.ts',
    cwd: repoRoot,
    port: 5404,
    reuseExistingServer: false,
    timeout: 60000,
  },
});
