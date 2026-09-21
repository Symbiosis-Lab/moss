/**
 * Render gate: the media track of a listing grid.
 *
 * A card's cover slot is sized by the grid ROW under `@supports
 * (grid-template-rows: subgrid)`, and by a per-card placeholder div without
 * it. Two live behaviours means two things to prove, and only a real engine
 * can answer either — subgrid track sizing is not something jsdom models at
 * all, and the fallback path is a computed-style question about a rule that
 * an `@supports` block may or may not have overridden.
 *
 *   npx playwright test -c playwright/card-media-track.config.ts
 *
 * No binary and no scratch site: the spec injects the branch's real site.css
 * straight into `page.setContent`, and simulates the unsupported engine by
 * stripping the `@supports` block from that same stylesheet — so the fallback
 * assertions are made against the real rules, not a hand-written copy.
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "../tests/render-gates/site",
  testMatch: /card-media-track\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-card-media-track",
  use: { colorScheme: "light" },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
});
