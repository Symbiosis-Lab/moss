/**
 * Render gate for notebook loading: the built site's JupyterLite must be able
 * to open the site's notebook.
 *
 * Runs tests/render-gates/site/notebook-loads.spec.ts against a scratch site
 * containing one .ipynb. Building it downloads the pinned JupyterLite bundle
 * (network, ~6 MB, cached under ~/.moss/assets/), so the first run on a
 * machine is slower than the rest.
 *
 * Chromium only, deliberately: the claim is "the app fetches moss's contents
 * index and renders the cells", which is bundle+config behaviour, not a
 * cascade — one engine answers it, and JupyterLite's boot is the expensive
 * part of the gate.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/notebook-loads.config.ts
 */
import './localhost-no-proxy';
import { defineConfig, devices } from "@playwright/test";
import { buildScratchSite } from "../tests/e2e/helpers/scratch-site";
import { NOTEBOOK_GATE } from "../tests/e2e/helpers/gate-sites";

const serveDir = buildScratchSite(NOTEBOOK_GATE);

export default defineConfig({
  testDir: "../tests/render-gates/site",
  testMatch: /notebook-loads\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  outputDir: "../target/test-tmp/playwright-notebook-loads",
  timeout: 120_000,
  use: {
    baseURL: "http://localhost:9378/",
    colorScheme: "light",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: `/usr/bin/python3 -m http.server 9378`,
    cwd: serveDir,
    url: "http://localhost:9378/",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
