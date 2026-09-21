/**
 * Render gate: `.moss-card-cover`'s default aspect-ratio is landscape (4/3),
 * not the phone-only portrait value (3/4) that leaked into the unscoped base
 * rule at a9db42542d6 (2026-09-15). See the spec header for the full story.
 *
 *   npx playwright test -c playwright/card-cover-ratio.config.ts
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
  testMatch: /card-cover-ratio\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-card-cover-ratio",
  use: {
    baseURL: "http://localhost:8798/",
    colorScheme: "light",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `/usr/bin/python3 -m http.server 8798`,
    cwd: serveDir,
    url: "http://localhost:8798/",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
