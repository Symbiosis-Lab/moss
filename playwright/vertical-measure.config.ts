/**
 * Playwright config for the vertical-rl measure gate
 * (tests/render-gates/site/vertical-measure.spec.ts).
 *
 * Stylesheet-injection gate: the spec inlines site.css and vertical.css into
 * `page.setContent` and reads real boxes. No webServer, no `moss build`, no
 * MOSS_BIN. Both engines, because the shipping preview is a WKWebView and
 * vertical writing modes are where engines historically disagree.
 *
 * Run via: pnpm run test:render-gates vertical-measure
 * Test-artifact policy: outputDir inside repo under target/test-tmp/ (gitignored).
 */
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: '../tests/render-gates/site',
  testMatch: /vertical-measure\.spec\.ts$/,
  fullyParallel: true,
  reporter: 'list',
  outputDir: '../target/test-tmp/playwright-vertical-measure',
  use: {
    reducedMotion: 'reduce',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
});
