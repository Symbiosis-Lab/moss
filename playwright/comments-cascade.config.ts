/**
 * Render gate for the native feature CSS @layer cascade contract.
 *
 * Runs tests/render-gates/cascade/comments-cascade.spec.ts in BOTH chromium and
 * webkit. The spec asserts:
 *   1. .comment-author computed color == rgb(0, 128, 0)
 *      (the user's @layer themes rule beats comments.css in @layer plugins)
 *   2. the .moss-comments element still renders after the layer wrapping
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
 *   npx playwright test -c playwright/comments-cascade.config.ts
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";
import { buildScratchSite } from "../tests/e2e/helpers/scratch-site";
import { COMMENTS_CASCADE_GATE } from "../tests/e2e/helpers/gate-sites";

const serveDir = buildScratchSite(COMMENTS_CASCADE_GATE);

export default defineConfig({
  testDir: "../tests/render-gates/cascade",
  testMatch: /comments-cascade\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-comments-cascade",
  use: {
    baseURL: "http://localhost:8744/",
    colorScheme: "light"
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `/usr/bin/python3 -m http.server 8744`,
    cwd: serveDir,
    url: "http://localhost:8744/",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
