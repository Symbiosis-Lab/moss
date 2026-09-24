/**
 * Render gate for the nav toggle cluster (search / language / theme).
 *
 * Runs tests/render-gates/site/nav-toggle-cluster.spec.ts in BOTH chromium and
 * webkit against a scratch site that has search on and a translation, so all
 * three toggles are in `.nav-icons`.
 *
 * The scratch site is scaffolded and built by `buildScratchSite` below, at
 * config-parse time. That is deliberate and not a `globalSetup`: playwright
 * starts `webServer` BEFORE `globalSetup`, so a site built in `globalSetup`
 * does not exist yet when `webServer.cwd` is needed. See
 * tests/e2e/helpers/scratch-site.ts.
 *
 * Prereq: `cargo build -p moss-cli` must have produced target/debug/moss-cli.
 *
 *   npx playwright test -c playwright/nav-toggle-cluster.config.ts
 */
import './localhost-no-proxy';
import { buildScratchSite } from '../tests/e2e/helpers/scratch-site';
import { NAV_TOGGLE_CLUSTER_GATE } from '../tests/e2e/helpers/gate-sites';
import { defineGateConfig } from './define-gate-config';
import { gatePort } from './gate-ports';

const PORT = gatePort('nav-toggle-cluster');
const serveDir = buildScratchSite(NAV_TOGGLE_CLUSTER_GATE);

export default defineGateConfig({
  gate: 'nav-toggle-cluster',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    colorScheme: 'light', // pin light so dark-mode tokens do not interfere
  },
  webServer: { serveDir, port: PORT },
});
