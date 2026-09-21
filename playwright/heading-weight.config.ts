/**
 * Render gate for the --moss-font-heading-weight token.
 *
 * Runs tests/render-gates/cascade/heading-weight.spec.ts in BOTH chromium and
 * webkit, asserting that headings compute to the fixture's 440 — a value no
 * hardcoded default uses, so a match proves the token drives the property.
 *
 * These assertions are only provable by a real browser render — jsdom is blind
 * to @layer cascade precedence.
 *
 * The scratch site is scaffolded and built by `buildScratchSite` below, at
 * config-parse time. That is deliberate and not a `globalSetup`: playwright
 * starts `webServer` BEFORE `globalSetup`, so a site built in `globalSetup`
 * does not exist yet when `webServer.cwd` is needed. See
 * tests/e2e/helpers/scratch-site.ts.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/heading-weight.config.ts
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";
import { buildScratchSite } from "../tests/e2e/helpers/scratch-site";
import { HEADING_WEIGHT_GATE } from "../tests/e2e/helpers/gate-sites";

const serveDir = buildScratchSite(HEADING_WEIGHT_GATE);

export default defineConfig({
  testDir: "../tests/render-gates/cascade",
  testMatch: /heading-weight\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-heading-weight-gate",
  use: {
    baseURL: "http://localhost:8745/",
    colorScheme: "light"
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `/usr/bin/python3 -m http.server 8745`,
    cwd: serveDir,
    url: "http://localhost:8745/",
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
