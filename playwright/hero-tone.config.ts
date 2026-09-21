/**
 * Render gate: a pale hero drops the scrim and flips its text dark, and the
 * three hero layouts stay told apart.
 *
 * Engine questions throughout. `content: none` vs `content: ""` on a
 * `::before` is only observable once a browser has resolved the cascade, and
 * three of the four rules under test win their tie on source order rather than
 * specificity — jsdom implements neither.
 *
 *   npx playwright test -c playwright/hero-tone.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";
import { buildScratchSite } from "../tests/e2e/helpers/scratch-site";
import { HERO_TONE_GATE } from "../tests/e2e/helpers/gate-sites";

const serveDir = buildScratchSite(HERO_TONE_GATE);

export default defineConfig({
  testDir: "../tests/render-gates/site",
  testMatch: /hero-tone\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-hero-tone",
  use: {
    baseURL: "http://localhost:8757/",
    colorScheme: "light",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `/usr/bin/python3 -m http.server 8757`,
    cwd: serveDir,
    url: "http://localhost:8757/",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
