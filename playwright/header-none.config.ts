/**
 * Render gate for `[site].header = "none"`, the pinned island it brings, and
 * the contents ruler's scope (`contents=end` and full-width blocks).
 *
 * Runs tests/render-gates/site/header-none.spec.ts in BOTH chromium and webkit
 * against two scratch sites built by the moss CLI: where the island and the
 * ruler sit, what they overlap, and when each is on screen are all painted
 * layout.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/header-none.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { BAND_GATE, ONE_PAGE_GATE } from '../tests/e2e/helpers/header-none-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('header-none');

export default defineGateConfig({
  gate: 'header-none',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: [
    { serveDir: buildScratchSite(ONE_PAGE_GATE), port: PORT },
    { serveDir: buildScratchSite(BAND_GATE), port: gatePort('header-none:band') },
  ],
});
