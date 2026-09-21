/**
 * Render gate: a hero caption is readable below the image, and a captioned
 * hero shows its whole subject.
 *
 * Both are engine questions. The caption sits next to a section with
 * `overflow: hidden` and a height cap, so "is it visible and below the photo"
 * cannot be answered from emitted text; and the crop is `object-fit`, which
 * only exists once something lays the image out.
 *
 *   npx playwright test -c playwright/hero-caption.config.ts
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli (or MOSS_BIN).
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";
import { buildScratchSite } from "../tests/e2e/helpers/scratch-site";
import { HERO_CAPTION_GATE } from "../tests/e2e/helpers/gate-sites";

const serveDir = buildScratchSite(HERO_CAPTION_GATE);

export default defineConfig({
  testDir: "../tests/render-gates/site",
  testMatch: /hero-caption\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-hero-caption",
  use: {
    baseURL: "http://localhost:8751/",
    colorScheme: "light",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `/usr/bin/python3 -m http.server 8751`,
    cwd: serveDir,
    url: "http://localhost:8751/",
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
