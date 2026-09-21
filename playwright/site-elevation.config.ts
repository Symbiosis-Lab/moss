/**
 * Render gate: the emitted site's elevation model — one declared light.
 *
 * Every floating surface reads a `--moss-elevation-N` token; content casts
 * nothing; turning the light or the theme moves the cast through the inputs,
 * never through a per-surface literal. Only an engine can compute a shadow
 * through calc() and color-mix(), and both are run because they serialize
 * computed shadows differently — the spec compares like with like inside one
 * engine, never a string it wrote itself.
 *
 *   npx playwright test -c playwright/site-elevation.config.ts
 *
 * No binary and no scratch site: the spec injects the branch's real site.css,
 * behind a token block built from tokens.json, straight into `page.setContent`.
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "../tests/render-gates/site",
  testMatch: /elevation\.spec\.js$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-site-elevation",
  use: { colorScheme: "light" },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
});
