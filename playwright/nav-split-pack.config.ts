/**
 * Render gate for the split masthead's link packing and site-name wrapping.
 *
 * Runs tests/render-gates/site/nav-split-pack.spec.ts in BOTH chromium and
 * webkit against a scratch site with three nav items and a long site name.
 * The three-link set is short enough that `justify-content` decides where the
 * links sit: the many-link fixture in nav-split-order fills row 2 and cannot
 * show it. Row placement depends on measured widths, so only a real
 * layout engine can tell whether the toggles landed on the links' row.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/nav-split-pack.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NAV_SPLIT_PACK_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('nav-split-pack');
const serveDir = buildScratchSite(NAV_SPLIT_PACK_GATE);

export default defineGateConfig({
  gate: 'nav-split-pack',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light',
  },
  webServer: { serveDir, port: PORT },
});
