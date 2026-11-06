/**
 * Render gate for `[site].header_scroll = "hide"`: the header slides away while
 * the reader scrolls down and returns on any scroll up, stays under reduced
 * motion or focus, and leaves an anchored heading uncovered.
 *
 * Runs tests/render-gates/site/header-scroll.spec.ts in BOTH chromium and
 * webkit against a scratch site built by the moss CLI.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/header-scroll.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { HEADER_SCROLL_GATE } from '../tests/e2e/helpers/header-scroll-site';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('header-scroll');

export default defineGateConfig({
  gate: 'header-scroll',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: [{ serveDir: buildScratchSite(HEADER_SCROLL_GATE), port: PORT }],
});
