/**
 * Render gate for the nav toggle cluster (search / language / theme).
 *
 * Runs tests/render-gates/site/nav-toggle-cluster.spec.ts in BOTH chromium and
 * webkit against a scratch site that has search on and a translation, so all
 * three toggles are in `.nav-icons`.
 *
 * The scratch site is scaffolded and built by `buildScratchSite` below, at
 * config-parse time. That is deliberate and not a `globalSetup`: playwright
 * starts `webServer` BEFORE `globalSetup`, so a site built in `globalSetup`
 * does not exist yet when `webServer.cwd` is needed. See
 * tests/e2e/helpers/scratch-site.ts.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/nav-toggle-cluster.config.ts
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";
import { buildScratchSite } from "../tests/e2e/helpers/scratch-site";
import { NAV_TOGGLE_CLUSTER_GATE } from "../tests/e2e/helpers/gate-sites";

const serveDir = buildScratchSite(NAV_TOGGLE_CLUSTER_GATE);

export default defineConfig({
  testDir: "../tests/render-gates/site",
  testMatch: /nav-toggle-cluster\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-nav-toggle-cluster",
  use: {
    baseURL: "http://localhost:9376/",
    colorScheme: "light", // pin light so dark-mode tokens do not interfere
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `/usr/bin/python3 -m http.server 9376`,
    cwd: serveDir,
    url: "http://localhost:9376/",
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
