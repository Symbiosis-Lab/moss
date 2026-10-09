/**
 * Render gate for the split masthead's row order.
 *
 * Runs tests/render-gates/site/nav-split-order.spec.ts in BOTH chromium and
 * webkit against a scratch site with seven nav items, a long site name, and
 * a places root. Row placement depends on measured widths, so only a real
 * layout engine can tell whether the toggles landed on the links' row.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/nav-split-order.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NAV_SPLIT_ORDER_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('nav-split-order');
const serveDir = buildScratchSite(NAV_SPLIT_ORDER_GATE);

export default defineGateConfig({
  gate: 'nav-split-order',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light',
  },
  webServer: { serveDir, port: PORT },
});
