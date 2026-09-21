/**
 * Render gate for the narrow-viewport (<20rem) hamburger nav.
 *
 * Runs tests/render-gates/site/nav-mobile.spec.ts in BOTH chromium and webkit
 * against a scratch site with SIX nav pages. The count is the gate: the menu
 * opened to a fixed `max-height: 200px`, and `.nav-links` inherits
 * `flex-wrap: wrap` from the desktop rule, so a bounded column flex container
 * wrapped its links into extra COLUMNS instead of growing — two columns of
 * three at six links, four columns at ten, and in webkit twelve pushed labels
 * past the right edge where `overflow: hidden` deleted them. Only a layout
 * engine can answer "how many columns is this menu", so this cannot move down
 * to a Rust test; what CAN be answered from emitted text is asserted in
 * `test_css_mobile_nav_links_are_a_single_column`.
 *
 * The spec previously existed with no config at all and a hard-coded
 * `localhost:8765`, so it ran only when someone started a server by hand — it
 * had never run on a PR, which is why the defect survived it.
 *
 * The scratch site is scaffolded and built by `buildScratchSite` at
 * config-parse time, not in `globalSetup`: playwright starts `webServer` first.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/nav-mobile.config.ts
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";
import { buildScratchSite } from "../tests/e2e/helpers/scratch-site";
import { NAV_MOBILE_GATE } from "../tests/e2e/helpers/gate-sites";

const serveDir = buildScratchSite(NAV_MOBILE_GATE);

export default defineConfig({
  testDir: "../tests/render-gates/site",
  testMatch: /nav-mobile\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-nav-mobile",
  use: {
    baseURL: "http://localhost:9377/",
    colorScheme: "light",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `/usr/bin/python3 -m http.server 9377`,
    cwd: serveDir,
    url: "http://localhost:9377/",
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
