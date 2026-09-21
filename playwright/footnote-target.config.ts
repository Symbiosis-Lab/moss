/**
 * Render gate: the footnote landing fix (docs/archive/2026-08-24-footnote-
 * landing-fix-plan.md) — `:target` highlight wash + `scroll-padding-top`
 * island clearance.
 *
 * Both claims need a real engine: whether the endnote `<li>` clears the
 * floating nav island is a live-layout question (`toBeInViewport` plus a
 * bounding-box comparison), and whether a `:target` selector actually paints
 * a non-transparent `background-color` — animated or, under reduced motion,
 * static — is a computed-style question jsdom cannot answer (no cascade, no
 * animation model).
 *
 *   npx playwright test -c playwright/footnote-target.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";
import { buildScratchSite } from "../tests/e2e/helpers/scratch-site";
import { FOOTNOTE_TARGET_GATE } from "../tests/e2e/helpers/gate-sites";

const serveDir = buildScratchSite(FOOTNOTE_TARGET_GATE);

export default defineConfig({
  testDir: "../tests/render-gates/site",
  testMatch: /footnote-target\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-footnote-target",
  use: {
    baseURL: "http://localhost:8761/",
    colorScheme: "light",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `/usr/bin/python3 -m http.server 8761`,
    cwd: serveDir,
    url: "http://localhost:8761/",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
