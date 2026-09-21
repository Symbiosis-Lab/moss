/**
 * Render gate: `:::grid` collapses to one column on mobile, ratio grids
 * included, and a grid cell presents the same image box whichever syntax the
 * author used.
 *
 * Only a real engine can answer either question. A ratio grid used to carry an
 * inline `style="grid-template-columns:…"`, which no stylesheet rule can
 * override — the markup was "correct" by every text assertion and the page
 * still showed two columns on a phone.
 *
 *   npx playwright test -c playwright/grid-mobile-collapse.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";
import { buildScratchSite } from "../tests/e2e/helpers/scratch-site";
import { GRID_MOBILE_COLLAPSE_GATE } from "../tests/e2e/helpers/gate-sites";

const serveDir = buildScratchSite(GRID_MOBILE_COLLAPSE_GATE);

export default defineConfig({
  testDir: "../tests/render-gates/site",
  testMatch: /grid-mobile-collapse\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-grid-mobile-collapse",
  use: {
    baseURL: "http://localhost:8749/",
    colorScheme: "light",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `/usr/bin/python3 -m http.server 8749`,
    cwd: serveDir,
    url: "http://localhost:8749/",
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
