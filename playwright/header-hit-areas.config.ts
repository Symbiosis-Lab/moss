/**
 * Render gate for the header controls' 44px hit areas and the reading-size
 * control's focus ring.
 *
 * Runs tests/render-gates/site/header-hit-areas.spec.ts in BOTH chromium and
 * webkit against a scratch site with search, a translation (so the toggle
 * cluster is full), nav links (for the hamburger), floating_nav (for the
 * island), and a dated, two-section article (for .font-trigger and
 * .moss-nav-island-sections).
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/header-hit-areas.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { HEADER_HIT_AREAS_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('header-hit-areas');
const serveDir = buildScratchSite(HEADER_HIT_AREAS_GATE);

export default defineGateConfig({
  gate: 'header-hit-areas',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light', // pin light; the dark-mode pass is a separate assertion inside the spec, via setTheme() (localStorage["moss-theme"] + reload)
  },
  webServer: { serveDir, port: PORT },
});
