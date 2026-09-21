import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";

/**
 * Render gate for `![[clip.mp4|loop]]` ambient video feature.
 * Self-contained: injects the compiled theme.js + minimal ambient CSS via
 * page.setContent + addStyleTag. No dev/vite webServer needed.
 *
 *   npx playwright test -c playwright/ambient-video.config.ts
 *
 * Catches JS/CSS behaviour that jsdom cannot prove:
 *   - wrapper injection, toggle presence, aria-label state
 *   - prefers-reduced-motion: reduce guard removes autoplay + shows toggle
 */
export default defineConfig({
  testDir: "../tests/render-gates/site",
  testMatch: /ambient-video\.spec\.ts$/,
  fullyParallel: true,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-ambient-video",
  use: {
    ...devices["Desktop Chrome"],
  },
});
