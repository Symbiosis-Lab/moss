/**
 * Render gate for the masthead's first paint.
 *
 * Runs tests/render-gates/site/nav-first-paint.spec.ts in BOTH chromium and
 * webkit against a scratch site with one link, a short name, a long trail page
 * and a short one. Whether a row fits is a question of measured widths, so only
 * a real layout engine can say whether the masthead moved.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/nav-first-paint.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NAV_FIRST_PAINT_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('nav-first-paint');
const serveDir = buildScratchSite(NAV_FIRST_PAINT_GATE);

export default defineGateConfig({
  gate: 'nav-first-paint',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light',
  },
  webServer: { serveDir, port: PORT },
});
