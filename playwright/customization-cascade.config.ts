/**
 * Render gate for Task 2.1: @layer cascade contract verification.
 *
 * Runs tests/render-gates/cascade/customization-cascade.spec.ts in BOTH
 * chromium and webkit. The spec asserts:
 *   1. body computed background-color == rgb(171, 205, 239)  (#abcdef via @layer themes)
 *   2. .main-nav a computed color   == rgb(255, 0, 0)        (user CSS wins @layer themes)
 *   3. <link> for the user theme stylesheet carries layer="themes" in the DOM
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
 *   npx playwright test -c playwright/customization-cascade.config.ts
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";
import { buildScratchSite } from "../tests/e2e/helpers/scratch-site";
import { CASCADE_GATE } from "../tests/e2e/helpers/gate-sites";

const serveDir = buildScratchSite(CASCADE_GATE);

export default defineConfig({
  testDir: "../tests/render-gates/cascade",
  testMatch: /customization-cascade\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-cascade-gate",
  use: {
    baseURL: "http://localhost:8743/",
    colorScheme: "light", // pin light so dark-mode :root vars do not interfere
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `/usr/bin/python3 -m http.server 8743`,
    cwd: serveDir,
    url: "http://localhost:8743/",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
