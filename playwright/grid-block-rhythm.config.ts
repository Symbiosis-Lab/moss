/**
 * Playwright config for the grid block-rhythm gate
 * (tests/render-gates/site/grid-block-rhythm.spec.ts).
 *
 * Stylesheet-injection gate: the spec inlines site.css into `page.setContent`
 * and reads real computed margins. No webServer, no `moss build`, no MOSS_BIN.
 * Both engines: margin-collapse and @layer precedence are invisible to jsdom.
 *
 * Run via: pnpm run test:render-gates grid-block-rhythm
 * Test-artifact policy: outputDir inside repo under target/test-tmp/ (gitignored).
 */
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: '../tests/render-gates/site',
  testMatch: /grid-block-rhythm\.spec\.ts$/,
  fullyParallel: true,
  reporter: 'list',
  outputDir: '../target/test-tmp/playwright-grid-block-rhythm',
  use: {
    reducedMotion: 'reduce',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
});
