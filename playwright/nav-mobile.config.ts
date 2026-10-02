/**
 * Render gate for the narrow-viewport (<20rem) hamburger nav.
 *
 * Runs tests/render-gates/site/nav-mobile.spec.ts in BOTH chromium and webkit
 * against a scratch site with SIX nav pages. The count is the gate: the menu
 * is a bounded column flex container and `.nav-links` inherits
 * `flex-wrap: wrap` from the desktop rule, so with enough links it can wrap
 * them into extra COLUMNS instead of growing, pushing labels past the edge
 * where `overflow: hidden` deletes them. Only a layout engine can answer "how
 * many columns is this menu", so this cannot move down to a Rust test; what
 * CAN be answered from emitted text is asserted in
 * `test_css_mobile_nav_links_are_a_single_column`.
 *
 * The scratch site is scaffolded and built by `buildScratchSite` at
 * config-parse time, not in `globalSetup`: playwright starts `webServer` first.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/nav-mobile.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NAV_MOBILE_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('nav-mobile');
const serveDir = buildScratchSite(NAV_MOBILE_GATE);

export default defineGateConfig({
  gate: 'nav-mobile',
  use: { baseURL: `http://localhost:${PORT}/`, colorScheme: 'light' },
  webServer: { serveDir, port: PORT },
});
