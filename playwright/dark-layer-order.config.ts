/**
 * Render gate for Task 2.4: dark tokens are beaten by layer order.
 *
 * Runs tests/render-gates/cascade/dark-layer-order.spec.ts in BOTH chromium and
 * webkit. The author's [data-theme="dark"] override and moss's tokens-layer dark
 * block have identical specificity, so only layer order can produce the author
 * win the spec asserts (background rgb(17, 17, 17)).
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
 *   npx playwright test -c playwright/dark-layer-order.config.ts
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";
import { buildScratchSite } from "../tests/e2e/helpers/scratch-site";
import { DARK_LAYER_ORDER_GATE } from "../tests/e2e/helpers/gate-sites";

const serveDir = buildScratchSite(DARK_LAYER_ORDER_GATE);

export default defineConfig({
  testDir: "../tests/render-gates/cascade",
  testMatch: /dark-layer-order\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-dark-layer-order-gate",
  use: {
    baseURL: "http://localhost:9372/",
    colorScheme: "dark",
    // The spec ALSO sets localStorage["moss-theme"]="dark" (the explicit
    // toggle), which the pre-paint script prefers. Matching colorScheme here
    // just keeps the system preference from fighting the assertion.
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `/usr/bin/python3 -m http.server 9372`,
    cwd: serveDir,
    url: "http://localhost:9372/",
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
