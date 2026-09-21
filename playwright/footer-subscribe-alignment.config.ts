/**
 * Playwright config for the footer subscribe-form alignment gate
 * (tests/render-gates/site/footer-subscribe-alignment.spec.ts).
 *
 * Stylesheet-injection gate: the spec inlines site.css, vertical.css and
 * email.css into `page.setContent` and reads real boxes. No webServer, no
 * `moss build`, no MOSS_BIN. Both engines, because the shipping preview is a
 * WKWebView and vertical writing modes are where engines historically
 * disagree.
 *
 * Run via: pnpm run test:render-gates footer-subscribe-alignment
 * Test-artifact policy: outputDir inside repo under target/test-tmp/ (gitignored).
 */
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: '../tests/render-gates/site',
  testMatch: /footer-subscribe-alignment\.spec\.ts$/,
  fullyParallel: true,
  reporter: 'list',
  outputDir: '../target/test-tmp/playwright-footer-subscribe-alignment',
  use: {
    reducedMotion: 'reduce',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
});
