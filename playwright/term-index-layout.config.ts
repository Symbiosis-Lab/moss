/**
 * Render gate: the INDEX state — a listing grid whose cards carry no cover.
 *
 * A generated term root (`/authors/`, `/tags/`) is a listing of bare labels.
 * Only a real engine can answer whether a name broke mid-word, whether the
 * cover placeholder still occupies height, or how many tracks the container
 * realized — and CJK glyph advance differs between the two engines, which is
 * precisely what decides a wrap. Both are run for that reason.
 *
 *   npx playwright test -c playwright/term-index-layout.config.ts
 *
 * No binary and no scratch site: the spec injects the branch's real site.css
 * straight into `page.setContent`.
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "../tests/render-gates/site",
  testMatch: /term-index-layout\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-term-index-layout",
  use: { colorScheme: "light" },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
});
