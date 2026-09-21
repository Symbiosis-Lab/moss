/**
 * Render gate for Task 2.3: the pre-paint data-theme script.
 *
 * Runs tests/render-gates/cascade/pre-paint-dark.spec.ts in BOTH chromium and
 * webkit.
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
 *   npx playwright test -c playwright/pre-paint-dark.config.ts
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";
import { buildScratchSite } from "../tests/e2e/helpers/scratch-site";
import { PRE_PAINT_DARK_GATE } from "../tests/e2e/helpers/gate-sites";

const serveDir = buildScratchSite(PRE_PAINT_DARK_GATE);

export default defineConfig({
  testDir: "../tests/render-gates/cascade",
  testMatch: /pre-paint-dark\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-pre-paint-dark-gate",
  use: {
    baseURL: "http://localhost:9371/",
    colorScheme: "dark",
    // A dark-preference OS with empty localStorage: the pre-paint script must
    // read matchMedia("(prefers-color-scheme: dark)") and set data-theme="dark"
    // before first paint. The fixture has no @media dark block, so nothing else
    // can produce the dark background.
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `/usr/bin/python3 -m http.server 9371`,
    cwd: serveDir,
    url: "http://localhost:9371/",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
